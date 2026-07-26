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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpendingEntry {
    pub id: String,
    pub date: NaiveDate,
    pub description: String,
    pub category: SpendingCategory,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecurringItem {
    pub id: String,
    pub name: String,
    pub amount: f64,
    pub kind: TxnKind,
    pub label: String,
    pub frequency: Frequency,
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
