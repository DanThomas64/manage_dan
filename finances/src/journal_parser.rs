//! Parses `~`-prefixed periodic-rule blocks directly out of the journal
//! file's raw text. hledger has no subcommand that lists raw periodic rules
//! (confirmed: `print` without `--forecast` returns nothing for them) so
//! listing/deleting recurring items has to read the file ourselves instead
//! of shelling out. Only ever parses blocks in the exact shape
//! `journal_writer::format_recurring_item` itself writes.

use crate::models::{Frequency, RecurringItem, TxnKind};

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
    let frequency = Frequency::from_period_phrase(period)?;

    let comment = comment.trim().strip_prefix("id:")?;
    let (id, name) = comment.split_once(" name:")?;
    let id = id.trim().to_string();
    let name = name.trim().to_string();

    let posting1 = lines.next()?.trim();
    let posting2 = lines.next()?.trim();

    let account1 = parse_account(posting1)?;
    let account2 = parse_account(posting2)?;
    let amount = parse_amount(posting1).or_else(|| parse_amount(posting2))?;

    let (kind, label) = if account1.starts_with("assets:") {
        (TxnKind::Income, account2.strip_prefix("income:")?.to_string())
    } else if account1.starts_with("expenses:") {
        (TxnKind::Expense, account1.strip_prefix("expenses:")?.to_string())
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
    })
}

pub fn parse_recurring_items(content: &str) -> Vec<RecurringItem> {
    content
        .split("\n\n")
        .filter(|block| block.trim_start().starts_with("~ "))
        .filter_map(parse_block)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal_writer::format_recurring_item;

    #[test]
    fn round_trips_expense_item() {
        let text = format_recurring_item(
            "rec-1",
            "Netflix",
            15.0,
            TxnKind::Expense,
            "netflix",
            Frequency::Monthly,
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
    fn ignores_plain_transactions() {
        let content = "2026-07-05 Junk food  ; id:abc\n    expenses:stupid    $12.50\n    assets:checking\n\n";
        assert_eq!(parse_recurring_items(content).len(), 0);
    }

    #[test]
    fn parses_multiple_blocks_mixed_with_transactions() {
        let mut content = String::new();
        content.push_str("2026-07-01 Salary  ; id:t1\n    assets:checking    $2000.00\n    income:salary\n\n");
        content.push_str(&format_recurring_item("rec-1", "Netflix", 15.0, TxnKind::Expense, "netflix", Frequency::Monthly));
        content.push_str(&format_recurring_item("rec-2", "Taxes", 500.0, TxnKind::Expense, "taxes", Frequency::Yearly));
        let items = parse_recurring_items(&content);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "rec-1");
        assert_eq!(items[1].id, "rec-2");
    }
}
