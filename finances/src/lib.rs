//! Finances subsystem.
//!
//! Backed by the external `hledger` CLI rather than the app's own SQLite
//! database: a plain-text double-entry journal file is the source of truth
//! (mirrors how the `notes` subsystem treats `nb`-managed markdown files as
//! its source of truth). `hledger` itself supplies the double-entry
//! accounting model, periodic-rule recurring transactions, and forecasting —
//! this crate only formats/parses journal text and shells out for queries.

pub mod finances_error;
pub mod finances_prelude;
pub mod hledger_client;
pub mod journal_parser;
pub mod journal_writer;
pub mod models;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use crate::finances_prelude::*;
use crate::models::{
    Account, AccountKind, CategoryTotals, Frequency, PayoffPoint, PreviewItem, ProjectionPoint,
    RecurringItem, RecurringTransfer, SpendingCategory, SpendingEntry, TransferEntry, TxnKind,
};
use chrono::{Local, Months, NaiveDate};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

pub use finances_error::FinancesLibError;

/// Initialises the finances subsystem: confirms `hledger` is installed and
/// ensures the configured journal file exists (an empty file is valid
/// hledger input; a missing one is not — confirmed via manual testing). A
/// journal that didn't exist at all (a brand-new deployment, or a fresh
/// `data/` dir) is seeded with one default "Checking" asset account rather
/// than left completely empty, so the app isn't accountless — spending and
/// recurring entries can't be created without at least one account to post
/// against — until someone thinks to add one by hand. An already-existing
/// (even empty) journal is left untouched: only true absence counts as
/// "new", so this never re-seeds a journal a user has deliberately cleared.
pub fn init(journal_path: &str) -> FinancesLibResult {
    info!("initializing finances");
    let out = std::process::Command::new("hledger")
        .arg("--version")
        .output()
        .map_err(|_| FinancesLibError::HledgerNotInstalled)?;
    if !out.status.success() {
        return Err(FinancesLibError::CannotInitialize(
            "hledger --version failed".to_string(),
        ));
    }

    let path = Path::new(journal_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(FinancesLibError::Io)?;
        }
    }
    if !path.exists() {
        let id = uuid::Uuid::new_v4().to_string();
        let seed = journal_writer::format_account_directive(
            &id,
            "Checking",
            AccountKind::Asset,
            "checking",
            None,
            None,
        );
        std::fs::write(path, seed.as_bytes()).map_err(FinancesLibError::Io)?;
        info!("finances: seeded new journal with default Checking account");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// hledger JSON response shapes (only the fields we actually read; unlisted
// fields are ignored by serde's default derive behaviour).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct HlQuantity {
    #[serde(rename = "floatingPoint")]
    floating_point: f64,
}

#[derive(Debug, Deserialize)]
struct HlAmount {
    aquantity: HlQuantity,
}

#[derive(Debug, Deserialize)]
struct HlPosting {
    paccount: String,
    pamount: Vec<HlAmount>,
}

#[derive(Debug, Deserialize)]
struct HlTransaction {
    tdate: String,
    tdescription: String,
    ttags: Vec<(String, String)>,
    tpostings: Vec<HlPosting>,
}

#[derive(Debug, Deserialize)]
struct HlDatePoint {
    contents: String,
}

#[derive(Debug, Deserialize)]
struct HlPeriodTotals {
    #[serde(rename = "prrAmounts")]
    prr_amounts: Vec<Vec<HlAmount>>,
}

#[derive(Debug, Deserialize)]
struct HlMultiBalance {
    #[serde(rename = "prDates")]
    pr_dates: Vec<[HlDatePoint; 2]>,
    #[serde(rename = "prTotals")]
    pr_totals: HlPeriodTotals,
}

fn amount_total(amounts: &[HlAmount]) -> f64 {
    amounts.iter().map(|a| a.aquantity.floating_point).sum()
}

/// hledger's own `-e/--end` report-period bound is *exclusive* (confirmed
/// against a real `hledger` install: `-e 2026-07-28` returns zero
/// transactions for an entry dated exactly 2026-07-28) — but every caller
/// here treats its own `to: NaiveDate` parameter as an *inclusive* last day
/// (e.g. `spending_stats(path, from, Local::now().date_naive())` is meant
/// to include today's entries). Formatting `to + 1 day` for the `-e` flag
/// bridges that gap once, here, rather than at every call site.
fn end_exclusive(to: NaiveDate) -> String {
    (to + chrono::Duration::days(1)).format("%Y-%m-%d").to_string()
}

// ---------------------------------------------------------------------------
// Spending entries
// ---------------------------------------------------------------------------

pub async fn add_spending_entry(
    journal_path: &str,
    category: SpendingCategory,
    amount: f64,
    description: &str,
    date: NaiveDate,
    account: &str,
) -> FinancesLibResult<SpendingEntry> {
    journal_writer::append_spending_entry(journal_path, category, amount, description, date, account)
        .await
}

