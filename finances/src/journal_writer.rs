//! Formats and appends transaction / periodic-rule text directly to the
//! hledger journal file. hledger's own `add` command is interactive-only
//! (confirmed via `hledger add --help`), so this crate owns writing
//! well-formatted journal text itself — the same division of labour as
//! `notes::nb_client::format_note_body` owning markdown bodies before `nb`
//! reads them back.

use crate::finances_prelude::*;
use crate::models::{
    build_period_phrase, Account, AccountKind, Frequency, RecurringItem, RecurringTransfer,
    SpendingCategory, SpendingEntry, TransferEntry, TxnKind,
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

/// When `category` is `Some`, a ` category:{tag}` tag is appended *last*
/// after `name:` — same trailing-tag idiom as the account directive's
/// `rate:`/`limit:` tags (see `format_account_directive`); the parser
/// anchors on the last occurrence of `" category:"` so it isolates the tag
/// correctly regardless of what `name` itself contains, as long as nothing
/// is ever appended after it. This is purely bookkeeping metadata — it
/// does not change which ledger account the item's expense/income leg
/// posts against (`label`, unaffected).
pub fn format_recurring_item(
    id: &str,
    name: &str,
    amount: f64,
    kind: TxnKind,
    label: &str,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
    account: &str,
    category: Option<SpendingCategory>,
) -> String {
    let label = sanitize_account_leaf(label);
    let (debit, credit) = match kind {
        TxnKind::Income => (account.to_string(), format!("income:{label}")),
        TxnKind::Expense => (format!("expenses:{label}"), account.to_string()),
    };
    let mut header = format!(
        "~ {period}  ; id:{id} name:{name}",
        period = build_period_phrase(frequency, reference_date),
        id = id,
        name = sanitize_line(name),
    );
    if let Some(cat) = category {
        header.push_str(&format!(" category:{}", cat.tag()));
    }
    format!("{header}\n    {debit}    ${amount:.2}\n    {credit}\n\n")
}

/// The `account` directive: registers a named account so it shows up
/// (with a balance of $0) even before any transaction references it.
/// hledger itself never needs this to *use* an account, but it's how this
/// crate lets a user create one ahead of time to appear in dropdowns.
///
/// When `interest_rate`/`credit_limit` are `Some`, ` rate:{rate}`/
/// ` limit:{limit}` tags are appended *after* `name:`, in that fixed order
/// — deliberately last, since `name` is arbitrary free text that could
/// itself contain either substring; the parser anchors on the *last*
/// occurrence of `" limit:"` first (stripping it off if present), then the
/// last occurrence of `" rate:"` on what remains, which only isolates the
/// real tags correctly as long as the writer never appends anything after
/// `limit` or between `rate` and `limit`. With both `None`, output is
/// byte-identical to before either field existed.
pub fn format_account_directive(
    id: &str,
    name: &str,
    kind: AccountKind,
    slug: &str,
    interest_rate: Option<f64>,
    credit_limit: Option<f64>,
) -> String {
    let name = sanitize_line(name);
    let mut header = format!("account {}:{}  ; id:{} name:{}", kind.prefix(), slug, id, name);
    if let Some(rate) = interest_rate {
        header.push_str(&format!(" rate:{rate}"));
    }
    if let Some(limit) = credit_limit {
        header.push_str(&format!(" limit:{limit}"));
    }
    header.push_str("\n\n");
    header
}

/// A one-off transfer between two of the user's own accounts. Tagged
/// `transfer:1` in a *comma*-separated comment (not space-separated, like
/// `id:`/`name:` elsewhere in this module) — hledger's own tag parser only
/// splits comment tags on commas, so `id:{id} transfer:1` would parse as one
/// `id` tag whose value is the literal string `"{id} transfer:1"` rather
/// than two separate tags (confirmed empirically against a real `hledger`
/// install) — and it's this tag, via `tag:transfer`, that lets
/// `list_transfer_entries` find these back out of real hledger transactions
/// (unlike periodic rules, which this crate parses from local file text
/// instead of querying hledger for). `to_account` gets the explicit
/// `$amount`; `from_account`'s is left for hledger to auto-balance to
/// `-amount` — the same one-posting-explicit convention every other
/// two-line entry in this module already uses.
pub fn format_transfer_entry(
    id: &str,
    date: NaiveDate,
    description: &str,
    amount: f64,
    from_account: &str,
    to_account: &str,
) -> String {
    format!(
        "{date} {description}  ; id:{id}, transfer:1\n    {to_account}    ${amount:.2}\n    {from_account}\n\n",
        date = date.format("%Y-%m-%d"),
        description = sanitize_line(description),
        id = id,
        to_account = to_account,
        amount = amount,
        from_account = from_account,
    )
}

/// The periodic-rule equivalent of `format_transfer_entry` — parsed back
/// locally (like `format_recurring_item`), so the `id:`/`name:` tags stay
/// space-separated for consistency with that convention rather than needing
/// hledger's own comma-separated tag syntax.
pub fn format_recurring_transfer(
    id: &str,
    name: &str,
    amount: f64,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
    from_account: &str,
    to_account: &str,
) -> String {
    format!(
        "~ {period}  ; id:{id} name:{name} transfer:1\n    {to_account}    ${amount:.2}\n    {from_account}\n\n",
        period = build_period_phrase(frequency, reference_date),
        id = id,
        name = sanitize_line(name),
        to_account = to_account,
        amount = amount,
        from_account = from_account,
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

#[allow(clippy::too_many_arguments)]
pub async fn append_recurring_item(
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
        category,
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
        category,
    })
}

pub async fn append_transfer_entry(
    journal_path: &str,
    description: &str,
    amount: f64,
    date: NaiveDate,
    from_account: &str,
    to_account: &str,
) -> FinancesLibResult<TransferEntry> {
    let id = Uuid::new_v4().to_string();
    let text = format_transfer_entry(&id, date, description, amount, from_account, to_account);
    append(journal_path, &text).await?;
    Ok(TransferEntry {
        id,
        date,
        description: sanitize_line(description),
        amount,
        from_account: from_account.to_string(),
        to_account: to_account.to_string(),
    })
}

pub async fn append_recurring_transfer(
    journal_path: &str,
    name: &str,
    amount: f64,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
    from_account: &str,
    to_account: &str,
) -> FinancesLibResult<RecurringTransfer> {
    let id = Uuid::new_v4().to_string();
    let text = format_recurring_transfer(
        &id,
        name,
        amount,
        frequency,
        reference_date,
        from_account,
        to_account,
    );
    append(journal_path, &text).await?;
    Ok(RecurringTransfer {
        id,
        name: sanitize_line(name),
        amount,
        frequency,
        reference_date,
        from_account: from_account.to_string(),
        to_account: to_account.to_string(),
    })
}

pub async fn append_account(
    journal_path: &str,
    name: &str,
    kind: AccountKind,
    interest_rate: Option<f64>,
    credit_limit: Option<f64>,
) -> FinancesLibResult<Account> {
    let id = Uuid::new_v4().to_string();
    let slug = sanitize_account_leaf(name).to_lowercase();
    let text = format_account_directive(&id, name, kind, &slug, interest_rate, credit_limit);
    append(journal_path, &text).await?;
    Ok(Account {
        id,
        name: sanitize_line(name),
        kind,
        slug,
        balance: 0.0,
        interest_rate,
        credit_limit,
    })
}

/// Rewrites an account's `name`/`interest_rate`/`credit_limit` in place
/// (delete + re-append with the same id/slug/kind, same idiom as
/// `rewrite_recurring_item`). `slug`/`kind` are never user-editable here —
/// `slug` is the permanent hledger-account identity once transactions may
/// reference it.
pub async fn rewrite_account(
    journal_path: &str,
    id: &str,
    name: &str,
    kind: AccountKind,
    slug: &str,
    interest_rate: Option<f64>,
    credit_limit: Option<f64>,
) -> FinancesLibResult<Option<Account>> {
    let removed = remove_block_with_id(journal_path, id).await?;
    if !removed {
        return Ok(None);
    }
    let text = format_account_directive(id, name, kind, slug, interest_rate, credit_limit);
    append(journal_path, &text).await?;
    Ok(Some(Account {
        id: id.to_string(),
        name: sanitize_line(name),
        kind,
        slug: slug.to_string(),
        balance: 0.0,
        interest_rate,
        credit_limit,
    }))
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

/// Same delete + re-append-with-same-id idiom as the `rewrite_*` functions
/// below, applied to a one-off spending entry. Returns `Ok(false)` if no
/// block with that id existed to rewrite.
pub async fn rewrite_spending_entry(
    journal_path: &str,
    id: &str,
    date: NaiveDate,
    description: &str,
    category: SpendingCategory,
    amount: f64,
    account: &str,
) -> FinancesLibResult<Option<SpendingEntry>> {
    let removed = remove_block_with_id(journal_path, id).await?;
    if !removed {
        return Ok(None);
    }
    let text = format_spending_entry(id, date, description, category, amount, account);
    append(journal_path, &text).await?;
    Ok(Some(SpendingEntry {
        id: id.to_string(),
        date,
        description: sanitize_line(description),
        category,
        amount,
        account: account.to_string(),
    }))
}

/// Editing a periodic rule in place isn't a thing hledger's own file format
/// supports — there's no "update a block" primitive, only whole-block
/// removal (`remove_block_with_id`). Every `rewrite_*` function below
/// therefore edits by deleting the old `id:<id>` block and re-appending a
/// freshly formatted one that reuses the same id, so anything that already
/// referenced that id (e.g. `preview_projection`'s `exclude_recurring_ids`)
/// keeps working. Where in the file the new block lands doesn't matter:
/// every reader here (`parse_recurring_items`, `parse_recurring_transfers`,
/// `parse_accounts`) scans the whole file for matching blocks regardless of
/// position, same as already documented on `remove_block_with_id` itself.
/// Returns `Ok(false)` if no block with that id existed to rewrite.
#[allow(clippy::too_many_arguments)]
pub async fn rewrite_recurring_item(
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
) -> FinancesLibResult<Option<RecurringItem>> {
    let removed = remove_block_with_id(journal_path, id).await?;
    if !removed {
        return Ok(None);
    }
    let text = format_recurring_item(id, name, amount, kind, label, frequency, reference_date, account, category);
    append(journal_path, &text).await?;
    Ok(Some(RecurringItem {
        id: id.to_string(),
        name: sanitize_line(name),
        amount,
        kind,
        label: sanitize_account_leaf(label),
        frequency,
        reference_date,
        account: account.to_string(),
        category,
    }))
}

#[allow(clippy::too_many_arguments)]
pub async fn rewrite_recurring_transfer(
    journal_path: &str,
    id: &str,
    name: &str,
    amount: f64,
    frequency: Frequency,
    reference_date: Option<NaiveDate>,
    from_account: &str,
    to_account: &str,
) -> FinancesLibResult<Option<RecurringTransfer>> {
    let removed = remove_block_with_id(journal_path, id).await?;
    if !removed {
        return Ok(None);
    }
    let text = format_recurring_transfer(id, name, amount, frequency, reference_date, from_account, to_account);
    append(journal_path, &text).await?;
    Ok(Some(RecurringTransfer {
        id: id.to_string(),
        name: sanitize_line(name),
        amount,
        frequency,
        reference_date,
        from_account: from_account.to_string(),
        to_account: to_account.to_string(),
    }))
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

    #[tokio::test]
    async fn rewrite_spending_entry_replaces_existing_block_same_id() {
        let dir = std::env::temp_dir().join(format!("fin-test-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();
        let date = NaiveDate::from_ymd_opt(2026, 7, 26).unwrap();

        let text = format_spending_entry("sp-1", date, "Junk food", SpendingCategory::Stupid, 12.5, "assets:checking");
        append(&path, &text).await.unwrap();

        let new_date = NaiveDate::from_ymd_opt(2026, 7, 27).unwrap();
        let result = rewrite_spending_entry(
            &path, "sp-1", new_date, "Groceries", SpendingCategory::Survival, 60.0, "assets:checking",
        ).await.unwrap();
        assert!(result.is_some());

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content.matches("id:sp-1").count(), 1);
        assert!(content.contains("Groceries"));
        assert!(content.contains("expenses:survival"));
        assert!(content.contains("$60.00"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn rewrite_spending_entry_returns_none_when_missing() {
        let dir = std::env::temp_dir().join(format!("fin-test-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();
        tokio::fs::write(&path, "").await.unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 7, 26).unwrap();

        let result = rewrite_spending_entry(
            &path, "missing", date, "X", SpendingCategory::Stupid, 1.0, "assets:checking",
        ).await.unwrap();
        assert!(result.is_none());

        let _ = tokio::fs::remove_dir_all(&dir).await;
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
            None,
        );
        assert_eq!(
            text,
            "~ monthly  ; id:rec-1 name:Netflix\n    expenses:netflix    $15.00\n    assets:checking\n\n"
        );
    }

    #[test]
    fn recurring_expense_with_category_format_is_exact() {
        let text = format_recurring_item(
            "rec-1b",
            "Netflix",
            15.0,
            TxnKind::Expense,
            "netflix",
            Frequency::Monthly,
            None,
            "assets:checking",
            Some(SpendingCategory::Stupid),
        );
        assert_eq!(
            text,
            "~ monthly  ; id:rec-1b name:Netflix category:stupid\n    expenses:netflix    $15.00\n    assets:checking\n\n"
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
            "assets:checking",
            None,
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
            None,
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
        let text = format_account_directive("acc-1", "Checking", AccountKind::Asset, "checking", None, None);
        assert_eq!(text, "account assets:checking  ; id:acc-1 name:Checking\n\n");
    }

    #[test]
    fn liability_account_directive_format_is_exact() {
        let text = format_account_directive("acc-2", "Visa", AccountKind::Liability, "visa", None, None);
        assert_eq!(text, "account liabilities:visa  ; id:acc-2 name:Visa\n\n");
    }

    #[test]
    fn account_directive_with_rate_format_is_exact() {
        let text = format_account_directive("acc-3", "Visa", AccountKind::Liability, "visa", Some(24.99), None);
        assert_eq!(text, "account liabilities:visa  ; id:acc-3 name:Visa rate:24.99\n\n");
    }

    #[test]
    fn account_directive_with_rate_and_limit_format_is_exact() {
        let text = format_account_directive("acc-4", "Visa", AccountKind::Liability, "visa", Some(24.99), Some(1000.0));
        assert_eq!(text, "account liabilities:visa  ; id:acc-4 name:Visa rate:24.99 limit:1000\n\n");
    }

    #[test]
    fn account_directive_with_limit_only_format_is_exact() {
        let text = format_account_directive("acc-5", "Visa", AccountKind::Liability, "visa", None, Some(500.0));
        assert_eq!(text, "account liabilities:visa  ; id:acc-5 name:Visa limit:500\n\n");
    }

    #[tokio::test]
    async fn rewrite_account_replaces_existing_block_same_id() {
        let dir = std::env::temp_dir().join(format!("fin-test-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        let text = format_account_directive("acc-1", "Visa", AccountKind::Liability, "visa", None, None);
        append(&path, &text).await.unwrap();

        let result = rewrite_account(&path, "acc-1", "Visa Signature", AccountKind::Liability, "visa", Some(19.99), Some(1500.0)).await.unwrap();
        assert!(result.is_some());

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content.matches("id:acc-1").count(), 1);
        assert!(content.contains("Visa Signature"));
        assert!(content.contains("rate:19.99"));
        assert!(content.contains("limit:1500"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn rewrite_account_returns_none_when_missing() {
        let dir = std::env::temp_dir().join(format!("fin-test-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();
        tokio::fs::write(&path, "").await.unwrap();

        let result = rewrite_account(&path, "missing", "X", AccountKind::Asset, "x", None, None).await.unwrap();
        assert!(result.is_none());

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn transfer_entry_format_is_exact() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 26).unwrap();
        let text = format_transfer_entry(
            "tr-1",
            date,
            "Move to savings",
            300.0,
            "assets:checking",
            "assets:savings",
        );
        assert_eq!(
            text,
            "2026-07-26 Move to savings  ; id:tr-1, transfer:1\n    assets:savings    $300.00\n    assets:checking\n\n"
        );
    }

    #[test]
    fn recurring_transfer_format_is_exact() {
        let text = format_recurring_transfer(
            "rtr-1",
            "Auto-save",
            100.0,
            Frequency::Monthly,
            None,
            "assets:checking",
            "assets:savings",
        );
        assert_eq!(
            text,
            "~ monthly  ; id:rtr-1 name:Auto-save transfer:1\n    assets:savings    $100.00\n    assets:checking\n\n"
        );
    }

    #[tokio::test]
    async fn rewrite_recurring_item_replaces_existing_block_same_id() {
        let dir = std::env::temp_dir().join(format!("fin-test-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        let text = format_recurring_item("rec-1", "Netflix", 15.0, TxnKind::Expense, "netflix", Frequency::Monthly, None, "assets:checking", None);
        append(&path, &text).await.unwrap();

        let result = rewrite_recurring_item(&path, "rec-1", "Netflix Premium", 22.99, TxnKind::Expense, "netflix", Frequency::Monthly, None, "assets:checking", Some(SpendingCategory::Stupid)).await.unwrap();
        assert!(result.is_some());

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content.matches("id:rec-1").count(), 1);
        assert!(content.contains("Netflix Premium"));
        assert!(content.contains("$22.99"));
        assert!(content.contains("category:stupid"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn rewrite_recurring_item_returns_false_when_missing() {
        let dir = std::env::temp_dir().join(format!("fin-test-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();
        tokio::fs::write(&path, "").await.unwrap();

        let result = rewrite_recurring_item(&path, "missing", "X", 1.0, TxnKind::Expense, "x", Frequency::Monthly, None, "assets:checking", None).await.unwrap();
        assert!(result.is_none());

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn rewrite_recurring_transfer_replaces_existing_block_same_id() {
        let dir = std::env::temp_dir().join(format!("fin-test-{}", Uuid::new_v4()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let path = dir.join("journal").to_str().unwrap().to_string();

        let text = format_recurring_transfer("rtr-1", "Auto-save", 100.0, Frequency::Monthly, None, "assets:checking", "assets:savings");
        append(&path, &text).await.unwrap();

        let result = rewrite_recurring_transfer(&path, "rtr-1", "Auto-save Plus", 150.0, Frequency::Monthly, None, "assets:checking", "assets:savings").await.unwrap();
        assert!(result.is_some());

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content.matches("id:rtr-1").count(), 1);
        assert!(content.contains("Auto-save Plus"));
        assert!(content.contains("$150.00"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
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
