//! Parses `~`-prefixed periodic-rule blocks directly out of the journal
//! file's raw text. hledger has no subcommand that lists raw periodic rules
//! (confirmed: `print` without `--forecast` returns nothing for them) so
//! listing/deleting recurring items has to read the file ourselves instead
//! of shelling out. Only ever parses blocks in the exact shape
//! `journal_writer::format_recurring_item` itself writes.

use crate::models::{
    parse_period_phrase, Account, AccountKind, RecurringItem, RecurringTransfer, TxnKind,
};

fn parse_amount(posting_line: &str) -> Option<f64> {
    let dollar_idx = posting_line.find('$')?;
    posting_line[dollar_idx + 1..].trim().parse::<f64>().ok()
}

fn parse_account(posting_line: &str) -> Option<&str> {
    posting_line.split_whitespace().next()
}

fn parse_block(block: &str) -> Option<RecurringItem> {
    let mut lines = block.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next()?.trim();
    let header = header.strip_prefix("~ ")?;
    let (period, comment) = header.split_once("; ")?;
    let period = period.trim();
    let (frequency, reference_date) = parse_period_phrase(period)?;

    let comment = comment.trim().strip_prefix("id:")?;
    let (id, name) = comment.split_once(" name:")?;
    let id = id.trim().to_string();
    let name = name.trim().to_string();

    let posting1 = lines.next()?.trim();
    let posting2 = lines.next()?.trim();

    let account1 = parse_account(posting1)?;
    let account2 = parse_account(posting2)?;
    let amount = parse_amount(posting1).or_else(|| parse_amount(posting2))?;

    // The non-category leg is "the account", whatever kind (asset or
    // liability) it happens to be — which side it's on is determined purely
    // by which leg is expenses:/income:, not by the account's own prefix.
    let (kind, label, account) = if account1.starts_with("expenses:") {
        (TxnKind::Expense, account1.strip_prefix("expenses:")?.to_string(), account2.to_string())
    } else if account2.starts_with("income:") {
        (TxnKind::Income, account2.strip_prefix("income:")?.to_string(), account1.to_string())
    } else {
        return None;
    };

    Some(RecurringItem {
        id,
        name,
        amount,
        kind,
        label,
        frequency,
        reference_date,
        account,
    })
}

pub fn parse_recurring_items(content: &str) -> Vec<RecurringItem> {
    content
        .split("\n\n")
        .filter(|block| block.trim_start().starts_with("~ "))
        .filter_map(parse_block)
        .collect()
}

/// Parses a `~`-prefixed periodic-rule block tagged `transfer:1` into a
/// `RecurringTransfer`. Kept as a separate pass from `parse_block` — that
/// one requires an `expenses:`/`income:` leg and always returns `None` for
/// a transfer block (both legs are real accounts), so there's no risk of
/// the two colliding on the same input.
fn parse_recurring_transfer_block(block: &str) -> Option<RecurringTransfer> {
    let mut lines = block.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next()?.trim().strip_prefix("~ ")?;
    let (period, comment) = header.split_once("; ")?;
    let (frequency, reference_date) = parse_period_phrase(period.trim())?;

    let comment = comment.trim().strip_prefix("id:")?;
    let (id, rest) = comment.split_once(" name:")?;
    let name = rest.strip_suffix(" transfer:1")?.trim().to_string();
    let id = id.trim().to_string();

    let posting1 = lines.next()?.trim();
    let posting2 = lines.next()?.trim();
    let account1 = parse_account(posting1)?;
    let account2 = parse_account(posting2)?;
    let amount = parse_amount(posting1).or_else(|| parse_amount(posting2))?;

    // Whichever posting carries the explicit amount is "to" (it's the one
    // written first, per format_recurring_transfer); the other is "from",
    // left for hledger to auto-balance to the negative.
    let (to_account, from_account) = if parse_amount(posting1).is_some() {
        (account1.to_string(), account2.to_string())
    } else {
        (account2.to_string(), account1.to_string())
    };

    Some(RecurringTransfer {
        id,
        name,
        amount,
        frequency,
        reference_date,
        from_account,
        to_account,
    })
}

/// Parses every `~`-prefixed, `transfer:1`-tagged block — recurring
/// transfers written by `journal_writer::format_recurring_transfer`.
pub fn parse_recurring_transfers(content: &str) -> Vec<RecurringTransfer> {
    content
        .split("\n\n")
        .filter(|block| block.trim_start().starts_with("~ ") && block.contains("transfer:1"))
        .filter_map(parse_recurring_transfer_block)
        .collect()
}

fn parse_account_block(block: &str) -> Option<Account> {
    let line = block.lines().find(|l| !l.trim().is_empty())?.trim();
    let header = line.strip_prefix("account ")?;
    let (path, comment) = header.split_once("; ")?;
    let path = path.trim();
    let (kind_str, slug) = path.split_once(':')?;
    let kind = AccountKind::from_prefix(kind_str)?;

    let comment = comment.trim().strip_prefix("id:")?;
    let (id, name) = comment.split_once(" name:")?;

    Some(Account {
        id: id.trim().to_string(),
        name: name.trim().to_string(),
        kind,
        slug: slug.trim().to_string(),
        balance: 0.0,
    })
}

