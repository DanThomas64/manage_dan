//! Merges `finances`'s recurring items/transfers (the schedule — hledger
//! periodic rules, journal-backed) with `db`'s `recurring_occurrence_status`
//! table (local paid/unpaid completion tracking) into a single per-item
//! summary. Kept here in `app` rather than as a dependency either crate
//! takes on the other: `finances` is deliberately hledger-only (journal is
//! its sole source of truth, mirroring `notes`'s relationship to `nb`), and
//! this feature's tracking is explicitly *not* real ledger data — it never
//! posts a journal transaction, just a completion checklist layered on top.
//! `app` already depends on both crates directly, the same role `project`
//! plays merging `lists`/`notes`/`todo`/`db` for its own aggregation needs.

use chrono::{Months, NaiveDate};
use finances::models::Frequency;
use std::collections::HashMap;

/// How far back an item/transfer with no resolved tracking row can surface
/// as "outstanding" — bounds the backlog for something that's simply never
/// been ticked, rather than listing every occurrence since its creation.
pub const LOOKBACK_MONTHS: u32 = 12;

#[derive(Debug, Clone, serde::Serialize)]
pub struct OccurrenceStatus {
    pub period_start: NaiveDate,
    pub paid: bool,
    /// A tracking row exists for this occurrence (paid or explicitly
    /// marked not-paid) — once true, this occurrence is never counted
    /// overdue again regardless of `paid`'s value.
    pub resolved: bool,
    pub overdue: bool,
    pub paid_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RecurringOccurrenceSummary {
    pub recurring_id: String,
    /// `"item"` | `"transfer"`.
    pub kind: String,
    pub name: String,
    /// The most recent occurrence within the lookback window — the one a
    /// single, un-modal'd tap of the tick reflects/toggles.
    pub current_period: OccurrenceStatus,
    /// Every unresolved occurrence within the window, oldest first,
    /// including `current_period` when it's itself unresolved — the
    /// catch-up modal's row list when there's more than one.
    pub outstanding: Vec<OccurrenceStatus>,
}

type TrackedMap = HashMap<(String, String, String), db::models::RecurringOccurrenceRow>;

fn window_start(as_of: NaiveDate, lookback_months: u32) -> NaiveDate {
    as_of.checked_sub_months(Months::new(lookback_months)).unwrap_or(as_of)
}

#[allow(clippy::too_many_arguments)]
fn build_summary(
    id: &str,
    kind: &str,
    name: &str,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
    start: NaiveDate,
    as_of: NaiveDate,
    tracked: &TrackedMap,
) -> Option<RecurringOccurrenceSummary> {
    let dates = frequency.occurrences_between(reference_date, start, as_of);
    let last_date = *dates.last()?;

    let statuses: Vec<OccurrenceStatus> = dates
        .iter()
        .map(|date| {
            let key = (id.to_string(), kind.to_string(), date.format("%Y-%m-%d").to_string());
            let row = tracked.get(&key);
            OccurrenceStatus {
                period_start: *date,
                paid: row.map(|r| r.paid).unwrap_or(false),
                resolved: row.is_some(),
                overdue: row.is_none() && *date < as_of,
                paid_date: row
                    .and_then(|r| r.paid_date.as_deref())
                    .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()),
            }
        })
        .collect();

    let current_period = statuses.iter().find(|s| s.period_start == last_date)?.clone();
    let outstanding: Vec<OccurrenceStatus> = statuses.into_iter().filter(|s| !s.resolved).collect();

    Some(RecurringOccurrenceSummary {
        recurring_id: id.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        current_period,
        outstanding,
    })
}

fn tracked_map(rows: Vec<db::models::RecurringOccurrenceRow>) -> TrackedMap {
    rows.into_iter()
        .map(|r| ((r.recurring_id.clone(), r.kind.clone(), r.period_start.clone()), r))
        .collect()
}

/// Every recurring item's/transfer's occurrence summary as of `as_of` — the
/// bulk read backing the frontend's per-card ticks/badges in one call
/// rather than one fetch per card.
pub async fn compute_occurrence_summaries(
    journal_path: &str,
    as_of: NaiveDate,
    lookback_months: u32,
) -> anyhow::Result<Vec<RecurringOccurrenceSummary>> {
    let items = finances::list_recurring_items(journal_path).await?;
    let transfers = finances::list_recurring_transfers(journal_path).await?;
    let tracked = tracked_map(db::recurring_occurrence_get_all().await?);
    let start = window_start(as_of, lookback_months);

    let mut summaries = Vec::with_capacity(items.len() + transfers.len());
    for item in &items {
        if let Some(summary) =
            build_summary(&item.id, "item", &item.name, item.frequency, item.reference_date, start, as_of, &tracked)
        {
            summaries.push(summary);
        }
    }
    for transfer in &transfers {
        if let Some(summary) = build_summary(
            &transfer.id,
            "transfer",
            &transfer.name,
            transfer.frequency,
            transfer.reference_date,
            start,
            as_of,
            &tracked,
        ) {
            summaries.push(summary);
        }
    }
    Ok(summaries)
}

/// A single recurring item's/transfer's occurrence summary. Recomputes the
/// full set and filters — this app's recurring-item counts are small
/// (personal finance use), so the simplicity outweighs the redundant work.
pub async fn compute_occurrences_for(
    journal_path: &str,
    id: &str,
    kind: &str,
    as_of: NaiveDate,
    lookback_months: u32,
) -> anyhow::Result<Option<RecurringOccurrenceSummary>> {
    let summaries = compute_occurrence_summaries(journal_path, as_of, lookback_months).await?;
    Ok(summaries.into_iter().find(|s| s.recurring_id == id && s.kind == kind))
}
