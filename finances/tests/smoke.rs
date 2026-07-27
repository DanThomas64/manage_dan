use chrono::NaiveDate;
use finances::models::{AccountKind, Frequency, SpendingCategory, TxnKind};

#[tokio::test]
async fn full_roundtrip_against_real_hledger() {
    let dir = std::env::temp_dir().join(format!("finances-smoke-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let journal = dir.join("test.journal");
    let journal_path = journal.to_str().unwrap();

    finances::init(journal_path).expect("init should succeed, hledger must be installed");

    // init() seeds a brand-new journal with a default "Checking" account —
    // reuse it instead of creating a duplicate.
    let seeded_accounts = finances::list_accounts(journal_path).await.expect("list seeded accounts");
    assert_eq!(seeded_accounts.len(), 1, "expected init() to seed exactly 1 account, got {:?}", seeded_accounts);
    let checking = seeded_accounts.into_iter().find(|a| a.name == "Checking").expect("seeded Checking account");
    assert_eq!(checking.kind, AccountKind::Asset);

    let visa = finances::create_account(journal_path, "Visa", AccountKind::Liability)
        .await
        .expect("create visa account");

    let e1 = finances::add_spending_entry(
        journal_path,
        SpendingCategory::Stupid,
        12.5,
        "Junk food",
        NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
        &checking.hledger_account(),
    )
    .await
    .expect("add stupid entry");

    finances::add_spending_entry(
        journal_path,
        SpendingCategory::Survival,
        60.0,
        "Groceries",
        NaiveDate::from_ymd_opt(2026, 7, 6).unwrap(),
        &visa.hledger_account(),
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
        &checking.hledger_account(),
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
        &checking.hledger_account(),
    )
    .await
    .expect("add recurring expense");

    let recurring = finances::list_recurring_items(journal_path)
        .await
        .expect("list recurring");
    assert_eq!(recurring.len(), 2, "expected 2 recurring items, got {:?}", recurring);

    let accounts = finances::list_accounts(journal_path)
        .await
        .expect("list accounts");
    assert_eq!(accounts.len(), 2, "expected 2 accounts, got {:?}", accounts);
    let checking_after = accounts.iter().find(|a| a.id == checking.id).expect("checking present");
    // checking: +12.5 spending posted against it (debit side of the expense
    // entry is expenses:stupid, credit is checking) -> checking balance -12.5
    assert_eq!(checking_after.balance, -12.5);

    let adjusted = finances::set_account_balance(journal_path, &checking.id, 500.0)
        .await
        .expect("set balance");
    assert_eq!(adjusted.balance, 500.0);
    let accounts_after_adjust = finances::list_accounts(journal_path)
        .await
        .expect("list accounts after adjust");
    let checking_reconciled = accounts_after_adjust
        .iter()
        .find(|a| a.id == checking.id)
        .expect("checking present after adjust");
    assert_eq!(checking_reconciled.balance, 500.0);

    let savings = finances::create_account(journal_path, "Savings", AccountKind::Asset)
        .await
        .expect("create savings account");

    let transfer = finances::add_transfer_entry(
        journal_path,
        "Move to savings",
        100.0,
        NaiveDate::from_ymd_opt(2026, 7, 10).unwrap(),
        &checking.hledger_account(),
        &savings.hledger_account(),
    )
    .await
    .expect("add transfer entry");

    let transfers = finances::list_transfer_entries(
        journal_path,
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
    )
    .await
    .expect("list transfers");
    assert_eq!(transfers.len(), 1, "expected 1 transfer, got {:?}", transfers);
    assert_eq!(transfers[0].amount, 100.0);
    assert_eq!(transfers[0].from_account, checking.hledger_account());
    assert_eq!(transfers[0].to_account, savings.hledger_account());

    let accounts_after_transfer = finances::list_accounts(journal_path)
        .await
        .expect("list accounts after transfer");
    let checking_after_transfer = accounts_after_transfer.iter().find(|a| a.id == checking.id).unwrap();
    let savings_after_transfer = accounts_after_transfer.iter().find(|a| a.id == savings.id).unwrap();
    assert_eq!(checking_after_transfer.balance, 400.0, "500 reconciled - 100 transferred out");
    assert_eq!(savings_after_transfer.balance, 100.0);

    finances::delete_transfer_entry(journal_path, &transfer.id)
        .await
        .expect("delete transfer");
    let transfers_after_delete = finances::list_transfer_entries(
        journal_path,
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
    )
    .await
    .expect("list transfers after delete");
    assert_eq!(transfers_after_delete.len(), 0);

    let rec_transfer = finances::add_recurring_transfer(
        journal_path,
        "Auto-save",
        50.0,
        Frequency::Monthly,
        None,
        &checking.hledger_account(),
        &savings.hledger_account(),
    )
    .await
    .expect("add recurring transfer");

    let recurring_transfers = finances::list_recurring_transfers(journal_path)
        .await
        .expect("list recurring transfers");
    assert_eq!(recurring_transfers.len(), 1, "expected 1 recurring transfer, got {:?}", recurring_transfers);
    assert_eq!(recurring_transfers[0].from_account, checking.hledger_account());
    assert_eq!(recurring_transfers[0].to_account, savings.hledger_account());

    // A recurring transfer must not leak into the regular income/expense
    // recurring-items list (they're parsed by two separate, non-overlapping
    // passes over the same file).
    let recurring_after_transfer = finances::list_recurring_items(journal_path)
        .await
        .expect("list recurring after transfer added");
    assert_eq!(recurring_after_transfer.len(), 2, "recurring transfer leaked into regular recurring items");

    finances::delete_recurring_transfer(journal_path, &rec_transfer.id)
        .await
        .expect("delete recurring transfer");
    let recurring_transfers_after = finances::list_recurring_transfers(journal_path)
        .await
        .expect("list recurring transfers after delete");
    assert_eq!(recurring_transfers_after.len(), 0);

    finances::delete_account(journal_path, &visa.id)
        .await
        .expect("delete visa account");
    let accounts_after_delete = finances::list_accounts(journal_path)
        .await
        .expect("list accounts after delete");
    assert_eq!(accounts_after_delete.len(), 2, "checking + savings remain after deleting visa");

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
