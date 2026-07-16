use crate::notes_prelude::*;
use crate::models::Note;
use chrono::{DateTime, Duration, Local, NaiveDate};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::process::Command;

async fn run(args: &[&str]) -> NotesLibResult<String> {
    let mut cmd = Command::new("nb");
    cmd.args(args).arg("--no-color");
    let out = cmd.output().await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            NotesLibError::NbNotInstalled
        } else {
            NotesLibError::Io(e)
        }
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let msg = if !stderr.is_empty() { stderr } else { stdout };
        return Err(NotesLibError::Nb(msg));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

// Always prefix explicitly with the notebook name (including "home"). `nb`
// persists whichever notebook a colon-prefixed command last targeted as its
// "current" notebook, so bare/unprefixed commands silently drift onto
// whatever notebook was last touched instead of "home".
fn nb_ref(notebook: &str, nb_id: u64) -> String {
    let notebook = if notebook.is_empty() { "home" } else { notebook };
    format!("{}:{}", notebook, nb_id)
}

fn nb_cmd(notebook: &str, subcmd: &str) -> String {
    let notebook = if notebook.is_empty() { "home" } else { notebook };
    format!("{}:{}", notebook, subcmd)
}

fn system_time_to_local(t: std::io::Result<SystemTime>) -> DateTime<Local> {
    t.ok()
        .and_then(|st| st.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| {
            let secs = d.as_secs() as i64;
            DateTime::from_timestamp(secs, 0)
                .map(|utc| utc.with_timezone(&Local))
                .unwrap_or_else(Local::now)
        })
        .unwrap_or_else(Local::now)
}

// Parses the default note body layout: `# Title`, optional `#tag1 #tag2`
// line, blank line, then content. Shared by whole-file notes and by
// individual entries inside a multi-entry daily log file.
fn parse_body<'a>(lines: impl Iterator<Item = &'a str>) -> (String, Vec<String>, String) {
    let mut lines = lines.peekable();

    // First non-empty line: `# Title`
    let title = loop {
        match lines.next() {
            None => break String::new(),
            Some(l) if l.starts_with("# ") => break l[2..].to_string(),
            Some(l) if l.trim().is_empty() => continue,
            Some(l) => break l.to_string(),
        }
    };

    // Skip blank line after title
    while lines.peek().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.next();
    }

    // Next non-empty block: tags line if ALL whitespace-separated tokens start with `#`
    let mut tags: Vec<String> = Vec::new();
    if let Some(&next) = lines.peek() {
        let tokens: Vec<&str> = next.split_whitespace().collect();
        if !tokens.is_empty() && tokens.iter().all(|t| t.starts_with('#')) {
            tags = tokens.iter().map(|t| t[1..].to_string()).collect();
            lines.next();
        }
    }

    // Skip blank line after tags
    while lines.peek().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.next();
    }

    // Remaining lines: content
    let content: String = lines.collect::<Vec<_>>().join("\n").trim_end().to_string();

    (title, tags, content)
}

fn parse_note_file(path: &Path, nb_id: u64, notebook: &str) -> NotesLibResult<Note> {
    let raw = std::fs::read_to_string(path)?;
    let meta = std::fs::metadata(path)?;

    let created_at = system_time_to_local(meta.created());
    let updated_at = system_time_to_local(meta.modified());

    let (title, tags, content) = parse_body(raw.lines());

    Ok(Note {
        nb_id,
        notebook: notebook.to_string(),
        title,
        content,
        tags,
        created_at,
        updated_at,
    })
}

// Splits a daily log file's raw text into its individual entries. Each entry
// begins with the `## HH:MM:SS` heading nb's `daily` plugin auto-inserts,
// followed by the title/tags/content layout `nb_daily` writes into it.
fn parse_daily_entries(raw: &str, date: &str) -> Vec<crate::models::LogEntry> {
    let mut lines = raw.lines().peekable();
    let mut entries = Vec::new();

    while let Some(line) = lines.next() {
        let Some(time) = line.strip_prefix("## ") else { continue };
        let time = time.trim().to_string();

        let mut body_lines = Vec::new();
        while let Some(&next) = lines.peek() {
            if next.starts_with("## ") {
                break;
            }
            body_lines.push(lines.next().unwrap());
        }

        let (title, tags, content) = parse_body(body_lines.into_iter());
        entries.push(crate::models::LogEntry {
            date: date.to_string(),
            time,
            title,
            tags,
            content,
        });
    }

    entries
}

