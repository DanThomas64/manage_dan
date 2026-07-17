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
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            group_id       INTEGER REFERENCES shopping_list_groups(id) ON DELETE CASCADE,
            name           TEXT NOT NULL,
            has_checkboxes INTEGER NOT NULL DEFAULT 1,
            has_quick_add  INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS shopping_items (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            category_id INTEGER NOT NULL
                            REFERENCES shopping_categories(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            quantity    TEXT,
            checked     INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL,
            position    INTEGER NOT NULL DEFAULT 0
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

    // ── Migration: add has_checkboxes to shopping_categories ─────────────
    let has_checkboxes_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('shopping_categories') WHERE name = 'has_checkboxes'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        != 0;

    if !has_checkboxes_exists {
        conn.execute_batch(
            "ALTER TABLE shopping_categories ADD COLUMN has_checkboxes INTEGER NOT NULL DEFAULT 1",
        )
        .map_err(|e| DbLibError::Sqlite(e))?;
    }

    // ── Migration: add has_quick_add to shopping_categories ──────────────
    let has_quick_add_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('shopping_categories') WHERE name = 'has_quick_add'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        != 0;

    if !has_quick_add_exists {
        conn.execute_batch(
            "ALTER TABLE shopping_categories ADD COLUMN has_quick_add INTEGER NOT NULL DEFAULT 1",
        )
        .map_err(|e| DbLibError::Sqlite(e))?;
    }

    // ── Migration: add position to shopping_items ─────────────────────────
    let position_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('shopping_items') WHERE name = 'position'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        != 0;

    if !position_exists {
        conn.execute_batch(
            "ALTER TABLE shopping_items ADD COLUMN position INTEGER NOT NULL DEFAULT 0",
        )
        .map_err(|e| DbLibError::Sqlite(e))?;
        // Seed positions for existing items using their id (preserves creation order).
        conn.execute_batch("UPDATE shopping_items SET position = id WHERE position = 0")
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
            "SELECT id, group_id, name, has_checkboxes, has_quick_add FROM shopping_categories
             WHERE group_id = ?1
             ORDER BY name",
        )?;
        let rows: rusqlite::Result<Vec<ListCategory>> = stmt
            .query_map(params![group_id], |row| {
                let has_checkboxes: i64 = row.get(3)?;
                let has_quick_add: i64 = row.get(4)?;
                Ok(ListCategory {
                    id: row.get(0)?,
                    group_id: row.get(1)?,
                    name: row.get(2)?,
                    has_checkboxes: has_checkboxes != 0,
                    has_quick_add: has_quick_add != 0,
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
    Ok(ListCategory { id, group_id, name, has_checkboxes: true, has_quick_add: true })
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

/// Returns all items for the given category, ordered by position.
pub async fn list_items(category_id: i64) -> ListsLibResult<Vec<ListItem>> {
    db::execute_async(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, category_id, name, quantity, checked, created_at, position
             FROM shopping_items
             WHERE category_id = ?1
             ORDER BY position ASC",
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

    let (id, position) = db::execute_async(move |conn| {
        let position: i64 = conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM shopping_items WHERE category_id = ?1",
            params![category_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO shopping_items (category_id, name, quantity, checked, created_at, position)
             VALUES (?1, ?2, ?3, 0, ?4, ?5)",
            params![category_id, name, qty, now, position],
        )?;
        Ok((conn.last_insert_rowid(), position))
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
        position,
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

/// Renames a category.
pub async fn rename_category(category_id: i64, name: String) -> ListsLibResult {
    db::execute_async(move |conn| {
        conn.execute(
            "UPDATE shopping_categories SET name = ?1 WHERE id = ?2",
            params![name, category_id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| ListsLibError::Db(e))
}

/// Sets whether a category uses checkboxes.
pub async fn set_checkboxes(category_id: i64, has_checkboxes: bool) -> ListsLibResult {
    let val: i64 = if has_checkboxes { 1 } else { 0 };
    db::execute_async(move |conn| {
        conn.execute(
            "UPDATE shopping_categories SET has_checkboxes = ?1 WHERE id = ?2",
            params![val, category_id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| ListsLibError::Db(e))
}

/// Sets whether a category shows its "Quick Add" pane of saved common items.
pub async fn set_quick_add(category_id: i64, has_quick_add: bool) -> ListsLibResult {
    let val: i64 = if has_quick_add { 1 } else { 0 };
    db::execute_async(move |conn| {
        conn.execute(
            "UPDATE shopping_categories SET has_quick_add = ?1 WHERE id = ?2",
            params![val, category_id],
        )?;
        Ok(())
    })
    .await
    .map_err(|e| ListsLibError::Db(e))
}

/// Reorders items in a category by assigning new positions matching the given id order.
pub async fn reorder_items(category_id: i64, ids: Vec<i64>) -> ListsLibResult {
    db::execute_async(move |conn| {
        for (pos, id) in ids.iter().enumerate() {
            conn.execute(
                "UPDATE shopping_items SET position = ?1 WHERE id = ?2 AND category_id = ?3",
                params![pos as i64, id, category_id],
            )?;
        }
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
// Stats
// ---------------------------------------------------------------------------

/// Aggregate statistics for the lists subsystem.
#[derive(Debug, Default)]
pub struct ListStats {
    /// Number of list groups (e.g. "Shopping Lists", "General Lists").
    pub groups: usize,
    /// Number of named lists / categories across all groups.
    pub lists: usize,
    /// Total number of items across all lists.
    pub items: usize,
    /// Items that are not yet checked off.
    pub items_pending: usize,
}

/// Returns aggregate counts for groups, lists, and items.
pub async fn stats() -> ListsLibResult<ListStats> {
    db::execute_async(|conn| {
        let groups: i64 = conn.query_row(
            "SELECT COUNT(*) FROM shopping_list_groups", [], |r| r.get(0)
        ).unwrap_or(0);
        let lists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM shopping_categories", [], |r| r.get(0)
        ).unwrap_or(0);
        let items: i64 = conn.query_row(
            "SELECT COUNT(*) FROM shopping_items", [], |r| r.get(0)
        ).unwrap_or(0);
        let items_pending: i64 = conn.query_row(
            "SELECT COUNT(*) FROM shopping_items WHERE checked = 0", [], |r| r.get(0)
        ).unwrap_or(0);
        Ok(ListStats {
            groups:        groups        as usize,
            lists:         lists         as usize,
            items:         items         as usize,
            items_pending: items_pending as usize,
        })
    })
    .await
    .map_err(ListsLibError::Db)
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
        position: row.get(6)?,
    })
}
