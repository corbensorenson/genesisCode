use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const TASK_CARD_REGISTRY_JSON: &str =
    include_str!("../../../docs/spec/GC_AGENT_TASK_CARDS_v0.3.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registry {
    aggregate_budget: AggregateBudget,
    cards: Vec<Card>,
    fallback_card: String,
    intent_schema: String,
    kind: String,
    profile_id: String,
    registry_identity_sha256: String,
    #[serde(rename = "compendiumSha256")]
    _compendium_sha256: String,
    #[serde(rename = "profileIdentitySha256")]
    _profile_identity_sha256: String,
    #[serde(rename = "sourceIdentities")]
    _source_identities: Value,
    #[serde(rename = "version")]
    _version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AggregateBudget {
    max_ascii_bytes: u64,
    #[serde(rename = "allCardsByteCount")]
    _all_cards_byte_count: u64,
    #[serde(rename = "allCardsTokenUpperBound")]
    _all_cards_token_upper_bound: u64,
    #[serde(rename = "targetAsciiBytes")]
    _target_ascii_bytes: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Card {
    byte_count: u64,
    card_sha256: String,
    content: String,
    id: String,
    max_ascii_bytes: u64,
    selectors: Selectors,
    source_hash_sha256: String,
    title: String,
    token_upper_bound: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Selectors {
    any_required_op: bool,
    domains: Vec<String>,
    goal_tokens: Vec<String>,
    workflow_tokens: Vec<String>,
}

fn tokens(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .map(|part| part.trim().to_ascii_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}

pub(super) fn select_task_cards(
    goal: &str,
    domains: &[String],
    required_workflows: &[String],
    exclude_workflows: &[String],
    required_ops: &[String],
    max_workflows: Option<usize>,
) -> Result<Value, String> {
    let registry: Registry = serde_json::from_str(TASK_CARD_REGISTRY_JSON)
        .map_err(|error| format!("embedded task-card registry is invalid: {error}"))?;
    if registry.kind != "genesis/gc-agent-task-cards-v0.3"
        || registry.intent_schema != "genesis/agent-intent-v0.1"
        || registry.profile_id != "GC-AGENT-v0.3"
        || registry.aggregate_budget.max_ascii_bytes != 12_000
    {
        return Err("embedded task-card registry identity/budget drift".to_string());
    }

    let goal_tokens = tokens(goal);
    let domain_set: BTreeSet<&str> = domains.iter().map(String::as_str).collect();
    let mut workflow_tokens = BTreeSet::new();
    for workflow in required_workflows {
        workflow_tokens.extend(tokens(workflow));
    }

    let mut selected = Vec::new();
    for card in &registry.cards {
        let mut reasons = Vec::new();
        for domain in &card.selectors.domains {
            if domain_set.contains(domain.as_str()) {
                reasons.push(format!("domain:{domain}"));
            }
        }
        for token in &card.selectors.goal_tokens {
            if goal_tokens.contains(token) {
                reasons.push(format!("goal:{token}"));
            }
        }
        for token in &card.selectors.workflow_tokens {
            if workflow_tokens.contains(token) {
                reasons.push(format!("workflow:{token}"));
            }
        }
        if card.selectors.any_required_op && !required_ops.is_empty() {
            reasons.push("required-ops".to_string());
        }
        if !reasons.is_empty() {
            selected.push(selected_card(card, reasons)?);
        }
    }
    if selected.is_empty() {
        let fallback = registry
            .cards
            .iter()
            .find(|card| card.id == registry.fallback_card)
            .ok_or_else(|| "embedded task-card fallback is missing".to_string())?;
        selected.push(selected_card(
            fallback,
            vec!["fallback:no-selector-match".to_string()],
        )?);
    }

    let byte_count: u64 = selected
        .iter()
        .filter_map(|card| card.get("byteCount").and_then(Value::as_u64))
        .sum();
    if byte_count > registry.aggregate_budget.max_ascii_bytes {
        return Err("selected task-card bundle exceeds AB-2 maximum".to_string());
    }
    let mut selection = serde_json::json!({
        "budget": {
            "byteCount": byte_count,
            "maxAsciiBytes": registry.aggregate_budget.max_ascii_bytes,
            "tokenUpperBound": byte_count,
        },
        "cards": selected,
        "intent": {
            "domains": domains,
            "exclude_workflows": exclude_workflows,
            "goal": goal,
            "max_workflows": max_workflows,
            "required_ops": required_ops,
            "required_workflows": required_workflows,
            "schema": registry.intent_schema,
        },
        "kind": "genesis/gc-agent-task-card-selection-v0.3",
        "profileId": registry.profile_id,
        "registryIdentitySha256": registry.registry_identity_sha256,
    });
    let canonical = crate::json_canonical_string(&selection);
    let identity = format!("{:x}", Sha256::digest(canonical.as_bytes()));
    selection
        .as_object_mut()
        .ok_or_else(|| "task-card selection did not produce an object".to_string())?
        .insert(
            "selectionIdentitySha256".to_string(),
            Value::String(identity),
        );
    Ok(selection)
}

fn selected_card(card: &Card, reasons: Vec<String>) -> Result<Value, String> {
    let mut value = serde_json::to_value(card)
        .map_err(|error| format!("task-card serialization failed: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "task-card did not serialize as an object".to_string())?;
    object.remove("selectors");
    object.insert(
        "selectionReasons".to_string(),
        Value::Array(reasons.into_iter().map(Value::String).collect()),
    );
    Ok(value)
}
