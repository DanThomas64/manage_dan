//! Formats and appends transaction / periodic-rule text directly to the
//! hledger journal file. hledger's own `add` command is interactive-only
//! (confirmed via `hledger add --help`), so this crate owns writing
//! well-formatted journal text itself — the same division of labour as
//! `notes::nb_client::format_note_body` owning markdown bodies before `nb`
//! reads them back.

use crate::finances_prelude::*;
use crate::models::{
    build_period_phrase, Account, AccountKind, Frequency, RecurringItem, SpendingCategory,
    SpendingEntry, TxnKind,
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
    account: &str,
) -> String {
    format!(
        "{date} {description}  ; id:{id}\n    {category_account}    ${amount:.2}\n    {account}\n\n",
        date = date.format("%Y-%m-%d"),
        description = sanitize_line(description),
        id = id,
        category_account = category.account(),
        amount = amount,
        account = account,
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
    account: &str,
) -> String {
    let label = sanitize_account_leaf(label);
    let (debit, credit) = match kind {
        TxnKind::Income => (account.to_string(), format!("income:{label}")),
        TxnKind::Expense => (format!("expenses:{label}"), account.to_string()),
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

/// The `account` directive: registers a named account so it shows up
/// (with a balance of $0) even before any transaction references it.
/// hledger itself never needs this to *use* an account, but it's how this
/// crate lets a user create one ahead of time to appear in dropdowns.
pub fn format_account_directive(id: &str, name: &str, kind: AccountKind, slug: &str) -> String {
    format!(
        "account {prefix}:{slug}  ; id:{id} name:{name}\n\n",
        prefix = kind.prefix(),
        slug = slug,
        id = id,
        name = sanitize_line(name),
    )
}

/// A reconciliation transaction that nudges an account's hledger-computed
/// balance to a user-entered actual balance, via an offsetting posting to
/// `equity:adjustments` — the standard hledger idiom for "setting" a
/// balance without breaking double-entry bookkeeping.
pub fn format_adjustment_transaction(id: &str, date: NaiveDate, account: &str, diff: f64) -> String {
    format!(
        "{date} Balance adjustment  ; id:{id}\n    {account}    ${diff:.2}\n    equity:adjustments\n\n",
        date = date.format("%Y-%m-%d"),
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
    account: &str,
) -> FinancesLibResult<SpendingEntry> {
    let id = Uuid::new_v4().to_string();
    let text = format_spending_entry(&id, date, description, category, amount, account);
    append(journal_path, &text).await?;
    Ok(SpendingEntry {
        id,
        date,
        description: sanitize_line(description),
        category,
        amount,
        account: account.to_string(),
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
    account: &str,
) -> FinancesLibResult<RecurringItem> {
    let id = Uuid::new_v4().to_string();
    let text = format_recurring_item(
        &id,
        name,
        amount,
        kind,
        label,
        frequency,
        reference_date,
        account,
    );
    append(journal_path, &text).await?;
    Ok(RecurringItem {
        id,
        name: sanitize_line(name),
        amount,
        kind,
        label: sanitize_account_leaf(label),
        frequency,
        reference_date,
        account: account.to_string(),
    })
}

pub async fn append_account(
    journal_path: &str,
    name: &str,
    kind: AccountKind,
) -> FinancesLibResult<Account> {
    let id = Uuid::new_v4().to_string();
    let slug = sanitize_account_leaf(name).to_lowercase();
    let text = format_account_directive(&id, name, kind, &slug);
    append(journal_path, &text).await?;
    Ok(Account {
        id,
        name: sanitize_line(name),
        kind,
        slug,
        balance: 0.0,
    })
}

pub async fn append_adjustment_transaction(
    journal_path: &str,
    account: &str,
    diff: f64,
    date: NaiveDate,
) -> FinancesLibResult<()> {
    let id = Uuid::new_v4().to_string();
    let text = format_adjustment_transaction(&id, date, account, diff);
    append(journal_path, &text).await
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
            "assets:checking",
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
            "assets:checking",
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
            "assets:checking",
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
            "assets:checking",
        );
        assert_eq!(
            text,
            "~ every 2 weeks from 2026-01-06  ; id:rec-3 name:Rent\n    expenses:rent    $1200.00\n    assets:checking\n\n"
        );
    }

    #[test]
    fn recurring_item_uses_selected_account() {
        let text = format_recurring_item(
            "rec-4",
            "Visa payment",
            50.0,
            TxnKind::Expense,
            "visa-payment",
            Frequency::Monthly,
            None,
            "liabilities:visa",
        );
        assert_eq!(
            text,
            "~ monthly  ; id:rec-4 name:Visa payment\n    expenses:visa-payment    $50.00\n    liabilities:visa\n\n"
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
            "assets:checking",
        );
        assert!(!text.contains("line1\nline2"));
        assert!(text.starts_with("2026-01-01 line1 line2"));
    }

    #[test]
    fn account_directive_format_is_exact() {
        let text = format_account_directive("acc-1", "Checking", AccountKind::Asset, "checking");
        assert_eq!(text, "account assets:checking  ; id:acc-1 name:Checking\n\n");
    }

    #[test]
    fn liability_account_directive_format_is_exact() {
        let text = format_account_directive("acc-2", "Visa", AccountKind::Liability, "visa");
        assert_eq!(text, "account liabilities:visa  ; id:acc-2 name:Visa\n\n");
    }

    #[test]
    fn adjustment_transaction_format_is_exact() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 26).unwrap();
        let text = format_adjustment_transaction("adj-1", date, "assets:checking", -42.5);
        assert_eq!(
            text,
            "2026-07-26 Balance adjustment  ; id:adj-1\n    assets:checking    $-42.50\n    equity:adjustments\n\n"
        );
    }
}
