//! End-to-end exercise of archive -> restore -> permanent delete against
//! scratch nb notebooks, a scratch project base_dir, and a scratch working
//! directory (so it never touches the real `app.sqlite`, the real `todo`/
//! notes notebooks, or the live systemd service).

const TEST_NOTEBOOK: &str = "zz_test_project_todo";

fn cleanup_notebooks(slug: &str) {
    let _ = std::process::Command::new("nb")
        .args(["notebooks", "delete", TEST_NOTEBOOK, "--force"])
        .output();
    let _ = std::process::Command::new("nb")
        .args(["notebooks", "delete", slug, "--force"])
        .output();
    let _ = std::process::Command::new("nb")
        .args(["archive:delete", &format!("{}/", slug), "--force"])
        .output();
}

#[tokio::test]
async fn archive_then_restore_then_delete() {
    let scratch = std::env::temp_dir().join(format!("project_test_{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    std::env::set_current_dir(&scratch).expect("cd into scratch dir");

    let slug = "zz_test_project";
    cleanup_notebooks(slug);

    db::init().expect("db init");
    notes::init().expect("notes init");
    printer::init(0, 0, "terminal", 42).expect("printer init");
    todo::init(TEST_NOTEBOOK).expect("todo init");
    lists::init().expect("lists init");
    project::init("./projects").expect("project init");

    let created = project::create_project("Zz Test Project").await.expect("create_project");
    assert_eq!(created.slug, slug);
    assert!(created.archived_at.is_none());
    assert!(std::path::Path::new(&created.fs_path).is_dir());

    // A todo and a note, both scoped to the project.
    let mut item = todo::models::TodoItem::new("scoped todo".to_string(), String::new());
    item.project_title = Some(slug.to_string());
    todo::create_item(item).await.expect("create scoped todo");

    notes::create(notes::CreateNoteRequest {
        title: Some("scoped note".to_string()),
        content: "body".to_string(),
        tags: Some(vec![created.tag.clone()]),
        notebook: Some(slug.to_string()),
    })
    .await
    .expect("create scoped note");

    let before = project::project_detail(created.id).await.expect("detail before archive");
    assert_eq!(before.todos.len(), 1);
    assert_eq!(before.notes.len(), 1);

    // Archive: fs folder gone, zip exists, live detail blanks out.
    let archived = project::archive_project(created.id).await.expect("archive_project");
    assert!(archived.archived_at.is_some());
    assert!(!std::path::Path::new(&created.fs_path).exists());
    let zip_path = format!("./projects/.archive/{}.zip", slug);
    assert!(std::path::Path::new(&zip_path).is_file(), "zip should exist after archiving");

    let during = project::project_detail(created.id).await.expect("detail while archived");
    assert!(during.todos.is_empty());
    assert!(during.notes.is_empty());

    // Restore: fs folder back, zip gone, scoped items visible again.
    let restored = project::restore_project(created.id).await.expect("restore_project");
    assert!(restored.archived_at.is_none());
    assert!(std::path::Path::new(&created.fs_path).is_dir(), "fs folder should be back after restore");
    assert!(!std::path::Path::new(&zip_path).exists(), "zip should be gone after restore");

    let after_restore = project::project_detail(created.id).await.expect("detail after restore");
    assert_eq!(after_restore.todos.len(), 1, "todo should be back after restore");
    assert_eq!(after_restore.notes.len(), 1, "note should be back after restore");

    // A live project can't be permanently deleted.
    let live_delete = project::delete_project(created.id).await;
    assert!(live_delete.is_err(), "deleting a non-archived project must fail");

    // Archive again, then permanently delete.
    project::archive_project(created.id).await.expect("re-archive");
    project::delete_project(created.id).await.expect("delete_project");

    let gone = project::get_project(created.id).await;
    assert!(gone.is_err(), "project row should be gone after permanent delete");
    assert!(!std::path::Path::new(&zip_path).exists(), "zip should be gone after permanent delete");

    cleanup_notebooks(slug);
}
