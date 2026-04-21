use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Serde helpers
// ---------------------------------------------------------------------------

/// Deserializes a Vikunja date field.
///
/// Vikunja's Go backend uses the zero time ("0001-01-01T00:00:00Z") to signal
/// "no value" and may also send `null`.  Both map to `None`.
mod vikunja_date {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn deserialize<'de, D>(d: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Option::<String>::deserialize(d)?;
        Ok(s.and_then(|s| {
            if s.is_empty() || s.starts_with("0001-") {
                None
            } else {
                s.parse::<DateTime<Utc>>().ok()
            }
        }))
    }

    pub fn serialize<S>(dt: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match dt {
            Some(dt) => s.serialize_str(&dt.to_rfc3339()),
            None => s.serialize_none(),
        }
    }
}

/// Deserializes a field that Vikunja may send as either a proper value *or*
/// as JSON `null`.  When `null` (or absent), `T::default()` is used.
///
/// Go nil slices → JSON `null`, not `[]`.  `#[serde(default)]` alone does
/// not handle the *present-but-null* case; this wrapper does.
fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

/// A Vikunja project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VikunjaProject {
    pub id: i64,
    pub title: String,
}

/// A label attached to a Vikunja task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VikunjaLabel {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub hex_color: Option<String>,
}

/// A single reminder attached to a Vikunja task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VikunjaReminder {
    #[serde(default)]
    pub id: i64,
    #[serde(default, with = "vikunja_date")]
    pub reminder: Option<DateTime<Utc>>,
}

/// A Vikunja task as returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VikunjaTask {
    #[serde(default)]
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub done: bool,
    #[serde(default, with = "vikunja_date")]
    pub done_at: Option<DateTime<Utc>>,
    #[serde(default, with = "vikunja_date")]
    pub due_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub priority: i64,
    #[serde(default, with = "vikunja_date")]
    pub created: Option<DateTime<Utc>>,
    #[serde(default, with = "vikunja_date")]
    pub updated: Option<DateTime<Utc>>,
    /// Related tasks grouped by relation kind (e.g. "subtask", "parenttask").
    /// Go serialises a nil map as JSON `null`; `null_default` turns that into `{}`.
    #[serde(default, deserialize_with = "null_default")]
    pub related_tasks: HashMap<String, Vec<VikunjaTask>>,
    /// Labels attached to this task.
    /// Go serialises a nil slice as JSON `null`; `null_default` turns that into `[]`.
    #[serde(default, deserialize_with = "null_default")]
    pub labels: Vec<VikunjaLabel>,
    /// The project this task belongs to.
    #[serde(default)]
    pub project_id: i64,
    /// Reminder datetimes set on this task.
    /// Go serialises a nil slice as JSON `null`; `null_default` turns that into `[]`.
    #[serde(default, deserialize_with = "null_default")]
    pub reminder_dates: Vec<VikunjaReminder>,
}

/// Request body for creating or updating a task.
#[derive(Debug, Serialize)]
pub struct TaskPayload {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none", with = "vikunja_date")]
    pub due_date: Option<DateTime<Utc>>,
    pub priority: i64,
}

/// Request body for creating a task relation.
#[derive(Debug, Serialize)]
pub struct CreateRelation {
    pub task_id: i64,
    pub other_task_id: i64,
    pub relation_kind: String,
}
