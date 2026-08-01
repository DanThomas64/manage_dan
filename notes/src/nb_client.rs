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
//
// `folder` is required, not just cosmetic: `nb` numbers items per listing
// scope, not per notebook — a note directly in a notebook's root and a note
// inside one of its subfolders can both be "id 1" at the same time (confirmed
// against a real `nb` install). A bare `notebook:id` only ever addresses a
// root-level item; an item inside a folder must be addressed as
// `notebook:folder/id`.
fn nb_ref(notebook: &str, folder: &str, nb_id: u64) -> String {
    let notebook = if notebook.is_empty() { "home" } else { notebook };
    if folder.is_empty() {
        format!("{}:{}", notebook, nb_id)
    } else {
        format!("{}:{}/{}", notebook, folder, nb_id)
    }
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

// nb marks a bookmark file with a `.bookmark.md` extension (regular notes
// are just `.md`) — confirmed against a real `nb` install. That's the only
// structural signal available (there's no separate bookmark table/index),
// so it's what routes a file to `parse_bookmark_body` instead of the plain
// `parse_body` every other note uses.
fn is_bookmark_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.ends_with(".bookmark.md"))
        .unwrap_or(false)
}

// Appends a named section's collected body (if non-empty) to `content_lines`
// as a `## Heading` block, mirroring nb's own bookmark body layout — used so
// any section nb writes that this parser doesn't special-case (Quote,
// Comment, Related, ...) still ends up visible in the note's `content`
// rather than being silently dropped.
fn flush_bookmark_section(name: &str, body: &[String], content_lines: &mut Vec<String>) {
    let text = body.join("\n").trim().to_string();
    if text.is_empty() {
        return;
    }
    if !content_lines.is_empty() {
        content_lines.push(String::new());
    }
    content_lines.push(format!("## {}", name));
    content_lines.push(String::new());
    content_lines.push(text);
}

// Parses nb's own bookmark body layout:
//   # Title
//
//   <url>
//
//   ## Quote / ## Comment / ## Tags / ...
//
//   <section body>
//
// `## Tags`'s body is `#tag1 #tag2 ...` — extracted into structured `tags`
// rather than left in `content` like every other section, so a bookmark's
// tags flow into `note_cache_tags` (project scoping, tag filtering) exactly
// like a regular note's. Every other section is kept, heading included, in
// `content` — new section kinds nb might add later still show up instead of
// vanishing.
fn parse_bookmark_body(raw: &str) -> (String, Option<String>, Vec<String>, String) {
    let mut lines = raw.lines().peekable();

    let title = loop {
        match lines.next() {
            None => break String::new(),
            Some(l) if l.trim().is_empty() => continue,
            Some(l) => break l.strip_prefix("# ").unwrap_or(l).trim().to_string(),
        }
    };

    while lines.peek().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.next();
    }

    let url = lines
        .peek()
        .and_then(|l| l.trim().strip_prefix('<'))
        .and_then(|l| l.strip_suffix('>'))
        .map(|s| s.to_string());
    if url.is_some() {
        lines.next();
    }

    let mut tags: Vec<String> = Vec::new();
    let mut content_lines: Vec<String> = Vec::new();
    let mut current_section: Option<String> = None;
    let mut section_body: Vec<String> = Vec::new();

    for line in lines {
        if let Some(name) = line.trim().strip_prefix("## ") {
            if let Some(prev) = &current_section {
                if prev.eq_ignore_ascii_case("tags") {
                    tags = section_body
                        .join(" ")
                        .split_whitespace()
                        .filter_map(|t| t.strip_prefix('#').map(|s| s.to_string()))
                        .collect();
                } else {
                    flush_bookmark_section(prev, &section_body, &mut content_lines);
                }
            }
            current_section = Some(name.trim().to_string());
            section_body.clear();
            continue;
        }
        section_body.push(line.to_string());
    }
    if let Some(name) = &current_section {
        if name.eq_ignore_ascii_case("tags") {
            tags = section_body
                .join(" ")
                .split_whitespace()
                .filter_map(|t| t.strip_prefix('#').map(|s| s.to_string()))
                .collect();
        } else {
            flush_bookmark_section(name, &section_body, &mut content_lines);
        }
    }

    (title, url, tags, content_lines.join("\n").trim_end().to_string())
}

