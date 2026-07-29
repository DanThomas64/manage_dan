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

    let visa = finances::create_account(journal_path, "Visa", AccountKind::Liability, Some(24.99), Some(2000.0))
        .await
        .expect("create visa account");
    assert_eq!(visa.interest_rate, Some(24.99));
    assert_eq!(visa.credit_limit, Some(2000.0));

    // Confirm interest_rate/credit_limit round-trip through a real hledger
    // balance query too (list_accounts re-parses the journal and re-queries
    // balances) — not just through the in-memory struct returned above.
    let accounts_with_rate = finances::list_accounts(journal_path).await.expect("list accounts with rate");
    let visa_reloaded = accounts_with_rate.iter().find(|a| a.id == visa.id).expect("visa present");
    assert_eq!(visa_reloaded.interest_rate, Some(24.99));
    assert_eq!(visa_reloaded.credit_limit, Some(2000.0));

    let visa_updated = finances::update_account(journal_path, &visa.id, "Visa Signature", Some(19.99), Some(2500.0))
        .await
        .expect("update visa account");
    assert_eq!(visa_updated.name, "Visa Signature");
    assert_eq!(visa_updated.interest_rate, Some(19.99));
    assert_eq!(visa_updated.credit_limit, Some(2500.0));
    let accounts_after_update = finances::list_accounts(journal_path).await.expect("list accounts after update");
    assert_eq!(accounts_after_update.len(), 2, "update must not duplicate the account block");

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

    let e1_updated = finances::update_spending_entry(
        journal_path,
        &e1.id,
        SpendingCategory::Survival,
        20.0,
        "Snacks",
        NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
        &checking.hledger_account(),
    )
    .await
    .expect("update spending entry");
    assert_eq!(e1_updated.description, "Snacks");
    assert_eq!(e1_updated.amount, 20.0);
    assert_eq!(e1_updated.category, SpendingCategory::Survival);
    let entries_after_update = finances::list_spending_entries(
        journal_path,
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
    )
    .await
    .expect("list entries after update");
    assert_eq!(entries_after_update.len(), 2, "update must not duplicate the spending entry");

    // hledger's own `-e/--end` is exclusive, but `list_spending_entries`'s
    // `to` parameter is meant to be inclusive of that exact day — a `to`
    // equal to an entry's own date must still return it (this regressed
    // once before: an entry dated exactly "today" silently vanished from
    // the Spending tab and its totals until the next day).
    let entries_to_exact_date = finances::list_spending_entries(
        journal_path,
        NaiveDate::from_ymd_opt(2026, 7, 6).unwrap(),
        NaiveDate::from_ymd_opt(2026, 7, 6).unwrap(),
    )
    .await
    .expect("list entries with to == entry's own date");
    assert_eq!(
        entries_to_exact_date.len(), 1,
        "entry dated exactly `to` must be included (hledger's -e is exclusive, this must compensate), got {:?}",
        entries_to_exact_date
    );

    let stats = finances::spending_stats(
        journal_path,
        NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
    )
    .await
    .expect("stats");
    // e1 was updated above from a $12.5 stupid entry to a $20 survival
    // entry, so stupid is now empty and survival covers both entries.
    assert_eq!(stats.stupid, 0.0);
    assert_eq!(stats.survival, 80.0);

    finances::add_recurring_item(
        journal_path,
        "Salary",
        2000.0,
        TxnKind::Income,
        "salary",
        Frequency::Biweekly,
        Some(NaiveDate::from_ymd_opt(2026, 1, 6).unwrap()),
        &checking.hledger_account(),
        None,
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
        Some(SpendingCategory::Stupid),
    )
    .await
    .expect("add recurring expense");

    let recurring = finances::list_recurring_items(journal_path)
        .await
        .expect("list recurring");
    assert_eq!(recurring.len(), 2, "expected 2 recurring items, got {:?}", recurring);

    let netflix = recurring.iter().find(|r| r.name == "Netflix").expect("netflix present");
    assert_eq!(netflix.category, Some(SpendingCategory::Stupid), "category should round-trip through list_recurring_items");
    let netflix_updated = finances::update_recurring_item(
        journal_path,
        &netflix.id,
        "Netflix Premium",
        22.99,
        TxnKind::Expense,
        "netflix",
        Frequency::Monthly,
        None,
        &checking.hledger_account(),
        Some(SpendingCategory::Stupid),
    )
    .await
    .expect("update recurring item");
    assert_eq!(netflix_updated.name, "Netflix Premium");
    assert_eq!(netflix_updated.amount, 22.99);
    assert_eq!(netflix_updated.category, Some(SpendingCategory::Stupid));
    let recurring_after_update = finances::list_recurring_items(journal_path)
        .await
        .expect("list recurring after update");
    assert_eq!(recurring_after_update.len(), 2, "update must not duplicate the recurring item block");

    let accounts = finances::list_accounts(journal_path)
        .await
        .expect("list accounts");
    assert_eq!(accounts.len(), 2, "expected 2 accounts, got {:?}", accounts);
    let checking_after = accounts.iter().find(|a| a.id == checking.id).expect("checking present");
    // e1 was updated above from $12.5 to $20.0 (still posted against
    // checking) -> checking balance -20.0
    assert_eq!(checking_after.balance, -20.0);

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

    let savings = finances::create_account(journal_path, "Savings", AccountKind::Asset, None, None)
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

    let rec_transfer_updated = finances::update_recurring_transfer(
        journal_path,
        &rec_transfer.id,
        "Auto-save Plus",
        75.0,
        Frequency::Monthly,
        None,
        &checking.hledger_account(),
        &savings.hledger_account(),
    )
    .await
    .expect("update recurring transfer");
    assert_eq!(rec_transfer_updated.name, "Auto-save Plus");
    assert_eq!(rec_transfer_updated.amount, 75.0);
    let recurring_transfers_after_update = finances::list_recurring_transfers(journal_path)
        .await
        .expect("list recurring transfers after update");
    assert_eq!(recurring_transfers_after_update.len(), 1, "update must not duplicate the recurring transfer block");

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

    // Sanity check debt_payoff_projection's "gather live balance + recurring
    // items via hledger" input path end-to-end (the compounding math itself
    // is covered by pure unit tests in lib.rs's own test module).
    let payoff = finances::debt_payoff_projection(journal_path, &visa.id, 6)
        .await
        .expect("debt payoff projection");
    assert!(!payoff.is_empty(), "payoff projection should have points");
    println!("payoff points: {:#?}", payoff);

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

    // account_balance_history scopes the same forecast math to a single
    // account's own hledger path rather than the combined assets+liabilities
    // total `projection()` uses.
    let checking_history = finances::account_balance_history(journal_path, &checking.hledger_account(), 6)
        .await
        .expect("account balance history");
    assert!(!checking_history.is_empty(), "account balance history should have points");
    println!("checking history points: {:#?}", checking_history);

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
