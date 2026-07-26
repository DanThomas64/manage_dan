use chrono::NaiveDate;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionPoint {
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub balance: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CategoryTotals {
    pub stupid: f64,
    pub survival: f64,
}