pub(crate) fn parse_note_file(path: &Path, nb_id: u64, notebook: &str, folder: &str) -> NotesLibResult<Note> {
    let raw = std::fs::read_to_string(path)?;
    let meta = std::fs::metadata(path)?;

    let created_at = system_time_to_local(meta.created());
    let updated_at = system_time_to_local(meta.modified());

    let (title, url, tags, content) = if is_bookmark_path(path) {
        parse_bookmark_body(&raw)
    } else {
        let (title, tags, content) = parse_body(raw.lines());
        (title, None, tags, content)
    };

    Ok(Note {
        nb_id,
        notebook: notebook.to_string(),
        folder: folder.to_string(),
        title,
        content,
        tags,
        url,
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

// Parse `[n] Title`, `[notebook:n] Title`, or `[notebook:folder/n] Title`
// list lines (e.g. `nb search` results). Returns (nb_id, notebook, folder, title).
fn parse_list_line(line: &str, ctx_notebook: &str) -> Option<(u64, String, String, String)> {
    let line = line.trim();
    let rest = line.strip_prefix('[')?;
    let (ref_part, title) = rest.split_once("] ")?;
    let (notebook, id_str) = if let Some((nb, id)) = ref_part.split_once(':') {
        (nb.to_string(), id.to_string())
    } else {
        (ctx_notebook.to_string(), ref_part.to_string())
    };
    let (folder, nb_id) = match id_str.rsplit_once('/') {
        Some((folder, id)) => (folder.to_string(), id.trim().parse().ok()?),
        None => (String::new(), id_str.trim().parse().ok()?),
    };
    Some((nb_id, notebook, folder, title.to_string()))
}

// Parses a `--paths` listing line: `[n] path` or `[notebook:n] path`,
// possibly with an icon in between (e.g. a folder's 📂 marker) — mirrors
// `todo::backends::nb::list_paths_in_folder`'s parsing of the same `nb`
// output shape. Returns (nb_id, notebook, path). One `<notebook>:list
// --paths` call yields every item's resolved file path directly, so callers
// can read+parse the file locally instead of a separate `nb show --path`
// subprocess call per item.
fn parse_list_path_line(line: &str, ctx_notebook: &str) -> Option<(u64, String, PathBuf)> {
    let line = line.trim();
    let rest = line.strip_prefix('[')?;
    let (ref_part, remainder) = rest.split_once(']')?;
    let (notebook, id_str) = if let Some((nb, id)) = ref_part.split_once(':') {
        (nb.to_string(), id.to_string())
    } else {
        (ctx_notebook.to_string(), ref_part.to_string())
    };
    // Inside a folder listing, `id_str` is itself folder-scoped, e.g.
    // `ProjA/3` (confirmed against a real `nb` install — ids are numbered
    // per listing scope, not per notebook). The caller already knows which
    // folder it asked to list, so only the trailing numeric segment is taken
    // as the id here; the folder path itself is threaded separately by
    // `nb_list_paths`'s recursion rather than re-derived from this string.
    let nb_id: u64 = id_str.rsplit('/').next().unwrap_or(&id_str).trim().parse().ok()?;
    // In `--paths` mode a line is exactly `[id] <path>` with nothing else
    // after it, so the whole trimmed remainder *is* the path — it must not
    // be split on whitespace, since a file moved with a literal-space
    // filename (`nb_move`/`nb_restore_folder`'s destination naming reuses a
    // note's title verbatim rather than sanitizing it like `nb add` does)
    // would otherwise have everything before the last space silently
    // dropped.
    let path = remainder.trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some((nb_id, notebook, PathBuf::from(path)))
}

async fn nb_path(notebook: &str, folder: &str, nb_id: u64) -> NotesLibResult<PathBuf> {
    let ref_str = nb_ref(notebook, folder, nb_id);
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

    let id_str = bracket_content.split(':').next_back().unwrap_or(bracket_content);
    id_str.trim().parse::<u64>().map_err(|_| {
        NotesLibError::Nb(format!("cannot parse id from nb add output: {}", out.trim()))
    })
}

/// Creates an nb bookmark (`nb bookmark <notebook>:<folder>/ <url> ...`),
/// stored as the `.bookmark.md` file `parse_note_file`/`is_bookmark_path`
/// detects. `--no-request` is always passed — this runs on a headless
/// server with no reason to fetch and store the target page's own HTML
/// (slow, and an arbitrary-URL server-side fetch this app has no need to
/// make); `title` is used as given rather than scraped from a live fetch.
/// `folder` must already exist — confirmed against a real `nb` install that
/// a nested destination folder can't be reliably created inline in the same
/// call (it prints "Creating new folder:" but then fails); callers create it
/// first via `nb_add_folder`, same as `notes::create`'s folder-at-create-time
/// path already does.
pub async fn nb_bookmark(
    notebook: &str,
    folder: &str,
    url: &str,
    title: Option<&str>,
    tags: &[String],
    comment: Option<&str>,
) -> NotesLibResult<u64> {
    let target = if folder.is_empty() { format!("{}:", notebook) } else { format!("{}:{}/", notebook, folder) };

    let mut args: Vec<String> = vec!["bookmark".to_string(), target, url.to_string(), "--no-request".to_string()];
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        args.push("--title".to_string());
        args.push(t.to_string());
    }
    if !tags.is_empty() {
        args.push("--tags".to_string());
        args.push(tags.join(","));
    }
    if let Some(c) = comment.map(str::trim).filter(|c| !c.is_empty()) {
        args.push("--comment".to_string());
        args.push(c.to_string());
    }

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run(&arg_refs).await?;

    // Output: `Added: [n] 🔖 filename "Title"`, `Added: [notebook:n] 🔖 ...`, or,
    // once a folder is involved, `Added: [notebook:folder/n] 🔖 ...` — the
    // same folder-scoped-id bracket shape `nb_move`'s "Moved to:" output uses
    // (ids are per-folder, not global — see `nb_ref`), so this reuses
    // `parse_bracket_folder_id` rather than `nb_add`'s plain `split(':')`,
    // which would wrongly try to parse `"Bookmarks/1"` as a bare integer.
    let bracket_content = out
        .lines()
        .find(|l| l.contains("Added:"))
        .and_then(|l| l.find('[').map(|s| &l[s + 1..]))
        .and_then(|s| s.find(']').map(|e| &s[..e]))
        .ok_or_else(|| NotesLibError::Nb(format!("unexpected nb bookmark output: {}", out.trim())))?;

    parse_bracket_folder_id(bracket_content)
        .map(|(_folder, id)| id)
        .ok_or_else(|| NotesLibError::Nb(format!("cannot parse id from nb bookmark output: {}", out.trim())))
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

pub async fn nb_show(notebook: &str, folder: &str, nb_id: u64) -> NotesLibResult<Note> {
    let path = nb_path(notebook, folder, nb_id).await?;
    parse_note_file(&path, nb_id, notebook, folder)
        .map_err(|_| NotesLibError::NotFound(nb_ref(notebook, folder, nb_id)))
}

/// Lists `(nb_id, folder, path)` for every note inside `notebook`, recursing
/// into subfolders, plus every folder path visited (including empty ones,
/// captured before recursing into them) — the shared enumeration step behind
/// `nb_list`/`nb_tags`, the background sync pass, and `notes::list_folders`
/// (tree browsing), which additionally needs each item's path to stat its
/// mtime before deciding whether to re-parse it.
///
/// `nb list <folder>/ --paths` only ever returns *that* folder's direct
/// children — folders show up as their own `📂`-marked entries rather than
/// having their contents inlined — so building a full tree costs one `nb`
/// call per folder (root included), not one call for the whole notebook.
pub(crate) async fn nb_list_paths(notebook: &str) -> NotesLibResult<(Vec<(u64, String, PathBuf)>, Vec<String>)> {
    let mut notes = Vec::new();
    let mut folders = Vec::new();
    let mut queue = vec![String::new()];

    while let Some(folder) = queue.pop() {
        let cmd = nb_cmd(notebook, "list");
        let out = if folder.is_empty() {
            run(&[&cmd, "--paths"]).await
        } else {
            let target = format!("{}/", folder);
            run(&[&cmd, &target, "--paths"]).await
        };
        let out = match out {
            Ok(o) => o,
            Err(NotesLibError::Nb(_)) => continue, // empty/missing folder
            Err(e) => return Err(e),
        };

        for line in out.lines() {
            let Some((id, _nb, path)) = parse_list_path_line(line, notebook) else { continue };
            if line.contains('\u{1F4C2}') {
                // 📂 folder entry — `path` is its resolved absolute path;
                // its own name (last component) appended to the current
                // folder prefix gives its path relative to the notebook
                // root, with no need to know the notebook's base directory.
                let Some(name) = path.file_name().and_then(|s| s.to_str()) else { continue };
                let sub = if folder.is_empty() { name.to_string() } else { format!("{}/{}", folder, name) };
                folders.push(sub.clone());
                queue.push(sub);
                continue;
            }
            notes.push((id, folder.clone(), path));
        }
    }

    folders.sort();
    Ok((notes, folders))
}

/// Lists notes, optionally scoped to one notebook. When `notebook` is
/// `None`, every notebook is enumerated except those named in `exclude` —
/// applied here, before any note is read, rather than filtering the
/// hydrated results afterward (the caller doesn't pay to read+parse notes
/// it's only going to discard). Uses `--paths` to resolve every note's file
/// path in the same call that lists it, so no separate per-note `nb show
/// --path` subprocess is needed — each note is parsed from its local file
/// directly.
pub async fn nb_list(notebook: Option<&str>, exclude: &[&str]) -> NotesLibResult<Vec<Note>> {
    let notebooks: Vec<String> = if let Some(nb) = notebook {
        vec![nb.to_string()]
    } else {
        nb_notebooks()
            .await?
            .into_iter()
            .filter(|n| !exclude.contains(&n.as_str()))
            .collect()
    };

    let mut notes = Vec::new();
    for nb in &notebooks {
        let (paths, _folders) = nb_list_paths(nb).await?;
        for (id, folder, path) in paths {
            if let Ok(note) = parse_note_file(&path, id, nb, &folder) {
                notes.push(note);
            }
        }
    }
    Ok(notes)
}

/// Moves every note directly inside `folder` within `notebook` to
/// `dest_notebook`'s root (keyed by title, one at a time — the same
/// rename-target style `nb_move`/`archive_note` already use). Returns the
/// number moved. Used to restore a project's archived notes: they're
/// addressed as `folder/id` while nested (confirmed against a real `nb`
/// install — a bare id fails with "Not found" once a note has been moved
/// into a subfolder), so this parses and moves in one pass rather than
/// going through `nb_show`/`nb_move`'s bare-id-only addressing.
pub async fn nb_restore_folder(notebook: &str, folder: &str, dest_notebook: &str) -> NotesLibResult<usize> {
    let cmd = nb_cmd(notebook, "list");
    let target = format!("{}/", folder);
    let out = match run(&[&cmd, &target]).await {
        Ok(o) => o,
        Err(NotesLibError::Nb(_)) => return Ok(0), // empty/missing folder
        Err(e) => return Err(e),
    };

    let mut moved = 0;
    for line in out.lines() {
        if line.contains('\u{1F4C2}') {
            continue; // 📂 nested subfolder entry, not a note
        }
        let Some(rest) = line.trim().strip_prefix('[') else { continue };
        let Some((ref_part, title)) = rest.split_once("] ") else { continue };
        let path_part = ref_part.rsplit_once(':').map(|(_, p)| p).unwrap_or(ref_part);
        // `<notebook>:move` as the subcommand itself, rather than bare
        // `move` plus a separate `notebook:path` source selector — the
        // installed `nb` CLI's own top-level argument dispatcher mishandles
        // the latter shape for `move`/`rename` (hits an internal "Not
        // found" error before ever reaching the actual move), confirmed via
        // a minimal repro; the notebook-prefixed-subcommand shape (already
        // used by `todo::backends::nb::archive_project_todos`) sidesteps it
        // entirely.
        let cmd = nb_cmd(notebook, "move");
        let dest = format!("{}:{}", dest_notebook, title.trim());
        run(&[&cmd, path_part, &dest, "--force"]).await?;
        moved += 1;
    }
    Ok(moved)
}

// Reads every daily log file in `notebook` dated within the last `days` days
// (inclusive of today) and returns their individual entries, most recent
// first. When `tag` is set, only entries carrying that tag are returned.
pub async fn nb_daily_entries(notebook: &str, days: i64, tag: Option<&str>) -> NotesLibResult<Vec<crate::models::LogEntry>> {
    let cmd = nb_cmd(notebook, "list");
    // `--paths` resolves every daily-log file's path in this one call, so the
    // per-file `nb_path` subprocess spawn below is no longer needed — and
    // the window cutoff (derived from each file's own name) is applied
    // before reading any file, so cost no longer grows with how many daily
    // logs have ever been written, only with how many fall in the window.
    let out = match run(&[&cmd, "--paths"]).await {
        Ok(o) => o,
        Err(NotesLibError::Nb(_)) => return Ok(Vec::new()), // empty notebook
        Err(e) => return Err(e),
    };

    let cutoff = Local::now().date_naive() - Duration::days(days.max(1) - 1);

    let mut entries = Vec::new();
    for line in out.lines() {
        let Some((_id, _nb_name, path)) = parse_list_path_line(line, notebook) else { continue };
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let Ok(date) = NaiveDate::parse_from_str(stem, "%Y%m%d") else { continue };
        if date < cutoff {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else { continue };
        entries.extend(parse_daily_entries(&raw, &date.format("%Y-%m-%d").to_string()));
    }

    entries.sort_by(|a: &crate::models::LogEntry, b| (&b.date, &b.time).cmp(&(&a.date, &a.time)));
    if let Some(tag) = tag {
        entries.retain(|e| e.tags.iter().any(|t| t == tag));
    }
    Ok(entries)
}

pub async fn nb_update(
    notebook: &str,
    folder: &str,
    nb_id: u64,
    title: Option<&str>,
    content: Option<&str>,
    tags: Option<&[String]>,
) -> NotesLibResult<Note> {
    let path = nb_path(notebook, folder, nb_id).await?;
    let current = parse_note_file(&path, nb_id, notebook, folder)
        .map_err(|_| NotesLibError::NotFound(nb_ref(notebook, folder, nb_id)))?;

    let new_title = title.unwrap_or(&current.title);
    let new_content = content.unwrap_or(&current.content);
    let new_tags: &[String] = tags.unwrap_or(&current.tags);

    write_note_file(&path, new_title, new_tags, new_content)?;
    parse_note_file(&path, nb_id, notebook, folder)
        .map_err(|_| NotesLibError::NotFound(nb_ref(notebook, folder, nb_id)))
}

pub async fn nb_delete(notebook: &str, folder: &str, nb_id: u64) -> NotesLibResult<()> {
    let ref_str = nb_ref(notebook, folder, nb_id);
    run(&["delete", &ref_str, "--force"]).await?;
    Ok(())
}

/// Ensures a notebook exists — `nb move` requires its destination notebook
/// to already exist (confirmed against a real `nb` install: moving into a
/// nonexistent notebook fails with "Target notebook not found"), unlike
/// `nb add`/`nb daily`, which create the notebook implicitly. Best-effort:
/// ignores the error when it already exists.
pub async fn nb_ensure_notebook(name: &str) -> NotesLibResult<()> {
    let _ = run(&["notebooks", "add", name]).await;
    Ok(())
}

// Splits bracket content from an `nb` "Added:"/"Moved to:" line — e.g.
// `notebook:ProjA/3` or `notebook:3` — into (folder, id), the same
// folder-scoped-id shape `parse_list_path_line` handles for listings.
fn parse_bracket_folder_id(bracket_content: &str) -> Option<(String, u64)> {
    let after_notebook = bracket_content.split(':').next_back().unwrap_or(bracket_content);
    match after_notebook.rsplit_once('/') {
        Some((folder, id)) => Some((folder.to_string(), id.trim().parse().ok()?)),
        None => Some((String::new(), after_notebook.trim().parse().ok()?)),
    }
}

/// Moves a note into `dest` (a `notebook:path` destination, e.g.
/// `archive:test-project/note-title` or `notebook:folder/` to relocate
/// within the same notebook, keeping the source file's own name) — used by
/// project archiving and by `notes::move_note`. `<notebook>:move` as the
/// subcommand itself, not bare `move` plus a separate `notebook:id` source
/// selector — see `nb_restore_folder`'s doc comment on the same convention
/// for why. Returns the moved note's new `(folder, nb_id)`, parsed from
/// `nb`'s own "Moved to: [notebook:folder/id]" output — moving a note
/// re-numbers it in the destination scope (ids are per-folder, not global,
/// see `nb_ref`), so the caller can't assume the id is unchanged.
pub async fn nb_move(src_notebook: &str, src_folder: &str, nb_id: u64, dest: &str) -> NotesLibResult<(String, u64)> {
    let cmd = nb_cmd(src_notebook, "move");
    let src_ref = if src_folder.is_empty() { nb_id.to_string() } else { format!("{}/{}", src_folder, nb_id) };
    let out = run(&[&cmd, &src_ref, dest, "--force"]).await?;

    let bracket_content = out
        .lines()
        .find(|l| l.contains("Moved to:"))
        .and_then(|l| l.find('[').map(|s| &l[s + 1..]))
        .and_then(|s| s.find(']').map(|e| &s[..e]))
        .ok_or_else(|| NotesLibError::Nb(format!("unexpected nb move output: {}", out.trim())))?;

    parse_bracket_folder_id(bracket_content)
        .ok_or_else(|| NotesLibError::Nb(format!("cannot parse id from nb move output: {}", out.trim())))
}

/// Creates an empty folder inside `notebook` at `path` (which may itself be
/// nested, e.g. `"Projects/Sub"` — `nb` creates intermediate folders as
/// needed). Checks whether `path` already exists first (`nb <notebook>:list
/// <path>/ --paths`, which errors "Not found" for a missing folder and
/// succeeds, even with "0 items", for an existing one) and only creates it
/// when that check fails — confirmed against a real `nb` install that `nb
/// add folder` on an already-existing path does *not* error or no-op like
/// callers here (e.g. `notes::create_bookmark`, called on every bookmark
/// creation regardless of whether `BOOKMARKS_FOLDER` already exists) used to
/// assume; it silently creates a second, numbered duplicate (`Bookmarks-1`,
/// `Bookmarks-2`, ...) instead. Best-effort on the actual creation call
/// itself: still ignores that error, for any other reason it might fail.
pub async fn nb_add_folder(notebook: &str, path: &str) -> NotesLibResult<()> {
    let list_cmd = nb_cmd(notebook, "list");
    let target = format!("{}/", path);
    if run(&[&list_cmd, &target, "--paths"]).await.is_ok() {
        return Ok(());
    }
    let add_cmd = nb_cmd(notebook, "add");
    let _ = run(&[&add_cmd, "folder", path]).await;
    Ok(())
}

/// Deletes `folder` and everything in it, recursively, from `notebook` — one
/// call (confirmed against a real `nb` install: `<notebook>:delete <folder>/`
/// removes the whole subtree, not just direct children). Used when
/// permanently deleting an archived project's remnants from the shared
/// `archive` notebook.
pub async fn nb_delete_folder(notebook: &str, folder: &str) -> NotesLibResult<()> {
    let cmd = nb_cmd(notebook, "delete");
    let target = format!("{}/", folder);
    run(&[&cmd, &target, "--force"]).await?;
    Ok(())
}

/// Permanently deletes an entire notebook — used when permanently deleting a
/// project's own dedicated notebook.
pub async fn nb_delete_notebook(name: &str) -> NotesLibResult<()> {
    run(&["notebooks", "delete", name, "--force"]).await?;
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
        if let Some((id, nb_name, folder, _)) = parse_list_line(line, "home") {
            let key = (nb_name.clone(), folder.clone(), id);
            if seen.insert(key) {
                match nb_show(&nb_name, &folder, id).await {
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

pub async fn nb_tags(exclude: &[&str]) -> NotesLibResult<Vec<String>> {
    let notebooks: Vec<String> = nb_notebooks()
        .await?
        .into_iter()
        .filter(|n| !exclude.contains(&n.as_str()))
        .collect();
    let mut all_tags = std::collections::HashSet::new();

    for nb in &notebooks {
        let (paths, _folders) = nb_list_paths(nb).await?;
        for (id, folder, path) in paths {
            if let Ok(note) = parse_note_file(&path, id, nb, &folder) {
                all_tags.extend(note.tags);
            }
        }
    }

    let mut result: Vec<String> = all_tags.into_iter().collect();
    result.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real output captured from a live `nb bookmark` install (`nb 7.25.4`) —
    // title/url only, no optional sections.
    const MINIMAL: &str = "# Example Domain (example.com)\n\n<https://example.com>\n";

    // Real output with every optional section present.
    const FULL: &str = "# Tools (www.rust-lang.org)\n\n<https://www.rust-lang.org/tools>\n\n## Quote\n\n> some excerpt text\n\n## Comment\n\nnice tools\n\n## Tags\n\n#rust #lang\n";

    #[test]
    fn parses_minimal_bookmark() {
        let (title, url, tags, content) = parse_bookmark_body(MINIMAL);
        assert_eq!(title, "Example Domain (example.com)");
        assert_eq!(url.as_deref(), Some("https://example.com"));
        assert!(tags.is_empty());
        assert!(content.is_empty());
    }

    #[test]
    fn parses_full_bookmark() {
        let (title, url, tags, content) = parse_bookmark_body(FULL);
        assert_eq!(title, "Tools (www.rust-lang.org)");
        assert_eq!(url.as_deref(), Some("https://www.rust-lang.org/tools"));
        assert_eq!(tags, vec!["rust".to_string(), "lang".to_string()]);
        assert!(content.contains("## Quote"));
        assert!(content.contains("> some excerpt text"));
        assert!(content.contains("## Comment"));
        assert!(content.contains("nice tools"));
        assert!(!content.contains("## Tags"));
    }

    #[test]
    fn is_bookmark_path_checks_extension() {
        assert!(is_bookmark_path(Path::new("/x/20260730.bookmark.md")));
        assert!(!is_bookmark_path(Path::new("/x/20260730.md")));
    }
}
