pub mod models;
pub mod notes_error;
pub mod notes_prelude;

use crate::notes_prelude::*;
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static NOTES_DIR: OnceLock<String> = OnceLock::new();

pub use models::{CreateNoteRequest, Note, NoteStatus, UpdateNoteRequest};

pub fn init(dir: &str) -> NotesLibResult {
    info!("initializing notes");
    std::fs::create_dir_all(dir).map_err(|e| {
        NotesLibError::CannotInitialize(format!("cannot create notes dir '{}': {}", dir, e))
    })?;
    NOTES_DIR
        .set(dir.to_string())
        .map_err(|_| NotesLibError::CannotInitialize("notes already initialized".to_string()))?;
    sync_from_disk()?;
    Ok(())
}

fn notes_dir() -> &'static str {
    NOTES_DIR.get().expect("notes subsystem not initialized")
}

fn note_path(uuid: &str, folder: &str) -> PathBuf {
    let base = Path::new(notes_dir());
    if folder.is_empty() {
        base.join(format!("{}.md", uuid))
    } else {
        base.join(folder).join(format!("{}.md", uuid))
    }
}

fn write_md_file(note: &Note) -> NotesLibResult {
    let path = note_path(&note.uuid, &note.folder);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tags_json = serde_json::to_string(&note.tags)?;
    let file_content = format!(
        "---\nuuid: {}\ntitle: {}\nstatus: {}\ntags: {}\nfolder: {}\ncreated_at: {}\nupdated_at: {}\n---\n\n{}",
        note.uuid,
        note.title,
        note.status.as_str(),
        tags_json,
        note.folder,
        note.created_at.to_rfc3339(),
        note.updated_at.to_rfc3339(),
        note.content,
    );
    std::fs::write(&path, file_content)?;
    Ok(())
}

fn parse_md_file(path: &Path) -> NotesLibResult<Note> {
    let raw = std::fs::read_to_string(path)?;

    let rest = raw.strip_prefix("---\n").ok_or_else(|| {
        NotesLibError::InvalidFrontmatter(path.display().to_string())
    })?;
    let end_idx = rest.find("\n---\n").ok_or_else(|| {
        NotesLibError::InvalidFrontmatter(path.display().to_string())
    })?;

    let fm_block = &rest[..end_idx];
    let content = rest[end_idx + 5..].trim_start_matches('\n').to_string();

    let mut uuid = String::new();
    let mut title = String::new();
    let mut status = NoteStatus::Raw;
    let mut tags: Vec<String> = Vec::new();
    let mut folder = String::new();
    let mut created_at = chrono::Local::now();
    let mut updated_at = chrono::Local::now();

    for line in fm_block.lines() {
        if let Some((key, val)) = line.split_once(": ") {
            match key {
                "uuid" => uuid = val.to_string(),
                "title" => title = val.to_string(),
                "status" => status = NoteStatus::from_str(val),
                "tags" => tags = serde_json::from_str(val).unwrap_or_default(),
                "folder" => folder = val.to_string(),
                "created_at" => {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(val) {
                        created_at = dt.with_timezone(&chrono::Local);
                    }
                }
                "updated_at" => {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(val) {
                        updated_at = dt.with_timezone(&chrono::Local);
                    }
                }
                _ => {}
            }
        }
    }

    Ok(Note {
        id: None,
        uuid,
        title,
        content,
        status,
        tags,
        folder,
        created_at,
        updated_at,
    })
}

fn sync_from_disk() -> NotesLibResult {
    let conn = rusqlite::Connection::open(db::DB_FILE).map_err(|e| {
        NotesLibError::CannotInitialize(format!("cannot open db for sync: {}", e))
    })?;
    sync_dir(Path::new(notes_dir()), &conn)
}

