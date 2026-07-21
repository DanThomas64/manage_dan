//! End-to-end exercise of the nb-backed todo backend against a scratch nb
//! notebook, a fully isolated `nb` data directory (via `NB_DIR`), and a
//! scratch working directory — so it never touches the real `app.sqlite`,
//! the real `todo` notebook, or the live systemd service.
//!
//! `NB_DIR` isolation matters here specifically because of `complete_item`'s
//! `todo::log_completion`: completing a todo always logs a daily-log
//! entry to the hardcoded, global `log` notebook regardless of which todo
//! notebook was used — a scratch *notebook name* alone doesn't isolate that
//! side effect, only redirecting `nb`'s entire data directory does. This
//! test used to skip `NB_DIR` (only isolating the todo notebook by name),
//! which for a long time silently wrote a real "Completed: Integration test
//! todo" entry into the real, live daily log on every test run — see
//! `scripts/dev/clean_test_pollution.sh` for the cleanup and detection this
//! caused.

use chrono::{Duration, Local};
use todo::models::{Subtask, TodoItem};

const TEST_NOTEBOOK: &str = "zz_test_todo";

#[tokio::test]
async fn nb_backend_end_to_end() {
    let scratch = std::env::temp_dir().join(format!("todo_nb_test_{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    std::env::set_current_dir(&scratch).expect("cd into scratch dir");

    let nb_dir = std::env::temp_dir().join(format!("todo_nb_test_nbdir_{}", std::process::id()));
    std::fs::create_dir_all(&nb_dir).expect("create scratch NB_DIR");
    std::env::set_var("NB_DIR", &nb_dir);

    // First real `nb` invocation on a fresh NB_DIR triggers a one-time
    // "Welcome" onboarding flow instead of doing what's asked — dismiss it
    // before doing anything real (same as completion_log.rs).
    let _ = std::process::Command::new("nb").arg("notebooks").output();

    db::init().expect("db init");
    printer::init(0, 0, "terminal", 42).expect("printer init");
    notes::init().expect("notes init");
    todo::init(TEST_NOTEBOOK).expect("todo init");

    // "zz_test:" prefix makes this identifiable as test data even outside
    // this test's own NB_DIR isolation (e.g. if a future change reintroduces
    // an un-isolated side effect) — see the tagging convention this
    // establishes, documented in CLAUDE.md and enforced by
    // scripts/dev/clean_test_pollution.sh.
    let mut item = TodoItem::new("zz_test: Integration test todo".to_string(), "desc line one".to_string());
    item.priority = 3;
    item.due_date = Some(Local::now() + Duration::days(2));
    item.labels = vec!["urgent".to_string()];
    item.subtasks = vec![
        Subtask { id: None, title: "sub A".to_string(), done: false },
        Subtask { id: None, title: "sub B".to_string(), done: true },
    ];

    let created = todo::create_item(item).await.expect("create_item");
    assert_eq!(created.title, "zz_test: Integration test todo");
    assert_eq!(created.description, "desc line one");
    assert_eq!(created.priority, 3);
    assert!(!created.completed);
    assert_eq!(created.labels, vec!["urgent".to_string()]);
    assert!(created.due_date.is_some());
    assert_eq!(created.subtasks.len(), 2);
    assert_eq!(created.subtasks[0].title, "sub A");
    assert!(!created.subtasks[0].done);
    assert_eq!(created.subtasks[1].title, "sub B");
    assert!(created.subtasks[1].done);
    assert_eq!(created.project_title, None);
    let id = created.id.expect("created item has an id");

    // The priority header must sit *below* everything nb itself generates
    // (title/due/description/tasks/tags), not prepended above the title.
    // Safe to address by raw local id `1` here: this is the sole item in a
    // freshly-cleaned root-folder scope at this point in the test.
    let raw = std::process::Command::new("nb")
        .args([&format!("{TEST_NOTEBOOK}:show"), "1", "--no-color"])
        .output()
        .expect("shell out to nb show");
    let raw_content = String::from_utf8_lossy(&raw.stdout);
    let raw_lines: Vec<&str> = raw_content.lines().collect();
    let title_line_idx = raw_lines.iter().position(|l| l.contains("Integration test todo"))
        .expect("title line present in raw file");
    let priority_line_idx = raw_lines.iter().position(|l| l.contains("priority:"))
        .expect("priority header present in raw file");
    assert!(
        priority_line_idx > title_line_idx,
        "priority header must be below the title, not above it (raw file: {raw_content})"
    );
    let tags_line_idx = raw_lines.iter().position(|l| l.trim() == "## Tags");
    if let Some(tags_idx) = tags_line_idx {
        assert!(
            priority_line_idx > tags_idx,
            "priority header must be below the Tags section (raw file: {raw_content})"
        );
    }

    let items = todo::read_items().await.expect("read_items");
    assert!(items.iter().any(|i| i.id == Some(id)), "created item missing from read_items");

    todo::complete_item(id, true).await.expect("complete_item(true)");
    let fetched = todo::get_item(id).await.expect("get_item after complete");
    assert!(fetched.completed);

    let mut updated = fetched.clone();
    updated.completed = false;
    updated.title = "zz_test: Updated title".to_string();
    updated.priority = 1;
    updated.project_title = Some("work".to_string());
    todo::update_item(updated).await.expect("update_item");

    let after_update = todo::get_item(id).await.expect("get_item after update");
    assert_eq!(after_update.id, Some(id), "id must stay stable across update");
    assert_eq!(after_update.title, "zz_test: Updated title");
    assert_eq!(after_update.priority, 1);
    assert!(!after_update.completed);
    assert_eq!(after_update.project_title, Some("work".to_string()));
    assert_eq!(after_update.subtasks.len(), 2);

    let summary = todo::get_summary().await.expect("get_summary");
    assert_eq!(summary.total_pending, 1);

    todo::delete_item(id).await.expect("delete_item");
    let items_after_delete = todo::read_items().await.expect("read_items after delete");
    assert!(!items_after_delete.iter().any(|i| i.id == Some(id)));

    std::fs::remove_dir_all(&nb_dir).ok();
    std::env::remove_var("NB_DIR");
}
