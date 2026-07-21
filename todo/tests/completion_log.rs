//! End-to-end exercise of the "completing a todo logs a daily-log entry"
//! feature, against a fully isolated `nb` data directory (via `NB_DIR`) and
//! a scratch working directory — so it never touches the real `app.sqlite`,
//! the real `todo`/`log` notebooks, or the live systemd service.
//!
//! `nb`'s `daily` command is a plugin (`daily.nb-plugin`), not built into
//! the binary — a fresh `NB_DIR` doesn't have it installed, so this test
//! copies it in from the machine's real `nb` setup (`~/.nb/.plugins/`)
//! before exercising anything log-related. That's the one piece of this
//! test that assumes `nb`'s daily-log feature is already set up on the
//! machine running it, same assumption the rest of the suite already makes
//! about `nb` being installed at all.

use todo::models::TodoItem;

const TEST_NOTEBOOK: &str = "zz_test_completion_log";

fn install_daily_plugin(nb_dir: &std::path::Path) {
    let plugins_dir = nb_dir.join(".plugins");
    std::fs::create_dir_all(&plugins_dir).expect("create .plugins dir");
    let home = std::env::var("HOME").expect("HOME must be set");
    let source = std::path::Path::new(&home).join(".nb/.plugins/daily.nb-plugin");
    std::fs::copy(&source, plugins_dir.join("daily.nb-plugin"))
        .expect("copy daily.nb-plugin into scratch NB_DIR — requires nb's daily log feature to already be set up on this machine");
}

#[tokio::test]
async fn completing_a_todo_logs_an_entry() {
    let scratch = std::env::temp_dir().join(format!("todo_completion_log_test_{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    std::env::set_current_dir(&scratch).expect("cd into scratch dir");

    let nb_dir = std::env::temp_dir().join(format!("todo_completion_log_nbdir_{}", std::process::id()));
    std::fs::create_dir_all(&nb_dir).expect("create scratch NB_DIR");
    std::env::set_var("NB_DIR", &nb_dir);

    // First real `nb` invocation on a fresh NB_DIR triggers a one-time
    // "Welcome" onboarding flow instead of doing what's asked — dismiss it
    // before doing anything real.
    let _ = std::process::Command::new("nb").arg("notebooks").output();
    let _ = std::process::Command::new("nb").args(["notebooks", "add", "log"]).output();
    install_daily_plugin(&nb_dir);

    db::init().expect("db init");
    printer::init(0, 0, "terminal", 42).expect("printer init");
    notes::init().expect("notes init");
    todo::init(TEST_NOTEBOOK).expect("todo init");

    // Plain (non-project) todo — its completion log should carry only the
    // `todo-complete` tag.
    let item = TodoItem::new("Buy groceries".to_string(), "milk, eggs, bread".to_string());
    let created = todo::create_item(item).await.expect("create_item");
    let id = created.id.expect("created item has an id");

    todo::complete_item(id, true).await.expect("complete_item(true)");

    let logs = notes::recent_logs(1).await.expect("recent_logs");
    let entry = logs.iter().find(|e| e.title.contains("Buy groceries"))
        .expect("completion log entry should exist after completing the todo");
    assert_eq!(entry.title, "Completed: Buy groceries");
    assert!(entry.tags.iter().any(|t| t == "todo-complete"), "log entry should carry the todo-complete tag");
    assert!(!entry.tags.iter().any(|t| t.starts_with("project-")), "non-project todo shouldn't get a project- tag");

    // Re-completing an already-completed item must not log a second entry.
    todo::complete_item(id, true).await.expect("complete_item(true) again");
    let logs_after_repeat = notes::recent_logs(1).await.expect("recent_logs after repeat");
    let matching: Vec<_> = logs_after_repeat.iter().filter(|e| e.title.contains("Buy groceries")).collect();
    assert_eq!(matching.len(), 1, "completing an already-completed todo must not log a duplicate entry");

    // Un-completing must not log anything either.
    todo::complete_item(id, false).await.expect("complete_item(false)");
    let logs_after_uncomplete = notes::recent_logs(1).await.expect("recent_logs after uncomplete");
    let matching_after_uncomplete: Vec<_> = logs_after_uncomplete.iter().filter(|e| e.title.contains("Buy groceries")).collect();
    assert_eq!(matching_after_uncomplete.len(), 1, "un-completing a todo must not add another log entry");

    // Project-associated todo — its completion log should also carry the
    // matching project-<slug> tag.
    let mut project_item = TodoItem::new("Ship the release".to_string(), String::new());
    project_item.project_title = Some("launch".to_string());
    let created_project_item = todo::create_item(project_item).await.expect("create_item (project)");
    let project_item_id = created_project_item.id.expect("created project item has an id");

    todo::complete_item(project_item_id, true).await.expect("complete_item(true) (project)");
    let logs_final = notes::recent_logs(1).await.expect("recent_logs final");
    let project_entry = logs_final.iter().find(|e| e.title.contains("Ship the release"))
        .expect("completion log entry should exist for the project-associated todo");
    assert!(project_entry.tags.iter().any(|t| t == "project-launch"), "project todo's completion log should carry project-<slug>");

    std::fs::remove_dir_all(&nb_dir).ok();
    std::env::remove_var("NB_DIR");
}