fn sync_dir(dir: &Path, conn: &rusqlite::Connection) -> NotesLibResult {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            sync_dir(&path, conn)?;
        } else if path.extension().map_or(false, |e| e == "md") {
            match parse_md_file(&path) {
                Ok(note) if !note.uuid.is_empty() => {
                    let tags_json = serde_json::to_string(&note.tags).unwrap_or_else(|_| "[]".to_string());
                    if let Err(e) = conn.execute(
                        "INSERT INTO notes (uuid, title, content, status, tags, folder, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                         ON CONFLICT(uuid) DO UPDATE SET
                             title=excluded.title, content=excluded.content,
                             status=excluded.status, tags=excluded.tags,
                             folder=excluded.folder, updated_at=excluded.updated_at",
                        params![
                            note.uuid, note.title, note.content,
                            note.status.as_str(), tags_json, note.folder,
                            note.created_at.to_rfc3339(), note.updated_at.to_rfc3339(),
                        ],
                    ) {
                        tracing::warn!("sync: db upsert failed for {}: {}", path.display(), e);
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("sync: skipping {}: {}", path.display(), e),
            }
        }
    }
    Ok(())
}

fn parse_note_row(row: &rusqlite::Row) -> rusqlite::Result<Note> {
    let parse_dt = |s: String| {
        chrono::DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&chrono::Local))
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
                0, rusqlite::types::Type::Text, Box::new(e),
            ))
    };
    let tags_json: String = row.get(5)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    let status_str: String = row.get(4)?;
    Ok(Note {
        id: Some(row.get(0)?),
        uuid: row.get(1)?,
        title: row.get(2)?,
        content: row.get(3)?,
        status: NoteStatus::from_str(&status_str),
        tags,
        folder: row.get(6)?,
        created_at: row.get::<_, String>(7).and_then(parse_dt)?,
        updated_at: row.get::<_, String>(8).and_then(parse_dt)?,
    })
}

pub async fn create(req: CreateNoteRequest) -> NotesLibResult<Note> {
    let uuid = uuid::Uuid::new_v4().to_string();
    let now = chrono::Local::now();
    let note = Note {
        id: None,
        uuid,
        title: req.title.unwrap_or_default(),
        content: req.content,
        status: NoteStatus::Raw,
        tags: req.tags.unwrap_or_default(),
        folder: req.folder.unwrap_or_default(),
        created_at: now,
        updated_at: now,
    };

    write_md_file(&note)?;

    let tags_json = serde_json::to_string(&note.tags)?;
    let now_str = now.to_rfc3339();
    let (u, t, c, tj, f, ns) = (
        note.uuid.clone(), note.title.clone(), note.content.clone(),
        tags_json, note.folder.clone(), now_str,
    );

    let id = db::execute_async(move |conn| {
        conn.execute(
            "INSERT INTO notes (uuid, title, content, status, tags, folder, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'raw', ?4, ?5, ?6, ?6)",
            params![u, t, c, tj, f, ns],
        )?;
        Ok(conn.last_insert_rowid())
    })
    .await
    .map_err(NotesLibError::Db)?;

    Ok(Note { id: Some(id), ..note })
}