// `# {title}` heading, optional `#tag1 #tag2` line, blank line, then content —
// the default layout nb uses for a note body.
fn format_note_body(title: &str, tags: &[String], content: &str) -> String {
    let mut out = format!("# {}\n", title);
    if !tags.is_empty() {
        let tag_line: String = tags.iter().map(|t| format!("#{}", t)).collect::<Vec<_>>().join(" ");
        out.push('\n');
        out.push_str(&tag_line);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(content.trim());
    out
}

fn write_note_file(path: &Path, title: &str, tags: &[String], content: &str) -> NotesLibResult<()> {
    let mut out = format_note_body(title, tags, content);
    out.push('\n');
    std::fs::write(path, out)?;
    Ok(())
}

// Parse `[n] Title` or `[notebook:n] Title` list lines.
// Returns (nb_id, notebook, title).
fn parse_list_line(line: &str, ctx_notebook: &str) -> Option<(u64, String, String)> {
    let line = line.trim();
    let rest = line.strip_prefix('[')?;
    let (ref_part, title) = rest.split_once("] ")?;
    let (notebook, id_str) = if let Some((nb, id)) = ref_part.split_once(':') {
        (nb.to_string(), id.to_string())
    } else {
        (ctx_notebook.to_string(), ref_part.to_string())
    };
    let nb_id: u64 = id_str.trim().parse().ok()?;
    Some((nb_id, notebook, title.to_string()))
}

async fn nb_path(notebook: &str, nb_id: u64) -> NotesLibResult<PathBuf> {
    let ref_str = nb_ref(notebook, nb_id);
    let out = run(&["show", &ref_str, "--path"]).await?;
    let path = out.trim().to_string();
    if path.is_empty() {
        return Err(NotesLibError::NotFound(ref_str));
    }
    Ok(PathBuf::from(path))
}

pub async fn nb_add(notebook: &str, title: &str, content: &str, tags: &[String]) -> NotesLibResult<u64> {
    let cmd = nb_cmd(notebook, "add");
    let mut args = vec![cmd.as_str(), "--content", content];

    if !title.is_empty() {
        args.extend_from_slice(&["--title", title]);
    }

    let tags_str;
    if !tags.is_empty() {
        tags_str = tags.join(",");
        args.extend_from_slice(&["--tags", &tags_str]);
    }

    let out = run(&args).await?;

    // Output: `Added: [n] filename "Title"` or `Added: [notebook:n] filename "Title"`
    let bracket_content = out
        .lines()
        .find(|l| l.contains("Added:"))
        .and_then(|l| l.find('[').map(|s| &l[s + 1..]))
        .and_then(|s| s.find(']').map(|e| &s[..e]))
        .ok_or_else(|| NotesLibError::Nb(format!("unexpected nb add output: {}", out.trim())))?;

    let id_str = bracket_content.split(':').last().unwrap_or(bracket_content);
    id_str.trim().parse::<u64>().map_err(|_| {
        NotesLibError::Nb(format!("cannot parse id from nb add output: {}", out.trim()))
    })
}

// Appends a titled, tagged entry to today's daily log via nb's `daily`
// plugin. Each entry lands under its own auto-generated `## HH:MM:SS`
// heading in the day's file, followed by the same title/tags/content layout
// a regular note uses.
pub async fn nb_daily(notebook: &str, title: &str, tags: &[String], content: &str) -> NotesLibResult<()> {
    let cmd = nb_cmd(notebook, "daily");
    let entry = format_note_body(title, tags, content);
    run(&[&cmd, &entry]).await?;
    Ok(())
}

pub async fn nb_show(notebook: &str, nb_id: u64) -> NotesLibResult<Note> {
    let path = nb_path(notebook, nb_id).await?;
    parse_note_file(&path, nb_id, notebook)
        .map_err(|_| NotesLibError::NotFound(nb_ref(notebook, nb_id)))
}

pub async fn nb_list(notebook: Option<&str>) -> NotesLibResult<Vec<Note>> {
    let notebooks: Vec<String> = if let Some(nb) = notebook {
        vec![nb.to_string()]
    } else {
        nb_notebooks().await?
    };

    let mut notes = Vec::new();
    for nb in &notebooks {
        let cmd = nb_cmd(nb, "list");
        let out = match run(&[&cmd]).await {
            Ok(o) => o,
            Err(NotesLibError::Nb(_)) => continue, // empty notebook returns error
            Err(e) => return Err(e),
        };

        for line in out.lines() {
            if let Some((id, nb_name, _title)) = parse_list_line(line, nb) {
                match nb_show(&nb_name, id).await {
                    Ok(note) => notes.push(note),
                    Err(NotesLibError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }
            }
        }
    }
    Ok(notes)
}

// Reads every daily log file in `notebook` dated within the last `days` days
// (inclusive of today) and returns their individual entries, most recent
// first.
pub async fn nb_daily_entries(notebook: &str, days: i64) -> NotesLibResult<Vec<crate::models::LogEntry>> {
    let cmd = nb_cmd(notebook, "list");
    let out = match run(&[&cmd]).await {
        Ok(o) => o,
        Err(NotesLibError::Nb(_)) => return Ok(Vec::new()), // empty notebook
        Err(e) => return Err(e),
    };

    let cutoff = Local::now().date_naive() - Duration::days(days.max(1) - 1);

    let mut entries = Vec::new();
    for line in out.lines() {
        let Some((id, nb_name, _title)) = parse_list_line(line, notebook) else { continue };
        let path = match nb_path(&nb_name, id).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let Ok(date) = NaiveDate::parse_from_str(stem, "%Y%m%d") else { continue };
        if date < cutoff {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else { continue };
        entries.extend(parse_daily_entries(&raw, &date.format("%Y-%m-%d").to_string()));
    }

    entries.sort_by(|a: &crate::models::LogEntry, b| (&b.date, &b.time).cmp(&(&a.date, &a.time)));
    Ok(entries)
}

pub async fn nb_update(
    notebook: &str,
    nb_id: u64,
    title: Option<&str>,
    content: Option<&str>,
    tags: Option<&[String]>,
) -> NotesLibResult<Note> {
    let path = nb_path(notebook, nb_id).await?;
    let current = parse_note_file(&path, nb_id, notebook)
        .map_err(|_| NotesLibError::NotFound(nb_ref(notebook, nb_id)))?;

    let new_title = title.unwrap_or(&current.title);
    let new_content = content.unwrap_or(&current.content);
    let new_tags: &[String] = tags.unwrap_or(&current.tags);

    write_note_file(&path, new_title, new_tags, new_content)?;
    parse_note_file(&path, nb_id, notebook)
        .map_err(|_| NotesLibError::NotFound(nb_ref(notebook, nb_id)))
}

pub async fn nb_delete(notebook: &str, nb_id: u64) -> NotesLibResult<()> {
    let ref_str = nb_ref(notebook, nb_id);
    run(&["delete", &ref_str, "--force"]).await?;
    Ok(())
}

pub async fn nb_search(query: &str) -> NotesLibResult<Vec<Note>> {
    // Search across all notebooks
    let out = match run(&["search", query, "--all"]).await {
        Ok(o) => o,
        Err(NotesLibError::Nb(_)) => return Ok(Vec::new()), // no results
        Err(e) => return Err(e),
    };

    let mut notes = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for line in out.lines() {
        if let Some((id, nb_name, _)) = parse_list_line(line, "home") {
            let key = (nb_name.clone(), id);
            if seen.insert(key) {
                match nb_show(&nb_name, id).await {
                    Ok(note) => notes.push(note),
                    Err(NotesLibError::NotFound(_)) => {}
                    Err(e) => return Err(e),
                }
            }
        }
    }
    Ok(notes)
}

pub async fn nb_notebooks() -> NotesLibResult<Vec<String>> {
    let out = run(&["notebooks"]).await?;
    Ok(out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

pub async fn nb_tags() -> NotesLibResult<Vec<String>> {
    let notebooks = nb_notebooks().await?;
    let mut all_tags = std::collections::HashSet::new();

    for nb in &notebooks {
        let cmd = nb_cmd(nb, "list");
        let out = match run(&[&cmd]).await {
            Ok(o) => o,
            Err(NotesLibError::Nb(_)) => continue,
            Err(e) => return Err(e),
        };

        for line in out.lines() {
            if let Some((id, nb_name, _)) = parse_list_line(line, nb) {
                if let Ok(note) = nb_show(&nb_name, id).await {
                    all_tags.extend(note.tags);
                }
            }
        }
    }

    let mut result: Vec<String> = all_tags.into_iter().collect();
    result.sort();
    Ok(result)
}
