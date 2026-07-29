//! Bridges `db`'s budget scenario/cap storage (local metadata — never
//! posted to the real hledger journal) with `finances`' real projection
//! computation — the same role `finances_occurrences.rs` plays for
//! recurring-item paid tracking, and for the same reason: `finances` stays
//! hledger-only, `db` holds everything that isn't real ledger data, and
//! `app` (already depending on both) is where the two get joined.

use chrono::NaiveDate;
use finances::models::{Frequency, PreviewItem, TxnKind};
use uuid::Uuid;

fn parse_kind(kind: &str) -> anyhow::Result<TxnKind> {
    serde_json::from_value(serde_json::Value::String(kind.to_string()))
        .map_err(|_| anyhow::anyhow!("invalid budget item kind: {kind}"))
}

fn parse_frequency(frequency: &str) -> anyhow::Result<Frequency> {
    serde_json::from_value(serde_json::Value::String(frequency.to_string()))
        .map_err(|_| anyhow::anyhow!("invalid budget item frequency: {frequency}"))
}

fn row_to_preview_item(row: db::models::BudgetScenarioItemRow) -> anyhow::Result<PreviewItem> {
    Ok(PreviewItem {
        name: row.name,
        amount: row.amount,
        kind: parse_kind(&row.kind)?,
        frequency: parse_frequency(&row.frequency)?,
        reference_date: row
            .reference_date
            .as_deref()
            .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .transpose()?,
        account: row.account,
    })
}

/// Every item across every given scenario id, converted to the
/// `finances::models::PreviewItem` shape `preview_projection`/
/// `debt_payoff_projection_with_overrides` already accept.
pub async fn scenario_ids_to_preview_items(scenario_ids: &[String]) -> anyhow::Result<Vec<PreviewItem>> {
    let mut items = Vec::new();
    for scenario_id in scenario_ids {
        let rows = db::budget_scenario_item_list(scenario_id.clone()).await?;
        for row in rows {
            items.push(row_to_preview_item(row)?);
        }
    }
    Ok(items)
}

pub async fn create_scenario(name: &str) -> anyhow::Result<db::models::BudgetScenario> {
    let id = Uuid::new_v4().to_string();
    let created_at = chrono::Local::now().to_rfc3339();
    db::budget_scenario_create(id.clone(), name.to_string(), created_at.clone()).await?;
    Ok(db::models::BudgetScenario { id, name: name.to_string(), created_at })
}

pub async fn list_scenarios() -> anyhow::Result<Vec<db::models::BudgetScenario>> {
    Ok(db::budget_scenario_list().await?)
}

pub async fn delete_scenario(id: &str) -> anyhow::Result<()> {
    db::budget_scenario_delete(id.to_string()).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn add_scenario_item(
    scenario_id: &str,
    name: &str,
    kind: &str,
    amount: f64,
    frequency: &str,
    reference_date: Option<String>,
    account: &str,
    replaces_recurring_id: Option<String>,
) -> anyhow::Result<db::models::BudgetScenarioItemRow> {
    // Validate up front so a bad kind/frequency string is rejected at
    // write time rather than silently failing to convert later when the
    // scenario is actually applied to a projection.
    parse_kind(kind)?;
    parse_frequency(frequency)?;
    let id = Uuid::new_v4().to_string();
    db::budget_scenario_item_add(
        id.clone(),
        scenario_id.to_string(),
        name.to_string(),
        kind.to_string(),
        amount,
        frequency.to_string(),
        reference_date.clone(),
        account.to_string(),
        replaces_recurring_id.clone(),
    )
    .await?;
    Ok(db::models::BudgetScenarioItemRow {
        id,
        scenario_id: scenario_id.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        amount,
        frequency: frequency.to_string(),
        reference_date,
        account: account.to_string(),
        replaces_recurring_id,
    })
}

pub async fn list_scenario_items(scenario_id: &str) -> anyhow::Result<Vec<db::models::BudgetScenarioItemRow>> {
    Ok(db::budget_scenario_item_list(scenario_id.to_string()).await?)
}

pub async fn delete_scenario_item(item_id: &str) -> anyhow::Result<()> {
    db::budget_scenario_item_delete(item_id.to_string()).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn update_scenario_item(
    item_id: &str,
    scenario_id: &str,
    name: &str,
    kind: &str,
    amount: f64,
    frequency: &str,
    reference_date: Option<String>,
    account: &str,
    replaces_recurring_id: Option<String>,
) -> anyhow::Result<db::models::BudgetScenarioItemRow> {
    parse_kind(kind)?;
    parse_frequency(frequency)?;
    db::budget_scenario_item_update(
        item_id.to_string(),
        name.to_string(),
        kind.to_string(),
        amount,
        frequency.to_string(),
        reference_date.clone(),
        account.to_string(),
        replaces_recurring_id.clone(),
    )
    .await?;
    Ok(db::models::BudgetScenarioItemRow {
        id: item_id.to_string(),
        scenario_id: scenario_id.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        amount,
        frequency: frequency.to_string(),
        reference_date,
        account: account.to_string(),
        replaces_recurring_id,
    })
}

pub async fn add_cap_allocation(
    category: &str,
    account: &str,
    amount: f64,
    include_in_projection: bool,
) -> anyhow::Result<db::models::BudgetCapAllocationRow> {
    let id = Uuid::new_v4().to_string();
    db::budget_cap_allocation_add(id.clone(), category.to_string(), account.to_string(), amount, include_in_projection)
        .await?;
    Ok(db::models::BudgetCapAllocationRow {
        id,
        category: category.to_string(),
        account: account.to_string(),
        amount,
        include_in_projection,
        updated_at: chrono::Local::now().to_rfc3339(),
    })
}

pub async fn update_cap_allocation(
    id: &str,
    category: &str,
    account: &str,
    amount: f64,
    include_in_projection: bool,
) -> anyhow::Result<db::models::BudgetCapAllocationRow> {
    db::budget_cap_allocation_update(id.to_string(), account.to_string(), amount, include_in_projection).await?;
    Ok(db::models::BudgetCapAllocationRow {
        id: id.to_string(),
        category: category.to_string(),
        account: account.to_string(),
        amount,
        include_in_projection,
        updated_at: chrono::Local::now().to_rfc3339(),
    })
}

pub async fn delete_cap_allocation(id: &str) -> anyhow::Result<()> {
    db::budget_cap_allocation_delete(id.to_string()).await?;
    Ok(())
}

pub async fn list_cap_allocations() -> anyhow::Result<Vec<db::models::BudgetCapAllocationRow>> {
    Ok(db::budget_cap_allocation_list_all().await?)
}