pub async fn get(uuid: &str) -> NotesLibResult<Note> {
    let u_db = uuid.to_string();
    let u_err = uuid.to_string();
    let uuid_owned = uuid.to_string();

    let folder = db::execute_async(move |conn| {
        conn.query_row(
            "SELECT folder FROM notes WHERE uuid = ?1",
            params![u_db],
            |row| row.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .map_err(NotesLibError::Db)?
    .ok_or_else(|| NotesLibError::NotFound(u_err))?;

    let path = note_path(&uuid_owned, &folder);
    parse_md_file(&path)
}

pub async fn list(
    status: Option<NoteStatus>,
    folder: Option<String>,
    tag: Option<String>,
) -> NotesLibResult<Vec<Note>> {
    let status_str = status.as_ref().map(|s| s.as_str().to_string());

    let mut notes = db::execute_async(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, uuid, title, content, status, tags, folder, created_at, updated_at
             FROM notes
             WHERE (?1 IS NULL OR status = ?1)
               AND (?2 IS NULL OR folder = ?2)
             ORDER BY updated_at DESC",
        )?;
        let rows: rusqlite::Result<Vec<Note>> = stmt
            .query_map(params![status_str, folder], parse_note_row)?
            .collect();
        rows
    })
    .await
    .map_err(NotesLibError::Db)?;

    if let Some(tag_filter) = tag {
        notes.retain(|n| n.tags.iter().any(|t| t == &tag_filter));
    }

    Ok(notes)
}

pub async fn update(uuid: &str, req: UpdateNoteRequest) -> NotesLibResult<Note> {
    let current = get(uuid).await?;

    let new_folder = req.folder.unwrap_or(current.folder.clone());
    let updated = Note {
        id: current.id,
        uuid: uuid.to_string(),
        title: req.title.unwrap_or(current.title),
        content: req.content.unwrap_or(current.content),
        status: req.status.unwrap_or(current.status),
        tags: req.tags.unwrap_or(current.tags),
        folder: new_folder,
        created_at: current.created_at,
        updated_at: chrono::Local::now(),
    };

    if updated.folder != current.folder {
        let old_path = note_path(uuid, &current.folder);
        let _ = std::fs::remove_file(old_path);
    }

    write_md_file(&updated)?;

    let tags_json = serde_json::to_string(&updated.tags)?;
    let now_str = updated.updated_at.to_rfc3339();
    let (u, t, c, s, tj, f, ns) = (
        uuid.to_string(), updated.title.clone(), updated.content.clone(),
        updated.status.as_str().to_string(), tags_json, updated.folder.clone(), now_str,
    );

    db::execute_async(move |conn| {
        conn.execute(
            "UPDATE notes SET title=?1, content=?2, status=?3, tags=?4, folder=?5, updated_at=?6
             WHERE uuid=?7",
            params![t, c, s, tj, f, ns, u],
        )?;
        Ok(())
    })
    .await
    .map_err(NotesLibError::Db)?;

    Ok(updated)
}

pub async fn delete(uuid: &str) -> NotesLibResult {
    let note = get(uuid).await?;
    let uuid_owned = uuid.to_string();

    db::execute_async(move |conn| {
        conn.execute("DELETE FROM notes WHERE uuid=?1", params![uuid_owned])?;
        Ok(())
    })
    .await
    .map_err(NotesLibError::Db)?;

    let path = note_path(&note.uuid, &note.folder);
    let _ = std::fs::remove_file(path);

    Ok(())
}

pub async fn search(query: &str) -> NotesLibResult<Vec<Note>> {
    let query = query.to_string();
    db::execute_async(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT n.id, n.uuid, n.title, n.content, n.status, n.tags, n.folder,
                    n.created_at, n.updated_at
             FROM notes n
             JOIN notes_fts ON notes_fts.rowid = n.id
             WHERE notes_fts MATCH ?1
             ORDER BY rank",
        )?;
        let rows: rusqlite::Result<Vec<Note>> = stmt
            .query_map(params![query], parse_note_row)?
            .collect();
        rows
    })
    .await
    .map_err(NotesLibError::Db)
}

pub async fn folders() -> NotesLibResult<Vec<String>> {
    db::execute_async(|conn| {
        let mut stmt =
            conn.prepare("SELECT DISTINCT folder FROM notes WHERE folder != '' ORDER BY folder")?;
        let rows: rusqlite::Result<Vec<String>> =
            stmt.query_map([], |row| row.get(0))?.collect();
        rows
    })
    .await
    .map_err(NotesLibError::Db)
}

pub async fn tags() -> NotesLibResult<Vec<String>> {
    let tags_jsons = db::execute_async(|conn| {
        let mut stmt = conn.prepare("SELECT tags FROM notes WHERE tags != '[]'")?;
        let rows: rusqlite::Result<Vec<String>> =
            stmt.query_map([], |row| row.get(0))?.collect();
        rows
    })
    .await
    .map_err(NotesLibError::Db)?;

    let mut all_tags: HashSet<String> = HashSet::new();
    for json in tags_jsons {
        if let Ok(ts) = serde_json::from_str::<Vec<String>>(&json) {
            all_tags.extend(ts);
        }
    }
    let mut result: Vec<String> = all_tags.into_iter().collect();
    result.sort();
    Ok(result)
}

