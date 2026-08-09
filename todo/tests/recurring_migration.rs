//! Exercises the one-time TOML-to-DB migration for recurring tasks and
//! reminders (`todo::recurring::migrate_from_toml_if_empty` /
//! `todo::reminders::migrate_from_toml_if_empty`) — see CLAUDE.md's
//! "recurring tasks & reminders" note. Runs against a scratch cwd (so it
//! gets its own `app.sqlite`) and a scratch `APP_CONFIG_DIR`, so it never
//! touches the real deployed DB or `config/*.toml` files.

#[tokio::test]
async fn migrates_toml_into_db_exactly_once() {
    let scratch = std::env::temp_dir().join(format!("todo_recurring_migration_test_{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    std::env::set_current_dir(&scratch).expect("cd into scratch dir");

    let config_dir = scratch.join("config");
    std::fs::create_dir_all(&config_dir).expect("create scratch config dir");
    std::env::set_var("APP_CONFIG_DIR", &config_dir);

    std::fs::write(
        config_dir.join("recurring.toml"),
        "[[tasks]]\ntitle = \"zz_test: Laundry\"\nschedule = \"weekly:sat\"\n",
    )
    .expect("write recurring.toml");

    std::fs::write(
        config_dir.join("reminders.toml"),
        "[[reminders]]\ntitle = \"zz_test: Check blood pressure\"\nschedule = \"daily\"\n",
    )
    .expect("write reminders.toml");

    db::init().expect("db init");

    todo::recurring::migrate_from_toml_if_empty().await.expect("migrate recurring");
    todo::reminders::migrate_from_toml_if_empty().await.expect("migrate reminders");

    let tasks = todo::recurring::load_config().await.expect("load_config");
    assert!(tasks.iter().any(|t| t.title == "zz_test: Laundry"));
    assert_eq!(tasks.len(), 1);

    let reminders = todo::reminders::load_reminders().await.expect("load_reminders");
    assert!(reminders.iter().any(|r| r.title == "zz_test: Check blood pressure"));
    assert_eq!(reminders.len(), 1);

    // Idempotent: re-running against a now-non-empty table must not
    // duplicate rows — this is what makes it safe to call unconditionally
    // on every app startup.
    todo::recurring::migrate_from_toml_if_empty().await.expect("migrate recurring again");
    todo::reminders::migrate_from_toml_if_empty().await.expect("migrate reminders again");

    let tasks = todo::recurring::load_config().await.expect("load_config after second run");
    assert_eq!(tasks.len(), 1, "second migration run must not duplicate rows");
    let reminders = todo::reminders::load_reminders().await.expect("load_reminders after second run");
    assert_eq!(reminders.len(), 1, "second migration run must not duplicate rows");
}