/// Parses `account <path>  ; id:<id> name:<name>` directive lines —
/// registrations written by `journal_writer::format_account_directive`.
pub fn parse_accounts(content: &str) -> Vec<Account> {
    content
        .split("\n\n")
        .filter(|block| block.trim_start().starts_with("account "))
        .filter_map(parse_account_block)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal_writer::{format_account_directive, format_recurring_item};
    use crate::models::Frequency;
    use chrono::NaiveDate;

    #[test]
    fn round_trips_expense_item() {
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
        let items = parse_recurring_items(&text);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.id, "rec-1");
        assert_eq!(item.name, "Netflix");
        assert_eq!(item.amount, 15.0);
        assert_eq!(item.kind, TxnKind::Expense);
        assert_eq!(item.label, "netflix");
        assert_eq!(item.frequency, Frequency::Monthly);
        assert_eq!(item.reference_date, None);
        assert_eq!(item.account, "assets:checking");
    }

    #[test]
    fn round_trips_income_item() {
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
        let items = parse_recurring_items(&text);
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.kind, TxnKind::Income);
        assert_eq!(item.label, "salary");
        assert_eq!(item.frequency, Frequency::Biweekly);
        assert_eq!(item.amount, 2000.0);
    }

    #[test]
    fn round_trips_reference_date() {
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
        let items = parse_recurring_items(&text);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].reference_date, Some(reference_date));
    }

    #[test]
    fn round_trips_liability_account() {
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
        let items = parse_recurring_items(&text);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].account, "liabilities:visa");
    }

    #[test]
    fn ignores_plain_transactions() {
        let content = "2026-07-05 Junk food  ; id:abc\n    expenses:stupid    $12.50\n    assets:checking\n\n";
        assert_eq!(parse_recurring_items(content).len(), 0);
    }

    #[test]
    fn parses_multiple_blocks_mixed_with_transactions() {
        let mut content = String::new();
        content.push_str("2026-07-01 Salary  ; id:t1\n    assets:checking    $2000.00\n    income:salary\n\n");
        content.push_str(&format_recurring_item("rec-1", "Netflix", 15.0, TxnKind::Expense, "netflix", Frequency::Monthly, None, "assets:checking"));
        content.push_str(&format_recurring_item("rec-2", "Taxes", 500.0, TxnKind::Expense, "taxes", Frequency::Yearly, None, "assets:checking"));
        let items = parse_recurring_items(&content);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "rec-1");
        assert_eq!(items[1].id, "rec-2");
    }

    #[test]
    fn round_trips_recurring_transfer() {
        let text = crate::journal_writer::format_recurring_transfer(
            "rtr-1",
            "Auto-save",
            100.0,
            Frequency::Monthly,
            None,
            "assets:checking",
            "assets:savings",
        );
        let transfers = parse_recurring_transfers(&text);
        assert_eq!(transfers.len(), 1);
        let t = &transfers[0];
        assert_eq!(t.id, "rtr-1");
        assert_eq!(t.name, "Auto-save");
        assert_eq!(t.amount, 100.0);
        assert_eq!(t.frequency, Frequency::Monthly);
        assert_eq!(t.from_account, "assets:checking");
        assert_eq!(t.to_account, "assets:savings");
    }

    #[test]
    fn recurring_transfer_not_parsed_as_regular_recurring_item() {
        let text = crate::journal_writer::format_recurring_transfer(
            "rtr-2", "Auto-save", 100.0, Frequency::Monthly, None,
            "assets:checking", "assets:savings",
        );
        assert_eq!(parse_recurring_items(&text).len(), 0);
    }

    #[test]
    fn regular_recurring_item_not_parsed_as_transfer() {
        let text = format_recurring_item(
            "rec-x", "Netflix", 15.0, TxnKind::Expense, "netflix",
            Frequency::Monthly, None, "assets:checking",
        );
        assert_eq!(parse_recurring_transfers(&text).len(), 0);
    }

    #[test]
    fn round_trips_account_directive() {
        let text = format_account_directive("acc-1", "Checking", AccountKind::Asset, "checking");
        let accounts = parse_accounts(&text);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, "acc-1");
        assert_eq!(accounts[0].name, "Checking");
        assert_eq!(accounts[0].kind, AccountKind::Asset);
        assert_eq!(accounts[0].slug, "checking");
    }

    #[test]
    fn round_trips_liability_account_directive() {
        let text = format_account_directive("acc-2", "Visa", AccountKind::Liability, "visa");
        let accounts = parse_accounts(&text);
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].kind, AccountKind::Liability);
    }

    #[test]
    fn ignores_non_account_blocks_when_parsing_accounts() {
        let content = "2026-07-05 Junk food  ; id:abc\n    expenses:stupid    $12.50\n    assets:checking\n\n";
        assert_eq!(parse_accounts(content).len(), 0);
    }
}