pub async fn advance_status(uuid: &str) -> NotesLibResult<Note> {
    let note = get(uuid).await?;
    let new_status = match note.status {
        NoteStatus::Raw => NoteStatus::Note,
        NoteStatus::Note => NoteStatus::Article,
        NoteStatus::Article => NoteStatus::Article,
    };
    let req = UpdateNoteRequest {
        title: None,
        content: None,
        status: Some(new_status),
        tags: None,
        folder: None,
    };
    update(uuid, req).await
}

pub async fn print(uuid: &str) -> NotesLibResult {
    let note = get(uuid).await?;
    let width = printer::line_width();
    let sep = "─".repeat(width);

    let title = if note.title.is_empty() {
        "Untitled Note".to_string()
    } else {
        note.title.clone()
    };
    let origin = format!("NOTE [{}]", note.status.as_str());

    let mut lines: Vec<String> = Vec::new();

    let mut meta: Vec<String> = Vec::new();
    if !note.folder.is_empty() {
        meta.push(format!("folder: {}", note.folder));
    }
    if !note.tags.is_empty() {
        meta.push(format!("tags: {}", note.tags.join(", ")));
    }
    if !meta.is_empty() {
        lines.push(meta.join("  •  "));
    }

    lines.push(sep.clone());
    lines.push(String::new());

    for line in note.content.lines() {
        lines.push(line.to_string());
    }

    lines.push(String::new());
    lines.push(sep);
    lines.push(format!(
        "Created: {}  |  Updated: {}",
        note.created_at.format("%d %b %Y"),
        note.updated_at.format("%d %b %Y"),
    ));

    printer::PrintJob::new(origin, title, lines)
        .with_qr(format!("manage-dan://notes/{}", uuid))
        .execute(0, 0)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn status_roundtrip() {
        use crate::models::NoteStatus;
        assert_eq!(NoteStatus::from_str("raw"), NoteStatus::Raw);
        assert_eq!(NoteStatus::from_str("note"), NoteStatus::Note);
        assert_eq!(NoteStatus::from_str("article"), NoteStatus::Article);
        assert_eq!(NoteStatus::Raw.as_str(), "raw");
        assert_eq!(NoteStatus::Note.as_str(), "note");
        assert_eq!(NoteStatus::Article.as_str(), "article");
    }

    #[test]
    fn frontmatter_roundtrip() {
        use crate::{parse_md_file, write_md_file};
        use crate::models::{Note, NoteStatus};
        use chrono::Local;
        use std::sync::OnceLock;

        // Need NOTES_DIR set for note_path()
        static DIR: OnceLock<()> = OnceLock::new();
        DIR.get_or_init(|| {
            let tmp = std::env::temp_dir().join("notes_test");
            std::fs::create_dir_all(&tmp).unwrap();
            super::NOTES_DIR.set(tmp.to_string_lossy().to_string()).ok();
        });

        let now = Local::now();
        let note = Note {
            id: None,
            uuid: "test-uuid-1234".to_string(),
            title: "Test Note: with colon".to_string(),
            content: "Hello **world**".to_string(),
            status: NoteStatus::Note,
            tags: vec!["rust".to_string(), "test".to_string()],
            folder: "inbox".to_string(),
            created_at: now,
            updated_at: now,
        };

        write_md_file(&note).unwrap();
        let path = super::note_path(&note.uuid, &note.folder);
        let parsed = parse_md_file(&path).unwrap();

        assert_eq!(parsed.uuid, note.uuid);
        assert_eq!(parsed.title, note.title);
        assert_eq!(parsed.status, note.status);
        assert_eq!(parsed.tags, note.tags);
        assert_eq!(parsed.folder, note.folder);
        assert_eq!(parsed.content, note.content);
    }
}
