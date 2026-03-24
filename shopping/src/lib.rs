//! Shopping list subsystem.
//!
//! Manages named shopping lists (categories) and their items, persisted in
//! the shared SQLite database.  Each category (e.g. "Groceries", "Toiletries")
//! holds an independent list of items that can be checked off and printed.

pub mod models;
pub mod shopping_error;
pub mod shopping_prelude;

use chrono::Local;
use db::db_error::DbLibError;
use rusqlite::{params, OptionalExtension};

use crate::models::{ShoppingCategory, ShoppingItem};
use crate::shopping_prelude::*;

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

/// Initialises the shopping subsystem.
///
/// Creates the `shopping_categories` and `shopping_items` tables if they do
/// not already exist and seeds a handful of default categories.
pub fn init() -> ShoppingLibResult {
    info!("initializing shopping");

    let conn = rusqlite::Connection::open(db::DB_FILE)
        .map_err(|e| DbLibError::Sqlite(e))?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS shopping_categories (
            id   INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS shopping_items (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            category_id INTEGER NOT NULL
                            REFERENCES shopping_categories(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            quantity    TEXT,
            checked     INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL
        );
        ",
    )
    .map_err(|e| DbLibError::Sqlite(e))?;

    // Seed default categories (no-op if they already exist).
    for name in &["Groceries", "Toiletries", "Hardware", "Pharmacy"] {
        conn.execute(
            "INSERT OR IGNORE INTO shopping_categories (name) VALUES (?1)",
            params![name],
        )
        .map_err(|e| DbLibError::Sqlite(e))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

/// Returns all shopping categories ordered by name.
pub async fn list_categories() -> ShoppingLibResult<Vec<ShoppingCategory>> {
    db::execute_async(|conn| {
        let mut stmt =
            conn.prepare("SELECT id, name FROM shopping_categories ORDER BY name")?;
        let rows: rusqlite::Result<Vec<ShoppingCategory>> = stmt
            .query_map([], |row| {
                Ok(ShoppingCategory {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect();
        rows
    })
    .await
    .map_err(|e| ShoppingLibError::Db(e))
}

/// Creates a new category and returns it (with its assigned id).
pub async fn add_category(name: &str) -> ShoppingLibResult<ShoppingCategory> {
    let name = name.to_string();
    let name_clone = name.clone();
    let id = db::execute_async(move |conn| {
        conn.execute(
            "INSERT INTO shopping_categories (name) VALUES (?1)",
            params![name_clone],
        )?;
        Ok(conn.last_insert_rowid())
    })
    .await
    .map_err(|e| ShoppingLibError::Db(e))?;

    Ok(ShoppingCategory { id, name })
}

/// Deletes a category and all its items.
pub async fn delete_category(id: i64) -> ShoppingLibResult {
    db::execute_async(move |conn| {
        conn.execute(
            "DELETE FROM shopping_categories WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| ShoppingLibError::Db(e))
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/// Returns all items for the given category, unchecked items first.
pub async fn list_items(category_id: i64) -> ShoppingLibResult<Vec<ShoppingItem>> {
    db::execute_async(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, category_id, name, quantity, checked, created_at
             FROM shopping_items
             WHERE category_id = ?1
             ORDER BY checked ASC, created_at ASC",
        )?;
        let rows: rusqlite::Result<Vec<ShoppingItem>> =
            stmt.query_map(params![category_id], row_to_item)?.collect();
        rows
    })
    .await
    .map_err(|e| ShoppingLibError::Db(e))
}

/// Adds an item to a category.  `quantity` is an optional free-text string
/// such as `"2"`, `"500g"`, or `"x3"`.
pub async fn add_item(
    category_id: i64,
    name: &str,
    quantity: Option<&str>,
) -> ShoppingLibResult<ShoppingItem> {
    let name = name.to_string();
    let qty = quantity.map(str::to_string);
    let now = Local::now().to_rfc3339();
    let name_ret = name.clone();
    let qty_ret = qty.clone();

    let id = db::execute_async(move |conn| {
        conn.execute(
            "INSERT INTO shopping_items (category_id, name, quantity, checked, created_at)
             VALUES (?1, ?2, ?3, 0, ?4)",
            params![category_id, name, qty, now],
        )?;
        Ok(conn.last_insert_rowid())
    })
    .await
    .map_err(|e| ShoppingLibError::Db(e))?;

    Ok(ShoppingItem {
        id,
        category_id,
        name: name_ret,
        quantity: qty_ret,
        checked: false,
        created_at: Local::now(),
    })
}

/// Sets the checked state of an item.
pub async fn check_item(id: i64, checked: bool) -> ShoppingLibResult {
    let checked_int: i64 = if checked { 1 } else { 0 };
    db::execute_async(move |conn| {
        conn.execute(
            "UPDATE shopping_items SET checked = ?1 WHERE id = ?2",
            params![checked_int, id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| ShoppingLibError::Db(e))
}

/// Deletes a single item.
pub async fn delete_item(id: i64) -> ShoppingLibResult {
    db::execute_async(move |conn| {
        conn.execute("DELETE FROM shopping_items WHERE id = ?1", params![id])?;
        Ok(())
    })
    .await
    .map_err(|e| ShoppingLibError::Db(e))
}

/// Removes all checked items from a category — useful for resetting a list
/// after a shopping trip.
pub async fn clear_checked(category_id: i64) -> ShoppingLibResult {
    db::execute_async(move |conn| {
        conn.execute(
            "DELETE FROM shopping_items WHERE category_id = ?1 AND checked = 1",
            params![category_id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| ShoppingLibError::Db(e))
}

// ---------------------------------------------------------------------------
// Printing
// ---------------------------------------------------------------------------

/// Prints the shopping list for the given category.
///
/// Items are grouped with unchecked first, then checked (already obtained).
/// The ticket title is the category name; the origin line shows the item count.
pub async fn print_list(category_id: i64) -> ShoppingLibResult {
    // Fetch category name
    let category_name = db::execute_async(move |conn| {
        conn.query_row(
            "SELECT name FROM shopping_categories WHERE id = ?1",
            params![category_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .map_err(|e| ShoppingLibError::Db(e))?
    .ok_or(ShoppingLibError::CategoryNotFound(category_id))?;

    let items = list_items(category_id).await?;

    let width = printer::line_width();
    let sep = "-".repeat(width);

    let pending: Vec<&ShoppingItem> = items.iter().filter(|i| !i.checked).collect();
    let done: Vec<&ShoppingItem> = items.iter().filter(|i| i.checked).collect();

    let origin = format!(
        "{} item{} remaining",
        pending.len(),
        if pending.len() == 1 { "" } else { "s" }
    );

    let mut lines: Vec<String> = Vec::new();

    // Pending items
    for item in &pending {
        lines.push(format_item(item));
    }

    // Checked items (if any)
    if !done.is_empty() {
        lines.push(String::new());
        lines.push(sep.clone());
        lines.push("Already obtained:".to_string());
        for item in &done {
            lines.push(format_item(item));
        }
    }

    // Footer
    lines.push(String::new());
    lines.push(sep);
    lines.push(format!(
        "Printed: {}",
        Local::now().format("%a %d %b %Y  %H:%M")
    ));

    let title = format!("SHOPPING: {}", category_name.to_uppercase());

    printer::PrintJob::new(origin, title, lines)
        .execute(0, 0)
        .await
        .map_err(ShoppingLibError::Print)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_item(item: &ShoppingItem) -> String {
    let marker = if item.checked { "[x]" } else { "[ ]" };
    match &item.quantity {
        Some(qty) => format!("{} {} ({})", marker, item.name, qty),
        None => format!("{} {}", marker, item.name),
    }
}

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<ShoppingItem> {
    let checked: i64 = row.get(4)?;
    let created_str: String = row.get(5)?;
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now());

    Ok(ShoppingItem {
        id: row.get(0)?,
        category_id: row.get(1)?,
        name: row.get(2)?,
        quantity: row.get(3)?,
        checked: checked != 0,
        created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_succeeds() {
        assert!(init().is_ok());
    }
}
