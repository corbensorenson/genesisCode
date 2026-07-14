use serde_json::{Value, json};

const REPAIR_PLAN_SCHEMA_V1: &str = "genesis/diagnostic-repair-plan-v0.1";

fn fallback_action() -> Value {
    json!({
        "id": "inspect-structured-failure",
        "description": "Inspect the structured failure and retry only after its precondition is satisfied.",
        "kind": "inspect",
        "policyEffect": "none",
        "obligationEffect": "preserve",
        "automaticEligible": true,
        "requiresReview": false,
        "preconditions": [
            "The diagnostic ID and catalog identity still match the current failure.",
            "The target content identity and active policy are unchanged since diagnosis."
        ],
        "postconditions": [
            "The original failing command is rerun against the repaired content.",
            "All declared obligations remain enabled and are rerun when required."
        ]
    })
}

fn fallback_guardrails() -> Value {
    json!({
        "schema": "genesis/diagnostic-repair-guardrails-v0.1",
        "promptAuthority": "none",
        "policyBroadening": "separate-reviewed-diff",
        "obligationSuppression": "forbidden",
        "automaticActionPolicy": "catalog-eligible-only"
    })
}

pub(crate) fn build_repair_plan(
    entry: &Value,
    catalog: &Value,
    blocking_capability: Option<&str>,
    capability_is_concrete: bool,
) -> Value {
    let action = entry
        .get("safeRepairActions")
        .and_then(Value::as_array)
        .and_then(|actions| actions.first())
        .cloned()
        .unwrap_or_else(fallback_action);
    let guardrails = catalog
        .get("repairGuardrails")
        .cloned()
        .unwrap_or_else(fallback_guardrails);
    let requires_review = action
        .get("requiresReview")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let policy_review =
        action.get("policyEffect").and_then(Value::as_str) == Some("review-required");
    let automatic_eligible = action
        .get("automaticEligible")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let policy_diff = if policy_review && capability_is_concrete {
        blocking_capability.map(|capability| {
            json!({
                "schema": "genesis/capability-policy-diff-v0.1",
                "status": "proposal",
                "operation": "add-allow",
                "capability": capability,
                "base_policy_identity_sha256": null,
                "requires_review": true,
                "auto_apply": false
            })
        })
    } else {
        None
    };

    json!({
        "schema": REPAIR_PLAN_SCHEMA_V1,
        "diagnostic_id": entry.get("id").cloned().unwrap_or(Value::Null),
        "catalog_identity_sha256": catalog.get("catalogIdentitySha256").cloned().unwrap_or(Value::Null),
        "action": action,
        "guardrails": guardrails,
        "authorization": {
            "automatic_allowed": automatic_eligible && !requires_review && !policy_review,
            "policy_change_allowed": false,
            "obligation_suppression_allowed": false,
            "requires_review": requires_review,
            "requires_separate_policy_diff": policy_review
        },
        "policy_diff": policy_diff
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::build_repair_plan;

    #[test]
    fn policy_repair_is_a_non_applicable_reviewed_diff() {
        let entry = json!({
            "id": "genesis/diagnostic/v1/caps/missing",
            "safeRepairActions": [{
                "id": "review-policy-denial",
                "description": "review",
                "kind": "policy-review",
                "policyEffect": "review-required",
                "obligationEffect": "preserve",
                "automaticEligible": false,
                "requiresReview": true,
                "preconditions": ["identity matches", "policy unchanged"],
                "postconditions": ["retry", "obligations enabled"]
            }]
        });
        let catalog = json!({
            "catalogIdentitySha256": "0".repeat(64),
            "repairGuardrails": super::fallback_guardrails()
        });
        let plan = build_repair_plan(&entry, &catalog, Some("sys/time::now"), true);
        assert_eq!(plan["authorization"]["automatic_allowed"], false);
        assert_eq!(plan["authorization"]["policy_change_allowed"], false);
        assert_eq!(
            plan["authorization"]["obligation_suppression_allowed"],
            false
        );
        assert_eq!(plan["policy_diff"]["status"], "proposal");
        assert_eq!(plan["policy_diff"]["auto_apply"], false);
        assert_eq!(plan["policy_diff"]["capability"], "sys/time::now");
    }
}