pub async fn list_spending_entries(
    journal_path: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> FinancesLibResult<Vec<SpendingEntry>> {
    let from_s = from.format("%Y-%m-%d").to_string();
    let to_s = end_exclusive(to);
    let out = hledger_client::run(
        journal_path,
        &[
            "print",
            "expenses:stupid",
            "expenses:survival",
            "-b",
            &from_s,
            "-e",
            &to_s,
            "-O",
            "json",
        ],
    )
    .await?;
    let txns: Vec<HlTransaction> = serde_json::from_str(&out)?;

    let entries = txns
        .into_iter()
        .filter_map(|t| {
            let category = t
                .tpostings
                .iter()
                .find_map(|p| SpendingCategory::from_account(&p.paccount))?;
            let amount_posting = t
                .tpostings
                .iter()
                .find(|p| SpendingCategory::from_account(&p.paccount).is_some())?;
            let amount = amount_total(&amount_posting.pamount);
            let account = t
                .tpostings
                .iter()
                .find(|p| SpendingCategory::from_account(&p.paccount).is_none())
                .map(|p| p.paccount.clone())
                .unwrap_or_default();
            let id = t
                .ttags
                .iter()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let date = NaiveDate::parse_from_str(&t.tdate, "%Y-%m-%d").ok()?;
            Some(SpendingEntry {
                id,
                date,
                description: t.tdescription,
                category,
                amount,
                account,
            })
        })
        .collect();

    Ok(entries)
}

pub async fn spending_stats(
    journal_path: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> FinancesLibResult<CategoryTotals> {
    let from_s = from.format("%Y-%m-%d").to_string();
    let to_s = end_exclusive(to);
    let out = hledger_client::run(
        journal_path,
        &[
            "balance",
            "expenses:stupid",
            "expenses:survival",
            "-b",
            &from_s,
            "-e",
            &to_s,
            "-O",
            "json",
        ],
    )
    .await?;
    let (rows, _totals): (Vec<(String, String, i64, Vec<HlAmount>)>, Vec<HlAmount>) =
        serde_json::from_str(&out)?;

    let mut totals = CategoryTotals::default();
    for (account, _display, _depth, amounts) in rows {
        let value = amount_total(&amounts);
        if account.starts_with("expenses:stupid") {
            totals.stupid += value;
        } else if account.starts_with("expenses:survival") {
            totals.survival += value;
        }
    }
    Ok(totals)
}

pub async fn delete_spending_entry(journal_path: &str, id: &str) -> FinancesLibResult<()> {
    let removed = journal_writer::remove_block_with_id(journal_path, id).await?;
    if !removed {
        return Err(FinancesLibError::EntryNotFound(id.to_string()));
    }
    Ok(())
}

pub async fn update_spending_entry(
    journal_path: &str,
    id: &str,
    category: SpendingCategory,
    amount: f64,
    description: &str,
    date: NaiveDate,
    account: &str,
) -> FinancesLibResult<SpendingEntry> {
    journal_writer::rewrite_spending_entry(journal_path, id, date, description, category, amount, account)
        .await?
        .ok_or_else(|| FinancesLibError::EntryNotFound(id.to_string()))
}

// ---------------------------------------------------------------------------
// Transfers (one-off, between two of the user's own accounts)
// ---------------------------------------------------------------------------

pub async fn add_transfer_entry(
    journal_path: &str,
    description: &str,
    amount: f64,
    date: NaiveDate,
    from_account: &str,
    to_account: &str,
) -> FinancesLibResult<TransferEntry> {
    journal_writer::append_transfer_entry(journal_path, description, amount, date, from_account, to_account)
        .await
}

/// Unlike spending entries (a fixed pair of category accounts), a
/// transfer's two legs can be any pair of accounts — so instead of
/// filtering by account name, real transfer transactions are found via
/// hledger's own `tag:transfer` query, matching the `transfer:1` comma-tag
/// `journal_writer::format_transfer_entry` writes.
pub async fn list_transfer_entries(
    journal_path: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> FinancesLibResult<Vec<TransferEntry>> {
    let from_s = from.format("%Y-%m-%d").to_string();
    let to_s = end_exclusive(to);
    let out = hledger_client::run(
        journal_path,
        &["print", "tag:transfer", "-b", &from_s, "-e", &to_s, "-O", "json"],
    )
    .await?;
    let txns: Vec<HlTransaction> = serde_json::from_str(&out)?;

    let entries = txns
        .into_iter()
        .filter_map(|t| {
            let to_posting = t.tpostings.iter().find(|p| amount_total(&p.pamount) > 0.0)?;
            let from_posting = t.tpostings.iter().find(|p| amount_total(&p.pamount) < 0.0)?;
            let amount = amount_total(&to_posting.pamount);
            let id = t
                .ttags
                .iter()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let date = NaiveDate::parse_from_str(&t.tdate, "%Y-%m-%d").ok()?;
            Some(TransferEntry {
                id,
                date,
                description: t.tdescription,
                amount,
                from_account: from_posting.paccount.clone(),
                to_account: to_posting.paccount.clone(),
            })
        })
        .collect();

    Ok(entries)
}

pub async fn delete_transfer_entry(journal_path: &str, id: &str) -> FinancesLibResult<()> {
    let removed = journal_writer::remove_block_with_id(journal_path, id).await?;
    if !removed {
        return Err(FinancesLibError::TransferNotFound(id.to_string()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Recurring items
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn add_recurring_item(
    journal_path: &str,
    name: &str,
    amount: f64,
    kind: TxnKind,
    label: &str,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
    account: &str,
    category: Option<SpendingCategory>,
) -> FinancesLibResult<RecurringItem> {
    journal_writer::append_recurring_item(
        journal_path,
        name,
        amount,
        kind,
        label,
        frequency,
        reference_date,
        account,
        category,
    )
    .await
}

pub async fn list_recurring_items(journal_path: &str) -> FinancesLibResult<Vec<RecurringItem>> {
    let content = tokio::fs::read_to_string(journal_path)
        .await
        .map_err(FinancesLibError::Io)?;
    Ok(journal_parser::parse_recurring_items(&content))
}

pub async fn delete_recurring_item(journal_path: &str, id: &str) -> FinancesLibResult<()> {
    let removed = journal_writer::remove_block_with_id(journal_path, id).await?;
    if !removed {
        return Err(FinancesLibError::RecurringItemNotFound(id.to_string()));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn update_recurring_item(
    journal_path: &str,
    id: &str,
    name: &str,
    amount: f64,
    kind: TxnKind,
    label: &str,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
    account: &str,
    category: Option<SpendingCategory>,
) -> FinancesLibResult<RecurringItem> {
    journal_writer::rewrite_recurring_item(
        journal_path,
        id,
        name,
        amount,
        kind,
        label,
        frequency,
        reference_date,
        account,
        category,
    )
    .await?
    .ok_or_else(|| FinancesLibError::RecurringItemNotFound(id.to_string()))
}

// ---------------------------------------------------------------------------
// Recurring transfers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn add_recurring_transfer(
    journal_path: &str,
    name: &str,
    amount: f64,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
    from_account: &str,
    to_account: &str,
) -> FinancesLibResult<RecurringTransfer> {
    journal_writer::append_recurring_transfer(
        journal_path,
        name,
        amount,
        frequency,
        reference_date,
        from_account,
        to_account,
    )
    .await
}

pub async fn list_recurring_transfers(journal_path: &str) -> FinancesLibResult<Vec<RecurringTransfer>> {
    let content = tokio::fs::read_to_string(journal_path)
        .await
        .map_err(FinancesLibError::Io)?;
    Ok(journal_parser::parse_recurring_transfers(&content))
}

pub async fn delete_recurring_transfer(journal_path: &str, id: &str) -> FinancesLibResult<()> {
    let removed = journal_writer::remove_block_with_id(journal_path, id).await?;
    if !removed {
        return Err(FinancesLibError::RecurringTransferNotFound(id.to_string()));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn update_recurring_transfer(
    journal_path: &str,
    id: &str,
    name: &str,
    amount: f64,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
    from_account: &str,
    to_account: &str,
) -> FinancesLibResult<RecurringTransfer> {
    journal_writer::rewrite_recurring_transfer(
        journal_path,
        id,
        name,
        amount,
        frequency,
        reference_date,
        from_account,
        to_account,
    )
    .await?
    .ok_or_else(|| FinancesLibError::RecurringTransferNotFound(id.to_string()))
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

pub async fn create_account(
    journal_path: &str,
    name: &str,
    kind: AccountKind,
    interest_rate: Option<f64>,
    credit_limit: Option<f64>,
) -> FinancesLibResult<Account> {
    journal_writer::append_account(journal_path, name, kind, interest_rate, credit_limit).await
}

/// Lists registered accounts with their live hledger-computed balances (one
/// combined `hledger balance` call for all of them, not one query each).
pub async fn list_accounts(journal_path: &str) -> FinancesLibResult<Vec<Account>> {
    let content = tokio::fs::read_to_string(journal_path)
        .await
        .map_err(FinancesLibError::Io)?;
    let mut accounts = journal_parser::parse_accounts(&content);
    if accounts.is_empty() {
        return Ok(accounts);
    }

    let paths: Vec<String> = accounts.iter().map(|a| a.hledger_account()).collect();
    let mut args: Vec<&str> = vec!["balance"];
    args.extend(paths.iter().map(|p| p.as_str()));
    args.extend(["--flat", "-O", "json"]);
    let out = hledger_client::run(journal_path, &args).await?;
    let (rows, _totals): (Vec<(String, String, i64, Vec<HlAmount>)>, Vec<HlAmount>) =
        serde_json::from_str(&out)?;

    for account in accounts.iter_mut() {
        let target = account.hledger_account();
        account.balance = rows
            .iter()
            .filter(|(acct, ..)| *acct == target)
            .map(|(_, _, _, amounts)| amount_total(amounts))
            .sum();
    }

    Ok(accounts)
}

pub async fn delete_account(journal_path: &str, id: &str) -> FinancesLibResult<()> {
    let removed = journal_writer::remove_block_with_id(journal_path, id).await?;
    if !removed {
        return Err(FinancesLibError::AccountNotFound(id.to_string()));
    }
    Ok(())
}

/// Updates an account's `name`/`interest_rate`/`credit_limit` (delete +
/// re-append with the same id, same idiom as `update_recurring_item`).
/// `kind`/`slug` aren't user-editable here, so they're looked up from the
/// existing row rather than accepted as input — mirrors how
/// `set_account_balance` already leaves those alone too.
pub async fn update_account(
    journal_path: &str,
    id: &str,
    name: &str,
    interest_rate: Option<f64>,
    credit_limit: Option<f64>,
) -> FinancesLibResult<Account> {
    let content = tokio::fs::read_to_string(journal_path)
        .await
        .map_err(FinancesLibError::Io)?;
    let existing = journal_parser::parse_accounts(&content)
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| FinancesLibError::AccountNotFound(id.to_string()))?;

    let mut updated = journal_writer::rewrite_account(
        journal_path,
        id,
        name,
        existing.kind,
        &existing.slug,
        interest_rate,
        credit_limit,
    )
    .await?
    .ok_or_else(|| FinancesLibError::AccountNotFound(id.to_string()))?;
    updated.balance = existing.balance;
    Ok(updated)
}

/// "Quickly update the current balance": rather than storing the balance
/// directly, this posts an adjustment transaction for the difference
/// between the entered target and hledger's currently-computed balance —
/// hledger stays the sole source of truth, and every correction leaves a
/// dated, auditable entry rather than silently overwriting a number.
pub async fn set_account_balance(
    journal_path: &str,
    account_id: &str,
    target_balance: f64,
) -> FinancesLibResult<Account> {
    let accounts = list_accounts(journal_path).await?;
    let mut account = accounts
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| FinancesLibError::AccountNotFound(account_id.to_string()))?;

    let diff = target_balance - account.balance;
    if diff != 0.0 {
        let today = Local::now().date_naive();
        journal_writer::append_adjustment_transaction(
            journal_path,
            &account.hledger_account(),
            diff,
            today,
        )
        .await?;
    }
    account.balance = target_balance;
    Ok(account)
}

// ---------------------------------------------------------------------------
// Debt payoff projection
//
// Deliberately separate from `projection()` below: hledger's own
// `--forecast` posts a fixed $ amount per period from a periodic rule, so it
// has no way to compute "X% of the current balance" each month — true
// compounding interest can't be expressed as one more `~` rule for hledger
// itself to forecast. This is a from-scratch Rust calculation instead,
// answering "how long until this debt is paid off" the way hledger's own
// engine can't.
// ---------------------------------------------------------------------------

/// One compounding step, pure and I/O-free so it's unit-testable without a
/// journal file or hledger subprocess. `starting_balance` follows this
/// crate's existing sign convention (a liability balance is negative when
/// money is owed, confirmed in journal_writer.rs); `rate` is a period rate
/// already divided down from an APR (daily: `rate / 100.0 / 365.0`); a
/// `net_scheduled_amount` is the signed sum of that period's scheduled
/// payments/charges against the account (positive pays debt down). Interest
/// is `starting_balance * rate`, applied to the balance carried in at the
/// *start* of the period, before that period's scheduled payments — the
/// same convention a credit card statement uses (issuers compound daily
/// internally even though they only report a monthly statement total).
/// Returns `(interest_charged, ending_balance)`.
fn compound_period(starting_balance: f64, rate: f64, net_scheduled_amount: f64) -> (f64, f64) {
    let interest_charged = starting_balance * rate;
    let ending_balance = starting_balance + interest_charged + net_scheduled_amount;
    (interest_charged, ending_balance)
}

/// The set of exact dates a recurring item/transfer occurs on, within
/// `[start, end]` inclusive — computed once per item over the whole
/// projection horizon with one stable anchor (`reference_date`, or `start`
/// itself when unset), not per-day. A per-day `occurrences_between(ref,
/// day, day)` call would re-anchor a reference-date-less item at that same
/// day every time it's called (the anchor defaults to the window's own
/// start), which would make it fire on literally every day rather than
/// respecting its actual frequency — this precomputed set is what lets the
/// daily payoff loop below check "does this occur today" cheaply and
/// correctly via a single `HashSet::contains`.
fn occurrence_date_set(frequency: Frequency, reference_date: Option<NaiveDate>, start: NaiveDate, end: NaiveDate) -> HashSet<NaiveDate> {
    frequency.occurrences_between(reference_date, start, end).into_iter().collect()
}

/// How many consecutive daily compounding steps get folded into a single
/// returned `PayoffPoint` — 1 (a real daily point) for a horizon of 12
/// months or less, 7 (weekly) beyond that. Same "cap the point count for a
/// wide horizon" reasoning as `balance_forecast`'s `-D`/`-W` switch, kept
/// as a plain day threshold here since the payoff loop has no hledger
/// period-flag equivalent to lean on.
fn payoff_step_days(months_ahead: u32) -> i64 {
    if months_ahead <= 12 { 1 } else { 7 }
}

/// Accumulates one or more consecutive daily `compound_period` steps into a
/// single `PayoffPoint`. Every day is still compounded individually and in
/// order — interest and scheduled amounts within a bucket are exact sums,
/// never approximated by treating the whole bucket as one wide period —
/// only the *emitted* points are coarser than daily when `step_days > 1`.
struct PayoffBucketer {
    step_days: i64,
    days_in_bucket: i64,
    bucket_start: NaiveDate,
    bucket_start_balance: f64,
    bucket_interest: f64,
    bucket_net: f64,
}

impl PayoffBucketer {
    fn new(step_days: i64, start: NaiveDate, start_balance: f64) -> Self {
        Self {
            step_days,
            days_in_bucket: 0,
            bucket_start: start,
            bucket_start_balance: start_balance,
            bucket_interest: 0.0,
            bucket_net: 0.0,
        }
    }

    /// Records one day's step (`day` being that day, already applied to
    /// produce `ending_balance`). Returns `Some(PayoffPoint)` once the
    /// bucket should flush — either it's full, or `force` is set (the
    /// account was just paid off, or this was the horizon's last day) —
    /// so a partial bucket at either end still gets its own accurate
    /// point rather than being silently dropped or padded.
    fn step(
        &mut self,
        day: NaiveDate,
        interest_charged: f64,
        net_scheduled_amount: f64,
        ending_balance: f64,
        force: bool,
    ) -> Option<PayoffPoint> {
        self.bucket_interest += interest_charged;
        self.bucket_net += net_scheduled_amount;
        self.days_in_bucket += 1;
        if self.days_in_bucket < self.step_days && !force {
            return None;
        }
        let point = PayoffPoint {
            period_start: self.bucket_start,
            period_end: day,
            starting_balance: self.bucket_start_balance,
            interest_charged: self.bucket_interest,
            net_scheduled_amount: self.bucket_net,
            ending_balance,
        };
        self.bucket_start = day + chrono::Duration::days(1);
        self.bucket_start_balance = ending_balance;
        self.bucket_interest = 0.0;
        self.bucket_net = 0.0;
        self.days_in_bucket = 0;
        Some(point)
    }
}

/// Projects a single account's balance forward `months_ahead` calendar
/// months, one point per *day*, compounding `account.interest_rate` daily
/// (APR/365) against the live balance and layering in every recurring
/// item/transfer that posts against this account on the days it actually
/// occurs (via `occurrence_date_set` above). An account with
/// `interest_rate: None` behaves identically to `Some(0.0)` — pure linear
/// amortization, useful as a cross-check. Stops early (returns fewer than
/// the full day count) once a liability account's balance reaches `>= 0.0`
/// — paid off.
pub async fn debt_payoff_projection(
    journal_path: &str,
    account_id: &str,
    months_ahead: u32,
) -> FinancesLibResult<Vec<PayoffPoint>> {
    debt_payoff_projection_with_overrides(journal_path, account_id, months_ahead, &[], &[]).await
}

/// Same as `debt_payoff_projection`, plus an optional scenario overlay:
/// `extra_items` are hypothetical recurring items layered on top of the
/// real schedule (never persisted — same throwaway-input idea as
/// `PreviewItem` in `preview_projection`, but no scratch journal file is
/// needed here, since this function never calls hledger's own forecast
/// engine for the schedule in the first place — it already loops over
/// `list_recurring_items`/`list_recurring_transfers` in plain Rust, so a
/// scenario item is just another entry in that same in-memory Vec).
/// `exclude_recurring_ids` drops matching real items/transfers first, the
/// same "preview removing this one" idea `preview_projection` already
/// supports for the combined chart.
pub async fn debt_payoff_projection_with_overrides(
    journal_path: &str,
    account_id: &str,
    months_ahead: u32,
    extra_items: &[PreviewItem],
    exclude_recurring_ids: &[String],
) -> FinancesLibResult<Vec<PayoffPoint>> {
    let account = list_accounts(journal_path)
        .await?
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| FinancesLibError::AccountNotFound(account_id.to_string()))?;
    let hledger_account = account.hledger_account();
    let daily_rate = account.interest_rate.unwrap_or(0.0) / 100.0 / 365.0;

    let mut items: Vec<RecurringItem> = list_recurring_items(journal_path)
        .await?
        .into_iter()
        .filter(|i| !exclude_recurring_ids.contains(&i.id))
        .collect();
    let transfers: Vec<RecurringTransfer> = list_recurring_transfers(journal_path)
        .await?
        .into_iter()
        .filter(|t| !exclude_recurring_ids.contains(&t.id))
        .collect();
    items.extend(extra_items.iter().map(|preview| RecurringItem {
        id: format!("preview-{}", uuid::Uuid::new_v4()),
        name: preview.name.clone(),
        amount: preview.amount,
        kind: preview.kind,
        label: "preview".to_string(),
        frequency: preview.frequency,
        reference_date: preview.reference_date,
        account: preview.account.clone(),
        category: None,
    }));

    let today = Local::now().date_naive();
    let end_date = models::add_months_clamped(today, months_ahead as i32);
    let last_day = end_date - chrono::Duration::days(1);

    // Relevant-only + precomputed once (not one hledger-style query per
    // day) — see `occurrence_date_set`'s doc comment for why per-day
    // re-anchoring would be wrong.
    let item_schedules: Vec<(&RecurringItem, HashSet<NaiveDate>)> = items
        .iter()
        .filter(|i| i.account == hledger_account)
        .map(|i| (i, occurrence_date_set(i.frequency, i.reference_date, today, last_day)))
        .collect();
    let transfer_schedules: Vec<(&RecurringTransfer, HashSet<NaiveDate>)> = transfers
        .iter()
        .filter(|t| t.to_account == hledger_account || t.from_account == hledger_account)
        .map(|t| (t, occurrence_date_set(t.frequency, t.reference_date, today, last_day)))
        .collect();

    let mut balance = account.balance;
    let mut points = Vec::new();
    let mut bucketer = PayoffBucketer::new(payoff_step_days(months_ahead), today, balance);
    let mut day = today;

    while day < end_date {
        let starting_balance = balance;

        let mut net_scheduled_amount = 0.0;
        for (item, dates) in &item_schedules {
            if dates.contains(&day) {
                net_scheduled_amount += match item.kind {
                    TxnKind::Income => item.amount,
                    TxnKind::Expense => -item.amount,
                };
            }
        }
        for (transfer, dates) in &transfer_schedules {
            if dates.contains(&day) {
                if transfer.to_account == hledger_account {
                    net_scheduled_amount += transfer.amount;
                }
                if transfer.from_account == hledger_account {
                    net_scheduled_amount -= transfer.amount;
                }
            }
        }

        let (interest_charged, ending_balance) =
            compound_period(starting_balance, daily_rate, net_scheduled_amount);
        balance = ending_balance;

        let paid_off = account.kind == AccountKind::Liability && ending_balance >= 0.0;
        let next_day = day + chrono::Duration::days(1);
        if let Some(point) = bucketer.step(day, interest_charged, net_scheduled_amount, ending_balance, paid_off || next_day >= end_date) {
            points.push(point);
        }
        if paid_off {
            break;
        }
        day = next_day;
    }

    Ok(points)
}

/// Same output shape as `debt_payoff_projection`, but for designing a
/// payment plan from scratch rather than reading real recurring items —
/// real recurring items/transfers against this account are ignored
/// entirely (a plan *replaces* them for this account's payoff chart, it
/// never stacks with them). `minimum_interest_only`, when true, pays
/// exactly that day's accrued interest every day (so the balance neither
/// grows nor shrinks on its own); `supplementary_amount` is an extra fixed
/// payment on its own `supplementary_frequency`, reducing principal,
/// stacked on top of the minimum. Both can be used independently —
/// supplementary alone (minimum off) still lets interest compound but
/// chips away at principal; minimum alone (supplementary zero) holds the
/// balance flat forever. One point per day — see
/// `debt_payoff_projection_with_overrides`'s doc for why.
#[allow(clippy::too_many_arguments)]
pub async fn debt_payoff_projection_with_plan(
    journal_path: &str,
    account_id: &str,
    months_ahead: u32,
    minimum_interest_only: bool,
    supplementary_amount: f64,
    supplementary_frequency: Frequency,
    supplementary_reference_date: Option<NaiveDate>,
) -> FinancesLibResult<Vec<PayoffPoint>> {
    let account = list_accounts(journal_path)
        .await?
        .into_iter()
        .find(|a| a.id == account_id)
        .ok_or_else(|| FinancesLibError::AccountNotFound(account_id.to_string()))?;
    let daily_rate = account.interest_rate.unwrap_or(0.0) / 100.0 / 365.0;

    let today = Local::now().date_naive();
    let end_date = models::add_months_clamped(today, months_ahead as i32);
    let last_day = end_date - chrono::Duration::days(1);
    let supplementary_dates = occurrence_date_set(supplementary_frequency, supplementary_reference_date, today, last_day);

    let mut balance = account.balance;
    let mut points = Vec::new();
    let mut bucketer = PayoffBucketer::new(payoff_step_days(months_ahead), today, balance);
    let mut day = today;

    while day < end_date {
        let starting_balance = balance;

        let interest = starting_balance * daily_rate;
        let minimum_payment = if minimum_interest_only { -interest } else { 0.0 };
        let supplementary_payment = if supplementary_dates.contains(&day) { supplementary_amount } else { 0.0 };
        let net_scheduled_amount = minimum_payment + supplementary_payment;

        let (interest_charged, ending_balance) =
            compound_period(starting_balance, daily_rate, net_scheduled_amount);
        balance = ending_balance;

        let paid_off = account.kind == AccountKind::Liability && ending_balance >= 0.0;
        let next_day = day + chrono::Duration::days(1);
        if let Some(point) = bucketer.step(day, interest_charged, net_scheduled_amount, ending_balance, paid_off || next_day >= end_date) {
            points.push(point);
        }
        if paid_off {
            break;
        }
        day = next_day;
    }

    Ok(points)
}

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Combines recorded history with hledger's own periodic-rule forecasting
/// for whichever account paths are given — no schedule math of our own,
/// hledger already does this (`--historical --forecast`). One point per
/// *day* (`-D`) for a horizon of 12 months or less, or per *week* (`-W`)
/// beyond that — a daily point count grows unbounded with the horizon
/// (3650+ points for a 10-year projection) while adding little visible
/// detail once the timespan is that wide, so this caps it the same way a
/// weekly point count does for a horizon of any length actually asked for.
/// hledger computes the same real ending balances regardless of period
/// size, it just reports them at coarser/finer intervals — confirmed
/// against a real `hledger` install that both `-D` and `-W` return the
/// identical JSON shape (`prDates`/`prTotals`).
///
/// Always passes an explicit `-b <today>` alongside `--historical` —
/// without it, hledger's report begins at the journal's very *first*
/// transaction, not "today," so on any journal with more than a few days
/// of real history this returned one extra daily/weekly point per day
/// *already elapsed* since account creation on top of the `months_ahead`
/// window actually asked for (confirmed against a real `hledger` install:
/// a journal with ~7 months of history and a 1-month-ahead request
/// returned 205 points instead of the ~31 expected). `--historical` is
/// exactly the flag that makes this safe — it folds every transaction
/// *before* `-b` into the first reported period's starting balance rather
/// than dropping it, so the numbers are unaffected, only the extra
/// already-past points disappear. Symptoms on the frontend: charts far
/// longer than the months slider requested, checkpoint/trendline math
/// computed relative to `points[0].period_start` silently anchored to
/// account-creation day instead of today, and — because that long-ago
/// history and the real recent/forecasted activity both plot in the same
/// series with no visual "today" marker — something that could easily
/// read as duplicated or inflated spending on a quick look at the chart.
/// `projection()` and `account_balance_history()` are both thin wrappers
/// over this, scoped to `["assets", "liabilities"]` or a single account
/// respectively.
async fn balance_forecast(
    journal_path: &str,
    account_paths: &[&str],
    months_ahead: u32,
) -> FinancesLibResult<Vec<ProjectionPoint>> {
    let today = Local::now().date_naive();
    let begin = today.format("%Y-%m-%d").to_string();
    let end = today
        .checked_add_months(Months::new(months_ahead))
        .unwrap_or(today)
        .format("%Y-%m-%d")
        .to_string();
    let period_flag = if months_ahead <= 12 { "-D" } else { "-W" };

    let mut args: Vec<&str> = vec!["balance"];
    args.extend(account_paths.iter().copied());
    args.extend([period_flag, "--historical", "--forecast", "-b", &begin, "-e", &end, "-O", "json"]);

    let out = hledger_client::run(journal_path, &args).await?;

    let parsed: HlMultiBalance = serde_json::from_str(&out)?;

    let points = parsed
        .pr_dates
        .iter()
        .zip(parsed.pr_totals.prr_amounts.iter())
        .filter_map(|(dates, amounts)| {
            let period_start = NaiveDate::parse_from_str(&dates[0].contents, "%Y-%m-%d").ok()?;
            let period_end = NaiveDate::parse_from_str(&dates[1].contents, "%Y-%m-%d").ok()?;
            let balance = amount_total(amounts);
            Some(ProjectionPoint {
                period_start,
                period_end,
                balance,
            })
        })
        .collect();

    Ok(points)
}

/// Previews the effect of adding any number of hypothetical recurring items
/// and/or excluding existing ones from the forecast balance trend for the
/// given account paths, without persisting anything to the real journal —
/// reuses hledger's own forecast math (rather than approximating it) by
/// writing a scratch copy of the journal (real content, minus any blocks
/// tagged with an id in `exclude_recurring_ids`, plus one extra periodic
/// rule per hypothetical item) and running `balance_forecast` against that,
/// then deleting the scratch file regardless of outcome. If both `items`
/// and `exclude_recurring_ids` are empty, skips the scratch file entirely
/// and just re-runs the real forecast.
async fn balance_forecast_preview(
    journal_path: &str,
    account_paths: &[&str],
    months_ahead: u32,
    items: &[PreviewItem],
    exclude_recurring_ids: &[String],
) -> FinancesLibResult<Vec<ProjectionPoint>> {
    if items.is_empty() && exclude_recurring_ids.is_empty() {
        return balance_forecast(journal_path, account_paths, months_ahead).await;
    }

    let real_content = tokio::fs::read_to_string(journal_path)
        .await
        .map_err(FinancesLibError::Io)?;

    let mut combined = if exclude_recurring_ids.is_empty() {
        real_content
    } else {
        real_content
            .split("\n\n")
            .filter(|block| {
                !exclude_recurring_ids
                    .iter()
                    .any(|id| block.contains(&format!("id:{id}")))
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    for item in items {
        let scratch_id = format!("preview-{}", uuid::Uuid::new_v4());
        let name = if item.name.trim().is_empty() { "Preview" } else { item.name.trim() };
        combined.push('\n');
        combined.push_str(&journal_writer::format_recurring_item(
            &scratch_id,
            name,
            item.amount,
            item.kind,
            "preview",
            item.frequency,
            item.reference_date,
            &item.account,
            None,
        ));
    }
    let scratch_path = format!("{journal_path}.preview-{}.tmp", uuid::Uuid::new_v4());
    tokio::fs::write(&scratch_path, combined.as_bytes())
        .await
        .map_err(FinancesLibError::Io)?;

    let result = balance_forecast(&scratch_path, account_paths, months_ahead).await;
    let _ = tokio::fs::remove_file(&scratch_path).await;
    result
}

/// Projects overall assets/liabilities balance forward — see
/// `balance_forecast`.
pub async fn projection(
    journal_path: &str,
    months_ahead: u32,
) -> FinancesLibResult<Vec<ProjectionPoint>> {
    balance_forecast(journal_path, &["assets", "liabilities"], months_ahead).await
}

/// Previews the effect of hypothetical/excluded recurring items on the
/// overall assets/liabilities balance — see `balance_forecast_preview`.
pub async fn preview_projection(
    journal_path: &str,
    months_ahead: u32,
    items: &[PreviewItem],
    exclude_recurring_ids: &[String],
) -> FinancesLibResult<Vec<ProjectionPoint>> {
    balance_forecast_preview(journal_path, &["assets", "liabilities"], months_ahead, items, exclude_recurring_ids).await
}

/// Same hledger-forecast math as `projection()`, scoped to one account —
/// deliberately the *plain* linear forecast, not the interest-aware Rust
/// math `debt_payoff_projection` uses for liabilities; this is the general
/// "what does this account's balance look like over time" view for any
/// account, asset or liability.
pub async fn account_balance_history(
    journal_path: &str,
    account_path: &str,
    months_ahead: u32,
) -> FinancesLibResult<Vec<ProjectionPoint>> {
    balance_forecast(journal_path, &[account_path], months_ahead).await
}

/// Previews the effect of hypothetical/excluded recurring items on a single
/// account's own balance trend — see `balance_forecast_preview`.
pub async fn account_balance_history_preview(
    journal_path: &str,
    account_path: &str,
    months_ahead: u32,
    items: &[PreviewItem],
    exclude_recurring_ids: &[String],
) -> FinancesLibResult<Vec<ProjectionPoint>> {
    balance_forecast_preview(journal_path, &[account_path], months_ahead, items, exclude_recurring_ids).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compound_period_zero_rate_is_pure_linear_amortization() {
        let (interest, ending) = compound_period(-1200.0, 0.0, 100.0);
        assert_eq!(interest, 0.0);
        assert_eq!(ending, -1100.0);
    }

    #[test]
    fn compound_period_none_rate_behaves_like_some_zero() {
        // debt_payoff_projection computes monthly_rate as
        // `interest_rate.unwrap_or(0.0) / 100.0 / 12.0`, so `None` and
        // `Some(0.0)` both reduce to the same `monthly_rate: 0.0` input here.
        let none_rate = 0.0_f64;
        let some_zero_rate = Some(0.0_f64).unwrap_or(0.0) / 100.0 / 12.0;
        assert_eq!(compound_period(-1200.0, none_rate, 100.0), compound_period(-1200.0, some_zero_rate, 100.0));
    }

    #[test]
    fn compound_period_nonzero_rate_makes_debt_worse_before_payment() {
        let monthly_rate = 24.99 / 100.0 / 12.0;
        let (interest, ending) = compound_period(-1000.0, monthly_rate, 100.0);
        assert!(interest < 0.0, "interest on a debt should be negative (makes it worse)");
        // Ending balance should improve less than a same-sized payment would
        // with zero interest (-1000 + 100 = -900).
        assert!(ending < -900.0);
    }

    #[test]
    fn compound_period_payment_less_than_interest_never_improves() {
        let monthly_rate = 24.99 / 100.0 / 12.0;
        // $10 payment against $2000 debt at ~2.08%/mo (~$41.65 interest) —
        // payment doesn't cover the interest, balance should get worse.
        let (_, ending) = compound_period(-2000.0, monthly_rate, 10.0);
        assert!(ending < -2000.0);
    }

    #[test]
    fn compound_period_nonzero_rate_takes_longer_to_pay_off_than_zero_rate() {
        let monthly_rate = 19.99 / 100.0 / 12.0;
        let mut zero_rate_balance = -1000.0_f64;
        let mut with_rate_balance = -1000.0_f64;
        let mut zero_rate_months = 0;
        let mut with_rate_months = 0;
        for _ in 0..60 {
            if zero_rate_balance < 0.0 {
                let (_, ending) = compound_period(zero_rate_balance, 0.0, 100.0);
                zero_rate_balance = ending;
                zero_rate_months += 1;
            }
            if with_rate_balance < 0.0 {
                let (_, ending) = compound_period(with_rate_balance, monthly_rate, 100.0);
                with_rate_balance = ending;
                with_rate_months += 1;
            }
        }
        assert!(with_rate_months > zero_rate_months);
    }

    #[tokio::test]
    async fn debt_payoff_projection_zero_rate_matches_linear_amortization() {
        let dir = std::env::temp_dir().join(format!("fin-payoff-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        create_account(&path, "Visa", AccountKind::Liability, None, None).await.unwrap();
        let accounts = list_accounts(&path).await.unwrap();
        let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
        // Give it a starting balance via the adjustment idiom.
        set_account_balance(&path, &visa.id, -1000.0).await.unwrap();
        add_recurring_item(
            &path, "Visa payment", 100.0, TxnKind::Expense, "visa-payment",
            Frequency::Monthly, None, &visa.hledger_account(), None,
        ).await.unwrap();

        let today = Local::now().date_naive();
        let expected_days = (models::add_months_clamped(today, 12) - today).num_days() as usize;

        let points = debt_payoff_projection(&path, &visa.id, 12).await.unwrap();
        assert!(!points.is_empty());
        assert_eq!(points[0].interest_charged, 0.0);
        // One point per day over the full 12-month horizon (never paid
        // off, so it never stops early).
        assert_eq!(points.len(), expected_days);
        // $1000 debt, $100/mo expense against it (makes it worse, matching
        // this crate's Expense-leg sign convention), 12 monthly occurrences
        // over 12 months, no interest (rate: None) -> exactly -$1200 net.
        assert_eq!(points.last().unwrap().ending_balance, -2200.0);
        assert!(points.last().unwrap().ending_balance < points[0].starting_balance);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn debt_payoff_projection_switches_to_weekly_buckets_beyond_12_months() {
        let dir = std::env::temp_dir().join(format!("fin-payoff-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        create_account(&path, "Visa", AccountKind::Liability, None, None).await.unwrap();
        let accounts = list_accounts(&path).await.unwrap();
        let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
        set_account_balance(&path, &visa.id, -1000.0).await.unwrap();
        add_recurring_item(
            &path, "Visa payment", 100.0, TxnKind::Expense, "visa-payment",
            Frequency::Monthly, None, &visa.hledger_account(), None,
        ).await.unwrap();

        let today = Local::now().date_naive();
        let expected_days = (models::add_months_clamped(today, 18) - today).num_days();
        let expected_points = ((expected_days + 6) / 7) as usize;

        let points = debt_payoff_projection(&path, &visa.id, 18).await.unwrap();
        // One point per *week* beyond a 12-month horizon, not per day —
        // confirms `payoff_step_days`/`PayoffBucketer` actually kick in.
        assert_eq!(points.len(), expected_points, "expected weekly bucketing beyond 12 months");
        // No bucket (including the final, possibly partial, one) spans
        // more than 7 days.
        for p in &points {
            assert!((p.period_end - p.period_start).num_days() < 7, "bucket wider than 7 days: {:?}", p);
        }
        // 18 monthly $100 charges over 18 months, no interest (rate: None)
        // -> exactly -$1800 net, same total regardless of bucket width.
        assert_eq!(points.last().unwrap().ending_balance, -2800.0);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn debt_payoff_projection_stops_early_once_paid_off() {
        let dir = std::env::temp_dir().join(format!("fin-payoff-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        create_account(&path, "Visa", AccountKind::Liability, None, None).await.unwrap();
        let accounts = list_accounts(&path).await.unwrap();
        let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
        set_account_balance(&path, &visa.id, -100.0).await.unwrap();
        // A recurring transfer *to* the liability account pays it down.
        add_recurring_transfer(
            &path, "Payoff", 100.0, Frequency::Monthly, None,
            "assets:checking", &visa.hledger_account(),
        ).await.unwrap();

        let points = debt_payoff_projection(&path, &visa.id, 12).await.unwrap();
        // Paid off in the first month -> stops early rather than running
        // all 12.
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].ending_balance, 0.0);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn debt_payoff_projection_with_overrides_empty_matches_plain_version() {
        let dir = std::env::temp_dir().join(format!("fin-payoff-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        create_account(&path, "Visa", AccountKind::Liability, Some(19.99), None).await.unwrap();
        let accounts = list_accounts(&path).await.unwrap();
        let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
        set_account_balance(&path, &visa.id, -500.0).await.unwrap();
        add_recurring_item(
            &path, "Visa charge", 50.0, TxnKind::Expense, "visa",
            Frequency::Monthly, None, &visa.hledger_account(), None,
        ).await.unwrap();

        let plain = debt_payoff_projection(&path, &visa.id, 6).await.unwrap();
        let overridden = debt_payoff_projection_with_overrides(&path, &visa.id, 6, &[], &[]).await.unwrap();
        assert_eq!(plain.len(), overridden.len());
        for (a, b) in plain.iter().zip(overridden.iter()) {
            assert_eq!(a.ending_balance, b.ending_balance);
        }

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn debt_payoff_projection_with_overrides_extra_item_worsens_trajectory() {
        let dir = std::env::temp_dir().join(format!("fin-payoff-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        create_account(&path, "Visa", AccountKind::Liability, None, None).await.unwrap();
        let accounts = list_accounts(&path).await.unwrap();
        let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
        set_account_balance(&path, &visa.id, -500.0).await.unwrap();

        let baseline = debt_payoff_projection(&path, &visa.id, 3).await.unwrap();
        let extra_item = PreviewItem {
            name: "Extra charge".to_string(),
            amount: 100.0,
            kind: TxnKind::Expense,
            frequency: Frequency::Monthly,
            reference_date: None,
            account: visa.hledger_account(),
        };
        let with_extra =
            debt_payoff_projection_with_overrides(&path, &visa.id, 3, &[extra_item], &[]).await.unwrap();

        assert!(with_extra.last().unwrap().ending_balance < baseline.last().unwrap().ending_balance);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn debt_payoff_projection_with_overrides_excludes_real_item() {
        let dir = std::env::temp_dir().join(format!("fin-payoff-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        create_account(&path, "Visa", AccountKind::Liability, None, None).await.unwrap();
        let accounts = list_accounts(&path).await.unwrap();
        let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
        set_account_balance(&path, &visa.id, -500.0).await.unwrap();
        let charge = add_recurring_item(
            &path, "Visa charge", 100.0, TxnKind::Expense, "visa",
            Frequency::Monthly, None, &visa.hledger_account(), None,
        ).await.unwrap();

        let with_charge = debt_payoff_projection(&path, &visa.id, 3).await.unwrap();
        let excluded =
            debt_payoff_projection_with_overrides(&path, &visa.id, 3, &[], &[charge.id.clone()]).await.unwrap();

        // Excluding the only recurring charge against this liability
        // leaves it with zero net scheduled amount and zero interest (no
        // rate set), so the balance never changes.
        assert!(excluded.last().unwrap().ending_balance > with_charge.last().unwrap().ending_balance);
        assert_eq!(excluded[0].ending_balance, -500.0);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn debt_payoff_projection_with_plan_interest_only_holds_balance_flat() {
        let dir = std::env::temp_dir().join(format!("fin-payoff-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        create_account(&path, "Visa", AccountKind::Liability, Some(24.0), None).await.unwrap();
        let accounts = list_accounts(&path).await.unwrap();
        let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
        set_account_balance(&path, &visa.id, -1000.0).await.unwrap();

        // Even with a real recurring charge against this account, the
        // plan must ignore it entirely (replace, not stack).
        add_recurring_item(
            &path, "Visa charge", 500.0, TxnKind::Expense, "visa",
            Frequency::Monthly, None, &visa.hledger_account(), None,
        ).await.unwrap();

        let today = Local::now().date_naive();
        let expected_days = (models::add_months_clamped(today, 6) - today).num_days() as usize;

        let points = debt_payoff_projection_with_plan(
            &path, &visa.id, 6, true, 0.0, Frequency::Monthly, None,
        ).await.unwrap();

        assert_eq!(points.len(), expected_days, "one point per day over the full horizon (never paid off on its own)");
        for p in &points {
            assert_eq!(p.starting_balance, -1000.0, "interest-only should hold the balance flat every day");
            assert_eq!(p.ending_balance, -1000.0);
        }

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn debt_payoff_projection_with_plan_supplementary_speeds_up_payoff() {
        let dir = std::env::temp_dir().join(format!("fin-payoff-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        create_account(&path, "Visa", AccountKind::Liability, Some(24.0), None).await.unwrap();
        let accounts = list_accounts(&path).await.unwrap();
        let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
        set_account_balance(&path, &visa.id, -1000.0).await.unwrap();

        let minimum_only = debt_payoff_projection_with_plan(
            &path, &visa.id, 12, true, 0.0, Frequency::Monthly, None,
        ).await.unwrap();
        let with_supplementary = debt_payoff_projection_with_plan(
            &path, &visa.id, 12, true, 200.0, Frequency::Monthly, None,
        ).await.unwrap();

        // Minimum-only never improves (flat forever); adding a
        // supplementary payment on top must actually reduce the balance.
        assert_eq!(minimum_only.last().unwrap().ending_balance, -1000.0);
        assert!(with_supplementary.last().unwrap().ending_balance > -1000.0);
    }

    #[tokio::test]
    async fn debt_payoff_projection_with_plan_zero_zero_never_improves() {
        let dir = std::env::temp_dir().join(format!("fin-payoff-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        create_account(&path, "Visa", AccountKind::Liability, Some(24.0), None).await.unwrap();
        let accounts = list_accounts(&path).await.unwrap();
        let visa = accounts.iter().find(|a| a.name == "Visa").unwrap();
        set_account_balance(&path, &visa.id, -1000.0).await.unwrap();

        let today = Local::now().date_naive();
        let expected_days = (models::add_months_clamped(today, 6) - today).num_days() as usize;

        let points = debt_payoff_projection_with_plan(
            &path, &visa.id, 6, false, 0.0, Frequency::Monthly, None,
        ).await.unwrap();

        // No payments at all: interest keeps compounding the debt worse
        // every day.
        assert_eq!(points.len(), expected_days);
        for pair in points.windows(2) {
            assert!(pair[1].ending_balance < pair[0].ending_balance);
        }

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn projection_excludes_history_before_today_from_point_count() {
        let dir = std::env::temp_dir().join(format!("fin-proj-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        let today = Local::now().date_naive();
        let old_date = today - chrono::Duration::days(200);

        // Seed an account with a real transaction dated well in the past —
        // before the `-b <today>` fix, `balance_forecast` had no begin
        // bound at all, so hledger's report started at the journal's
        // *first* transaction (here, ~200 days ago) instead of today,
        // inflating the point count with that much extra already-elapsed
        // history on top of the requested `months_ahead` window.
        create_account(&path, "Checking", AccountKind::Asset, None, None).await.unwrap();
        let accounts = list_accounts(&path).await.unwrap();
        let checking = accounts.iter().find(|a| a.name == "Checking").unwrap();
        add_spending_entry(&path, SpendingCategory::Stupid, 20.0, "Old junk food", old_date, &checking.hledger_account())
            .await
            .unwrap();

        let points = projection(&path, 1).await.unwrap();
        let expected_days = (models::add_months_clamped(today, 1) - today).num_days() as usize;
        assert_eq!(
            points.len(), expected_days,
            "projection must cover only the requested months-ahead window, not the journal's full history"
        );
        assert_eq!(
            points[0].period_start, today,
            "first point must start today, not the account's old-entry date"
        );

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
