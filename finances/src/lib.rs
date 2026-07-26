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

use crate::finances_prelude::*;
use crate::models::{
    CategoryTotals, Frequency, ProjectionPoint, RecurringItem, SpendingCategory, SpendingEntry,
    TxnKind,
};
use chrono::{Local, Months, NaiveDate};
use serde::Deserialize;
use std::path::Path;

pub use finances_error::FinancesLibError;

/// Initialises the finances subsystem: confirms `hledger` is installed and
/// ensures the configured journal file exists (an empty file is valid
/// hledger input; a missing one is not — confirmed via manual testing).
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
        std::fs::File::create(path).map_err(FinancesLibError::Io)?;
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

// ---------------------------------------------------------------------------
// Spending entries
// ---------------------------------------------------------------------------

pub async fn add_spending_entry(
    journal_path: &str,
    category: SpendingCategory,
    amount: f64,
    description: &str,
    date: NaiveDate,
) -> FinancesLibResult<SpendingEntry> {
    journal_writer::append_spending_entry(journal_path, category, amount, description, date).await
}

pub async fn list_spending_entries(
    journal_path: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> FinancesLibResult<Vec<SpendingEntry>> {
    let from_s = from.format("%Y-%m-%d").to_string();
    let to_s = to.format("%Y-%m-%d").to_string();
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
    let to_s = to.format("%Y-%m-%d").to_string();
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

// ---------------------------------------------------------------------------
// Recurring items
// ---------------------------------------------------------------------------

pub async fn add_recurring_item(
    journal_path: &str,
    name: &str,
    amount: f64,
    kind: TxnKind,
    label: &str,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
) -> FinancesLibResult<RecurringItem> {
    journal_writer::append_recurring_item(
        journal_path,
        name,
        amount,
        kind,
        label,
        frequency,
        reference_date,
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

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Projects overall assets/liabilities balance forward, combining recorded
/// history with hledger's own periodic-rule forecasting — no schedule math
/// of our own, hledger already does this (`--historical --forecast`).
pub async fn projection(
    journal_path: &str,
    months_ahead: u32,
) -> FinancesLibResult<Vec<ProjectionPoint>> {
    let end = Local::now()
        .date_naive()
        .checked_add_months(Months::new(months_ahead))
        .unwrap_or_else(|| Local::now().date_naive())
        .format("%Y-%m-%d")
        .to_string();

    let out = hledger_client::run(
        journal_path,
        &[
            "balance",
            "assets",
            "liabilities",
            "-M",
            "--historical",
            "--forecast",
            "-e",
            &end,
            "-O",
            "json",
        ],
    )
    .await?;

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
