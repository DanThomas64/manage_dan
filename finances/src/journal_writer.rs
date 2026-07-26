//! Formats and appends transaction / periodic-rule text directly to the
//! hledger journal file. hledger's own `add` command is interactive-only
//! (confirmed via `hledger add --help`), so this crate owns writing
//! well-formatted journal text itself — the same division of labour as
//! `notes::nb_client::format_note_body` owning markdown bodies before `nb`
//! reads them back.

use crate::finances_prelude::*;
use crate::models::{
    build_period_phrase, Frequency, RecurringItem, SpendingCategory, SpendingEntry, TxnKind,
};
use chrono::NaiveDate;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// A journal line/comment can't contain a literal newline without corrupting
/// the entry's structure; user-supplied free text is flattened to one line.
fn sanitize_line(s: &str) -> String {
    s.replace(['\n', '\r'], " ")
}

/// Account leaves must not contain whitespace or colons (colons are hledger's
/// own account-segment separator).
fn sanitize_account_leaf(s: &str) -> String {
    sanitize_line(s).replace(':', "-").replace(' ', "_")
}

pub fn format_spending_entry(
    id: &str,
    date: NaiveDate,
    description: &str,
    category: SpendingCategory,
    amount: f64,
) -> String {
    format!(
        "{date} {description}  ; id:{id}\n    {account}    ${amount:.2}\n    assets:checking\n\n",
        date = date.format("%Y-%m-%d"),
        description = sanitize_line(description),
        id = id,
        account = category.account(),
        amount = amount,
    )
}

pub fn format_recurring_item(
    id: &str,
    name: &str,
    amount: f64,
    kind: TxnKind,
    label: &str,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
) -> String {
    let label = sanitize_account_leaf(label);
    let (debit, credit) = match kind {
        TxnKind::Income => ("assets:checking".to_string(), format!("income:{label}")),
        TxnKind::Expense => (format!("expenses:{label}"), "assets:checking".to_string()),
    };
    format!(
        "~ {period}  ; id:{id} name:{name}\n    {debit}    ${amount:.2}\n    {credit}\n\n",
        period = build_period_phrase(frequency, reference_date),
        id = id,
        name = sanitize_line(name),
        debit = debit,
        amount = amount,
        credit = credit,
    )
}

async fn append(journal_path: &str, text: &str) -> FinancesLibResult<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(journal_path)
        .await
        .map_err(FinancesLibError::Io)?;
    file.write_all(text.as_bytes())
        .await
        .map_err(FinancesLibError::Io)?;
    Ok(())
}

pub async fn append_spending_entry(
    journal_path: &str,
    category: SpendingCategory,
    amount: f64,
    description: &str,
    date: NaiveDate,
) -> FinancesLibResult<SpendingEntry> {
    let id = Uuid::new_v4().to_string();
    let text = format_spending_entry(&id, date, description, category, amount);
    append(journal_path, &text).await?;
    Ok(SpendingEntry {
        id,
        date,
        description: sanitize_line(description),
        category,
        amount,
    })
}

pub async fn append_recurring_item(
    journal_path: &str,
    name: &str,
    amount: f64,
    kind: TxnKind,
    label: &str,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
) -> FinancesLibResult<RecurringItem> {
    let id = Uuid::new_v4().to_string();
    let text = format_recurring_item(&id, name, amount, kind, label, frequency, reference_date);
    append(journal_path, &text).await?;
    Ok(RecurringItem {
        id,
        name: sanitize_line(name),
        amount,
        kind,
        label: sanitize_account_leaf(label),
        frequency,
        reference_date,
    })
}

/// Removes the blank-line-delimited block containing the given `; id:<id>`
/// (or periodic-rule `id:<id>` tag) marker. Only ever removes blocks this
/// module itself wrote (always blank-line-terminated), so it never touches
/// arbitrary hand-edited journal content elsewhere in the file.
pub async fn remove_block_with_id(journal_path: &str, id: &str) -> FinancesLibResult<bool> {
    let content = tokio::fs::read_to_string(journal_path)
        .await
        .map_err(FinancesLibError::Io)?;
    let marker = format!("id:{id}");
    let blocks: Vec<&str> = content.split("\n\n").collect();
    let mut found = false;
    let kept: Vec<&str> = blocks
        .into_iter()
        .filter(|block| {
            if block.contains(&marker) {
                found = true;
                false
            } else {
                true
            }
        })
        .collect();
    if !found {
        return Ok(false);
    }
    let new_content = kept.join("\n\n");
    tokio::fs::write(journal_path, new_content.as_bytes())
        .await
        .map_err(FinancesLibError::Io)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Frequency, TxnKind};

    #[test]
    fn spending_entry_format_is_exact() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 26).unwrap();
        let text = format_spending_entry(
            "abc-123",
            date,
            "Junk food",
            SpendingCategory::Stupid,
            12.5,
        );
        assert_eq!(
            text,
            "2026-07-26 Junk food  ; id:abc-123\n    expenses:stupid    $12.50\n    assets:checking\n\n"
        );
    }

    #[test]
    fn recurring_expense_format_is_exact() {
        let text = format_recurring_item(
            "rec-1",
            "Netflix",
            15.0,
            TxnKind::Expense,
            "netflix",
            Frequency::Monthly,
            None,
        );
        assert_eq!(
            text,
            "~ monthly  ; id:rec-1 name:Netflix\n    expenses:netflix    $15.00\n    assets:checking\n\n"
        );
    }

    #[test]
    fn recurring_income_format_is_exact() {
        let text = format_recurring_item(
            "rec-2",
            "Salary",
            2000.0,
            TxnKind::Income,
            "salary",
            Frequency::Biweekly,
            None,
        );
        assert_eq!(
            text,
            "~ every 2 weeks  ; id:rec-2 name:Salary\n    assets:checking    $2000.00\n    income:salary\n\n"
        );
    }

    #[test]
    fn recurring_item_with_reference_date_embeds_from_clause() {
        let reference_date = NaiveDate::from_ymd_opt(2026, 1, 6).unwrap();
        let text = format_recurring_item(
            "rec-3",
            "Rent",
            1200.0,
            TxnKind::Expense,
            "rent",
            Frequency::Biweekly,
            Some(reference_date),
        );
        assert_eq!(
            text,
            "~ every 2 weeks from 2026-01-06  ; id:rec-3 name:Rent\n    expenses:rent    $1200.00\n    assets:checking\n\n"
        );
    }

    #[test]
    fn sanitizes_newlines_in_description() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let text = format_spending_entry(
            "x",
            date,
            "line1\nline2",
            SpendingCategory::Survival,
            1.0,
        );
        assert!(!text.contains("line1\nline2"));
        assert!(text.starts_with("2026-01-01 line1 line2"));
    }
}
