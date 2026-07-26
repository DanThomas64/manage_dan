use chrono::NaiveDate;
use finances::models::{Frequency, SpendingCategory, TxnKind};

#[tokio::test]
async fn full_roundtrip_against_real_hledger() {
    let dir = std::env::temp_dir().join(format!("finances-smoke-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let journal = dir.join("test.journal");
    let journal_path = journal.to_str().unwrap();

    finances::init(journal_path).expect("init should succeed, hledger must be installed");

    let e1 = finances::add_spending_entry(
        journal_path,
        SpendingCategory::Stupid,
        12.5,
        "Junk food",
        NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
    )
    .await
    .expect("add stupid entry");

    finances::add_spending_entry(
        journal_path,
        SpendingCategory::Survival,
        60.0,
        "Groceries",
        NaiveDate::from_ymd_opt(2026, 7, 6).unwrap(),
    )
    .await
    .expect("add survival entry");

    let entries = finances::list_spending_entries(
        journal_path,
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
    )
    .await
    .expect("list entries");
    assert_eq!(entries.len(), 2, "expected 2 entries, got {:?}", entries);

    let stats = finances::spending_stats(
        journal_path,
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
    )
    .await
    .expect("stats");
    assert_eq!(stats.stupid, 12.5);
    assert_eq!(stats.survival, 60.0);

    finances::add_recurring_item(
        journal_path,
        "Salary",
        2000.0,
        TxnKind::Income,
        "salary",
        Frequency::Biweekly,
        Some(NaiveDate::from_ymd_opt(2026, 1, 6).unwrap()),
    )
    .await
    .expect("add recurring income");

    finances::add_recurring_item(
        journal_path,
        "Netflix",
        15.0,
        TxnKind::Expense,
        "netflix",
        Frequency::Monthly,
        None,
    )
    .await
    .expect("add recurring expense");

    let recurring = finances::list_recurring_items(journal_path)
        .await
        .expect("list recurring");
    assert_eq!(recurring.len(), 2, "expected 2 recurring items, got {:?}", recurring);

    let proj = finances::projection(journal_path, 6)
        .await
        .expect("projection");
    assert!(!proj.is_empty(), "projection should have points");
    println!("projection points: {:#?}", proj);

    finances::delete_spending_entry(journal_path, &e1.id)
        .await
        .expect("delete entry");
    let entries_after = finances::list_spending_entries(
        journal_path,
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
    )
    .await
    .expect("list after delete");
    assert_eq!(entries_after.len(), 1);

    std::fs::remove_dir_all(&dir).ok();
}
