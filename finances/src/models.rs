use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpendingCategory {
    Stupid,
    Survival,
}

impl SpendingCategory {
    /// The hledger account this category posts to.
    pub fn account(&self) -> &'static str {
        match self {
            SpendingCategory::Stupid => "expenses:stupid",
            SpendingCategory::Survival => "expenses:survival",
        }
    }

    pub fn from_account(account: &str) -> Option<Self> {
        match account {
            "expenses:stupid" => Some(SpendingCategory::Stupid),
            "expenses:survival" => Some(SpendingCategory::Survival),
            _ => None,
        }
    }

    /// Short tag value used for `RecurringItem`'s optional `category:` tag
    /// (see `journal_writer::format_recurring_item`) — distinct from
    /// `account()`, which returns a full hledger account path rather than
    /// a bare tag value.
    pub fn tag(&self) -> &'static str {
        match self {
            SpendingCategory::Stupid => "stupid",
            SpendingCategory::Survival => "survival",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "stupid" => Some(SpendingCategory::Stupid),
            "survival" => Some(SpendingCategory::Survival),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxnKind {
    Income,
    Expense,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Frequency {
    Weekly,
    Biweekly,
    Monthly,
    Yearly,
}

impl Frequency {
    /// The hledger periodic-rule period expression this frequency maps to.
    pub fn period_phrase(&self) -> &'static str {
        match self {
            Frequency::Weekly => "weekly",
            Frequency::Biweekly => "every 2 weeks",
            Frequency::Monthly => "monthly",
            Frequency::Yearly => "yearly",
        }
    }

    pub fn from_period_phrase(phrase: &str) -> Option<Self> {
        match phrase {
            "weekly" => Some(Frequency::Weekly),
            "every 2 weeks" => Some(Frequency::Biweekly),
            "monthly" => Some(Frequency::Monthly),
            "yearly" => Some(Frequency::Yearly),
            _ => None,
        }
    }
}

/// Builds the full hledger period expression for a periodic-rule header —
/// just `frequency.period_phrase()` if there's no reference date, or that
/// phrase plus hledger's own `from <date>` anchor clause otherwise, so a
/// biweekly/etc. item's forecasted occurrences actually land on the
/// reference date's cadence instead of hledger's default anchor (today /
/// the report's start date).
pub fn build_period_phrase(frequency: Frequency, reference_date: Option<NaiveDate>) -> String {
    match reference_date {
        Some(d) => format!("{} from {}", frequency.period_phrase(), d.format("%Y-%m-%d")),
        None => frequency.period_phrase().to_string(),
    }
}

/// Inverse of `build_period_phrase`: splits a period expression back into its
/// `Frequency` and optional reference date.
pub fn parse_period_phrase(period: &str) -> Option<(Frequency, Option<NaiveDate>)> {
    match period.split_once(" from ") {
        Some((phrase, date_str)) => {
            let frequency = Frequency::from_period_phrase(phrase.trim())?;
            let reference_date = NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d").ok();
            Some((frequency, reference_date))
        }
        None => Frequency::from_period_phrase(period.trim()).map(|f| (f, None)),
    }
}

/// Adds `months` calendar months to `date`, clamping the day-of-month down
/// to the target month's last day rather than overflowing into the next
/// month (e.g. Jan 31 + 1 month = Feb 28, not Mar 3) — always computed from
/// `date`'s own day, never from an already-clamped intermediate date, so a
/// monthly schedule anchored on the 31st reverts to the 31st in any month
/// that has one rather than drifting permanently down to the 28th/30th.
pub(crate) fn add_months_clamped(date: NaiveDate, months: i32) -> NaiveDate {
    let total_months = date.year() * 12 + date.month() as i32 - 1 + months;
    let year = total_months.div_euclid(12);
    let month = total_months.rem_euclid(12) as u32 + 1;
    let last_day = last_day_of_month(year, month);
    NaiveDate::from_ymd_opt(year, month, date.day().min(last_day)).expect("valid clamped date")
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .expect("valid first-of-month date")
        .pred_opt()
        .expect("day before first-of-month is valid")
        .day()
}

impl Frequency {
    /// The `k`-th occurrence (0-indexed) of this frequency anchored at
    /// `anchor`, always computed directly from `anchor` (not from the
    /// previous occurrence) so monthly/yearly day-of-month clamping never
    /// compounds across steps.
    fn nth_occurrence(&self, anchor: NaiveDate, k: i64) -> NaiveDate {
        match self {
            Frequency::Weekly => anchor + chrono::Duration::days(7 * k),
            Frequency::Biweekly => anchor + chrono::Duration::days(14 * k),
            Frequency::Monthly => add_months_clamped(anchor, k as i32),
            Frequency::Yearly => add_months_clamped(anchor, 12 * k as i32),
        }
    }

    /// Concrete occurrence dates of this frequency, anchored at
    /// `reference_date` (or `start` itself when `None` — an item with no
    /// reference date has no unambiguous cadence anchor, so this is a
    /// documented fallback rather than a guess), falling within
    /// `[start, end]` inclusive.
    pub fn occurrences_between(
        &self,
        reference_date: Option<NaiveDate>,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Vec<NaiveDate> {
        if start > end {
            return Vec::new();
        }
        let anchor = reference_date.unwrap_or(start);
        let mut occurrences = Vec::new();
        let mut k: i64 = 0;
        loop {
            let date = self.nth_occurrence(anchor, k);
            if date > end {
                break;
            }
            if date >= start {
                occurrences.push(date);
            }
            k += 1;
        }
        occurrences
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountKind {
    Asset,
    Liability,
}

impl AccountKind {
    /// The hledger top-level account this kind posts under.
    pub fn prefix(&self) -> &'static str {
        match self {
            AccountKind::Asset => "assets",
            AccountKind::Liability => "liabilities",
        }
    }

    pub fn from_prefix(prefix: &str) -> Option<Self> {
        match prefix {
            "assets" => Some(AccountKind::Asset),
            "liabilities" => Some(AccountKind::Liability),
            _ => None,
        }
    }
}

/// A user-managed account (checking, savings, a credit card, ...) that
/// spending entries and recurring items post their non-category leg against.
/// Registered in the journal via an `account` directive line (declares the
/// name without requiring a transaction); `balance` is populated separately
/// from a live `hledger balance` query, never stored — hledger, not this
/// struct, is the source of truth for the number itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub kind: AccountKind,
    pub slug: String,
    pub balance: f64,
    /// Annual percentage rate, e.g. `24.99` — `None` for accounts with no
    /// interest (checking/savings, or a liability the user hasn't set a
    /// rate for yet). Feeds `debt_payoff_projection`'s compounding
    /// calculator; unrelated to `projection()`'s hledger-linear forecast.
    pub interest_rate: Option<f64>,
    /// Credit limit, e.g. `1000.0` — display/utilization metadata only
    /// (never enforced or fed into any projection math), `None` for
    /// accounts with no limit (any asset account, or a liability the user
    /// hasn't set one for yet).
    pub credit_limit: Option<f64>,
}

impl Account {
    /// The full hledger account path this account posts against, e.g.
    /// `assets:checking` or `liabilities:visa`.
    pub fn hledger_account(&self) -> String {
        format!("{}:{}", self.kind.prefix(), self.slug)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendingEntry {
    pub id: String,
    pub date: NaiveDate,
    pub description: String,
    pub category: SpendingCategory,
    pub amount: f64,
    pub account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurringItem {
    pub id: String,
    pub name: String,
    pub amount: f64,
    pub kind: TxnKind,
    pub label: String,
    pub frequency: Frequency,
    pub reference_date: Option<NaiveDate>,
    pub account: String,
    /// Optional budget-category attribution (stupid/survival) — purely
    /// bookkeeping metadata layered on top of the item's own `expenses:
    /// {label}` posting account (unchanged), not a change to which ledger
    /// account it posts against. Lets the Overview tab's Budget Caps
    /// section fold recurring commitments into a category's total
    /// alongside one-off `SpendingEntry` actuals. `None` for an
    /// uncategorized item (income items never carry one).
    pub category: Option<SpendingCategory>,
}

/// One hypothetical, never-persisted recurring item fed into
/// `preview_projection` — same shape as `RecurringItem`'s creation fields,
/// minus an `id` (each gets a fresh scratch-only one) and plus a
/// user-facing `name` so several can be told apart in the preview UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewItem {
    pub name: String,
    pub amount: f64,
    pub kind: TxnKind,
    pub frequency: Frequency,
    pub reference_date: Option<NaiveDate>,
    pub account: String,
}

/// A one-off movement of money between two of the user's own accounts —
/// e.g. Checking -> Savings. Distinct from `SpendingEntry`: neither leg is
/// an expense/income category, both are real accounts, so it's queried
/// back out of real hledger transactions via a `tag:transfer` search
/// rather than a fixed pair of category account names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferEntry {
    pub id: String,
    pub date: NaiveDate,
    pub description: String,
    pub amount: f64,
    pub from_account: String,
    pub to_account: String,
}

/// The periodic-rule equivalent of `TransferEntry` — a recurring movement
/// between two accounts (e.g. an automatic monthly transfer into savings),
/// parsed the same locally-read-file way `RecurringItem` is (periodic rules
/// aren't real hledger transactions, so there's nothing to query).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurringTransfer {
    pub id: String,
    pub name: String,
    pub amount: f64,
    pub frequency: Frequency,
    pub reference_date: Option<NaiveDate>,
    pub from_account: String,
    pub to_account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionPoint {
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub balance: f64,
}

/// One month of `debt_payoff_projection`'s compounding calculation — kept
/// separate from `ProjectionPoint` since the two come from different math
/// models (this one applies interest month-over-month in Rust; hledger's
/// own `--forecast` never does).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoffPoint {
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub starting_balance: f64,
    /// Negative for a liability (interest makes the debt worse).
    pub interest_charged: f64,
    /// Signed sum of that month's matching recurring items/transfers
    /// against this account (positive = paid down/deposited).
    pub net_scheduled_amount: f64,
    pub ending_balance: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CategoryTotals {
    pub stupid: f64,
    pub survival: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn add_months_clamped_normal_case() {
        assert_eq!(add_months_clamped(d(2026, 1, 15), 1), d(2026, 2, 15));
        assert_eq!(add_months_clamped(d(2026, 1, 15), 12), d(2027, 1, 15));
    }

    #[test]
    fn add_months_clamped_clamps_month_end() {
        // Jan 31 + 1 month -> Feb 28 (2026 is not a leap year), not Mar 3.
        assert_eq!(add_months_clamped(d(2026, 1, 31), 1), d(2026, 2, 28));
        // Jan 31 + 1 month in a leap year -> Feb 29.
        assert_eq!(add_months_clamped(d(2028, 1, 31), 1), d(2028, 2, 29));
        // The day-of-month reverts once the target month is long enough
        // again — computed from the original anchor day, not the previous
        // (already-clamped) step.
        assert_eq!(add_months_clamped(d(2026, 1, 31), 2), d(2026, 3, 31));
    }

    #[test]
    fn occurrences_between_weekly_no_reference_date() {
        let start = d(2026, 1, 1);
        let end = d(2026, 1, 31);
        let occ = Frequency::Weekly.occurrences_between(None, start, end);
        // Anchored at `start` itself when there's no reference date.
        assert_eq!(occ, vec![d(2026, 1, 1), d(2026, 1, 8), d(2026, 1, 15), d(2026, 1, 22), d(2026, 1, 29)]);
    }

    #[test]
    fn occurrences_between_biweekly_with_old_reference_date() {
        // Anchor is well before the window — confirms fast, correct
        // stepping rather than only working near the anchor.
        let anchor = d(2020, 1, 6);
        let start = d(2026, 1, 1);
        let end = d(2026, 1, 31);
        let occ = Frequency::Biweekly.occurrences_between(Some(anchor), start, end);
        for date in &occ {
            let days_since_anchor = (*date - anchor).num_days();
            assert_eq!(days_since_anchor % 14, 0, "{date} not on the biweekly cadence from {anchor}");
        }
        assert!(!occ.is_empty());
    }

    #[test]
    fn occurrences_between_monthly_clamps_across_the_window() {
        let anchor = d(2026, 1, 31);
        let occ = Frequency::Monthly.occurrences_between(Some(anchor), d(2026, 1, 1), d(2026, 6, 30));
        assert_eq!(
            occ,
            vec![d(2026, 1, 31), d(2026, 2, 28), d(2026, 3, 31), d(2026, 4, 30), d(2026, 5, 31), d(2026, 6, 30)]
        );
    }

    #[test]
    fn occurrences_between_yearly() {
        let anchor = d(2024, 2, 29);
        let occ = Frequency::Yearly.occurrences_between(Some(anchor), d(2024, 1, 1), d(2027, 12, 31));
        assert_eq!(occ, vec![d(2024, 2, 29), d(2025, 2, 28), d(2026, 2, 28), d(2027, 2, 28)]);
    }

    #[test]
    fn occurrences_between_empty_when_start_after_end() {
        assert_eq!(Frequency::Monthly.occurrences_between(None, d(2026, 2, 1), d(2026, 1, 1)), Vec::new());
    }

    #[test]
    fn occurrences_between_reference_date_after_window_is_empty() {
        let occ = Frequency::Monthly.occurrences_between(Some(d(2027, 1, 1)), d(2026, 1, 1), d(2026, 12, 31));
        assert!(occ.is_empty());
    }
}
