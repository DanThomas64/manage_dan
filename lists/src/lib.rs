//! List subsystem.
//!
//! Manages list groups (e.g. "Shopping Lists", "General Lists"), the named
//! lists within each group (e.g. "Groceries", "Movies to Watch"), and the
//! items on each list.  Everything is persisted in the shared SQLite database.

pub mod models;
pub mod lists_error;
pub mod lists_prelude;

use chrono::Local;
use db::db_error::DbLibError;
use rusqlite::{params, OptionalExtension};

use crate::models::{ListGroup, ListCategory, ListItem, CommonItem};
use crate::lists_prelude::*;

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

/// Initialises the lists subsystem.
///
/// Creates tables if they do not already exist, runs any pending migrations,
/// and seeds default groups and categories.
pub fn init() -> ListsLibResult {
    info!("initializing lists subsystem");

    let conn = rusqlite::Connection::open(db::DB_FILE)
        .map_err(|e| DbLibError::Sqlite(e))?;

    // ── Create tables ──────────────────────────────────────────────────────
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS shopping_list_groups (
            id   INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS shopping_categories (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            group_id INTEGER REFERENCES shopping_list_groups(id) ON DELETE CASCADE,
            name     TEXT NOT NULL
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

        CREATE TABLE IF NOT EXISTS list_common_items (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            category_id INTEGER NOT NULL
                            REFERENCES shopping_categories(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            quantity    TEXT
        );
        ",
    )
    .map_err(|e| DbLibError::Sqlite(e))?;

    // ── Seed default groups ────────────────────────────────────────────────
    for name in &["Shopping Lists", "General Lists"] {
        conn.execute(
            "INSERT OR IGNORE INTO shopping_list_groups (name) VALUES (?1)",
            params![name],
        )
        .map_err(|e| DbLibError::Sqlite(e))?;
    }

    // ── Migration: add group_id column to existing installs ───────────────
    let group_id_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('shopping_categories') WHERE name = 'group_id'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        != 0;

    if !group_id_exists {
        conn.execute_batch(
            "ALTER TABLE shopping_categories ADD COLUMN group_id INTEGER \
             REFERENCES shopping_list_groups(id) ON DELETE CASCADE",
        )
        .map_err(|e| DbLibError::Sqlite(e))?;
    }

    // ── Migration: assign orphaned categories to "Shopping Lists" ──────────
    conn.execute(
        "UPDATE shopping_categories
         SET group_id = (SELECT id FROM shopping_list_groups WHERE name = 'Shopping Lists' LIMIT 1)
         WHERE group_id IS NULL",
        [],
    )
    .map_err(|e| DbLibError::Sqlite(e))?;

    // ── Seed default categories (only when a group has none yet) ───────────
    let shopping_gid: Option<i64> = conn
        .query_row(
            "SELECT id FROM shopping_list_groups WHERE name = 'Shopping Lists'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| DbLibError::Sqlite(e))?;

    if let Some(gid) = shopping_gid {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM shopping_categories WHERE group_id = ?1",
                params![gid],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if count == 0 {
            for name in &["Groceries", "Toiletries", "Hardware", "Pharmacy"] {
                conn.execute(
                    "INSERT OR IGNORE INTO shopping_categories (group_id, name) VALUES (?1, ?2)",
                    params![gid, name],
                )
                .map_err(|e| DbLibError::Sqlite(e))?;
            }
        }
    }

    let general_gid: Option<i64> = conn
        .query_row(
            "SELECT id FROM shopping_list_groups WHERE name = 'General Lists'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| DbLibError::Sqlite(e))?;

    if let Some(gid) = general_gid {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM shopping_categories WHERE group_id = ?1",
                params![gid],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if count == 0 {
            for name in &["Movies to Watch", "Books to Read"] {
                conn.execute(
                    "INSERT OR IGNORE INTO shopping_categories (group_id, name) VALUES (?1, ?2)",
                    params![gid, name],
                )
                .map_err(|e| DbLibError::Sqlite(e))?;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// List Groups
// ---------------------------------------------------------------------------

/// Returns all list groups ordered by insertion order.
pub async fn list_groups() -> ListsLibResult<Vec<ListGroup>> {
    db::execute_async(|conn| {
        let mut stmt =
            conn.prepare("SELECT id, name FROM shopping_list_groups ORDER BY id")?;
        let rows: rusqlite::Result<Vec<ListGroup>> = stmt
            .query_map([], |row| Ok(ListGroup { id: row.get(0)?, name: row.get(1)? }))?
            .collect();
        rows
    })
    .await
    .map_err(ListsLibError::Db)
}

/// Creates a new list group and returns it.
pub async fn add_group(name: &str) -> ListsLibResult<ListGroup> {
    let name = name.to_string();
    let name_clone = name.clone();
    let id = db::execute_async(move |conn| {
        conn.execute(
            "INSERT INTO shopping_list_groups (name) VALUES (?1)",
            params![name_clone],
        )?;
        Ok(conn.last_insert_rowid())
    })
    .await
    .map_err(ListsLibError::Db)?;
    Ok(ListGroup { id, name })
}

/// Deletes a list group and all its categories and items.
pub async fn delete_group(id: i64) -> ListsLibResult {
    db::execute_async(move |conn| {
        conn.execute(
            "DELETE FROM shopping_list_groups WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    })
    .await
    .map_err(ListsLibError::Db)
}

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

/// Returns all categories in the given group, ordered by name.
pub async fn list_categories(group_id: i64) -> ListsLibResult<Vec<ListCategory>> {
    db::execute_async(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, group_id, name FROM shopping_categories
             WHERE group_id = ?1
             ORDER BY name",
        )?;
        let rows: rusqlite::Result<Vec<ListCategory>> = stmt
            .query_map(params![group_id], |row| {
                Ok(ListCategory {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    name: row.get(2)?,
                })
            })?
            .collect();
        rows
    })
    .await
    .map_err(ListsLibError::Db)
}

/// Creates a new category in the given group and returns it.
pub async fn add_category(group_id: i64, name: &str) -> ListsLibResult<ListCategory> {
    let name = name.to_string();
    let name_clone = name.clone();
    let id = db::execute_async(move |conn| {
        conn.execute(
            "INSERT INTO shopping_categories (group_id, name) VALUES (?1, ?2)",
            params![group_id, name_clone],
        )?;
        Ok(conn.last_insert_rowid())
    })
    .await
    .map_err(ListsLibError::Db)?;
    Ok(ListCategory { id, group_id, name })
}

/// Deletes a category and all its items.
pub async fn delete_category(id: i64) -> ListsLibResult {
    db::execute_async(move |conn| {
        conn.execute(
            "DELETE FROM shopping_categories WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    })
    .await
    .map_err(ListsLibError::Db)
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/// Returns all items for the given category, unchecked items first.
pub async fn list_items(category_id: i64) -> ListsLibResult<Vec<ListItem>> {
    db::execute_async(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, category_id, name, quantity, checked, created_at
             FROM shopping_items
             WHERE category_id = ?1
             ORDER BY checked ASC, created_at ASC",
        )?;
        let rows: rusqlite::Result<Vec<ListItem>> =
            stmt.query_map(params![category_id], row_to_item)?.collect();
        rows
    })
    .await
    .map_err(|e| ListsLibError::Db(e))
}

/// Adds an item to a category.  `quantity` is an optional free-text string
/// such as `"2"`, `"500g"`, or `"x3"`.
pub async fn add_item(
    category_id: i64,
    name: &str,
    quantity: Option<&str>,
) -> ListsLibResult<ListItem> {
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
    .map_err(|e| ListsLibError::Db(e))?;

    Ok(ListItem {
        id,
        category_id,
        name: name_ret,
        quantity: qty_ret,
        checked: false,
        created_at: Local::now(),
    })
}

/// Sets the checked state of an item.
pub async fn check_item(id: i64, checked: bool) -> ListsLibResult {
    let checked_int: i64 = if checked { 1 } else { 0 };
    db::execute_async(move |conn| {
        conn.execute(
            "UPDATE shopping_items SET checked = ?1 WHERE id = ?2",
            params![checked_int, id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| ListsLibError::Db(e))
}

/// Deletes a single item.
pub async fn delete_item(id: i64) -> ListsLibResult {
    db::execute_async(move |conn| {
        conn.execute("DELETE FROM shopping_items WHERE id = ?1", params![id])?;
        Ok(())
    })
    .await
    .map_err(|e| ListsLibError::Db(e))
}

/// Removes all checked items from a category.
pub async fn clear_checked(category_id: i64) -> ListsLibResult {
    db::execute_async(move |conn| {
        conn.execute(
            "DELETE FROM shopping_items WHERE category_id = ?1 AND checked = 1",
            params![category_id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| ListsLibError::Db(e))
}

// ---------------------------------------------------------------------------
// Printing
// ---------------------------------------------------------------------------

/// Prints the list for the given category.
pub async fn print_list(category_id: i64) -> ListsLibResult {
    let category_name = db::execute_async(move |conn| {
        conn.query_row(
            "SELECT name FROM shopping_categories WHERE id = ?1",
            params![category_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .map_err(|e| ListsLibError::Db(e))?
    .ok_or(ListsLibError::CategoryNotFound(category_id))?;

    let items = list_items(category_id).await?;

    let width = printer::line_width();
    let sep = "-".repeat(width);

    let pending: Vec<&ListItem> = items.iter().filter(|i| !i.checked).collect();
    let done: Vec<&ListItem> = items.iter().filter(|i| i.checked).collect();

    let origin = format!(
        "{} item{} remaining",
        pending.len(),
        if pending.len() == 1 { "" } else { "s" }
    );

    let mut lines: Vec<String> = Vec::new();

    for item in &pending {
        lines.push(format_item(item));
    }

    if !done.is_empty() {
        lines.push(String::new());
        lines.push(sep.clone());
        lines.push("Already obtained:".to_string());
        for item in &done {
            lines.push(format_item(item));
        }
    }

    lines.push(String::new());
    lines.push(sep);
    lines.push(format!(
        "Printed: {}",
        Local::now().format("%a %d %b %Y  %H:%M")
    ));

    let title = format!("LIST: {}", category_name.to_uppercase());

    printer::PrintJob::new(origin, title, lines)
        .execute(0, 0)
        .await
        .map_err(ListsLibError::Print)
}

// ---------------------------------------------------------------------------
// Common Items
// ---------------------------------------------------------------------------

/// Returns all common item templates for the given category, ordered by name.
pub async fn list_common_items(category_id: i64) -> ListsLibResult<Vec<CommonItem>> {
    db::execute_async(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, category_id, name, quantity
             FROM list_common_items
             WHERE category_id = ?1
             ORDER BY name",
        )?;
        let rows: rusqlite::Result<Vec<CommonItem>> = stmt
            .query_map(params![category_id], |row| {
                Ok(CommonItem {
                    id: row.get(0)?,
                    category_id: row.get(1)?,
                    name: row.get(2)?,
                    quantity: row.get(3)?,
                })
            })?
            .collect();
        rows
    })
    .await
    .map_err(ListsLibError::Db)
}

/// Saves a new common item template for the given category.
pub async fn add_common_item(
    category_id: i64,
    name: &str,
    quantity: Option<&str>,
) -> ListsLibResult<CommonItem> {
    let name = name.to_string();
    let qty = quantity.map(str::to_string);
    let name_ret = name.clone();
    let qty_ret = qty.clone();
    let id = db::execute_async(move |conn| {
        conn.execute(
            "INSERT INTO list_common_items (category_id, name, quantity) VALUES (?1, ?2, ?3)",
            params![category_id, name, qty],
        )?;
        Ok(conn.last_insert_rowid())
    })
    .await
    .map_err(ListsLibError::Db)?;
    Ok(CommonItem { id, category_id, name: name_ret, quantity: qty_ret })
}

/// Deletes a common item template.
pub async fn delete_common_item(id: i64) -> ListsLibResult {
    db::execute_async(move |conn| {
        conn.execute("DELETE FROM list_common_items WHERE id = ?1", params![id])?;
        Ok(())
    })
    .await
    .map_err(ListsLibError::Db)
}

/// Adds a live list item from a common item template (copies name and quantity).
pub async fn add_item_from_common(common_id: i64) -> ListsLibResult<ListItem> {
    let common = db::execute_async(move |conn| {
        conn.query_row(
            "SELECT id, category_id, name, quantity FROM list_common_items WHERE id = ?1",
            params![common_id],
            |row| Ok(CommonItem {
                id: row.get(0)?,
                category_id: row.get(1)?,
                name: row.get(2)?,
                quantity: row.get(3)?,
            }),
        )
    })
    .await
    .map_err(ListsLibError::Db)?;

    add_item(common.category_id, &common.name, common.quantity.as_deref()).await
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_item(item: &ListItem) -> String {
    let marker = if item.checked { "[x]" } else { "[ ]" };
    match &item.quantity {
        Some(qty) => format!("{} {} ({})", marker, item.name, qty),
        None => format!("{} {}", marker, item.name),
    }
}

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<ListItem> {
    let checked: i64 = row.get(4)?;
    let created_str: String = row.get(5)?;
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_str)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|_| Local::now());

    Ok(ListItem {
        id: row.get(0)?,
        category_id: row.get(1)?,
        name: row.get(2)?,
        quantity: row.get(3)?,
        checked: checked != 0,
        created_at,
    })
}
