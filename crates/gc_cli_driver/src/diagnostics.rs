use std::sync::OnceLock;

use serde_json::{Map, Value};

pub(crate) const DIAGNOSTICS_SCHEMA_V1: &str = "genesis/diagnostics-schema-v1";
pub(crate) const DIAGNOSTIC_CATALOG_PATH: &str = "docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json";
const DIAGNOSTIC_CATALOG_JSON: &str =
    include_str!("../../../docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json");
const CATALOG_MISS_CODE: &str = "diagnostic/catalog-miss";

fn diagnostic_catalog() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(DIAGNOSTIC_CATALOG_JSON).unwrap_or_else(|_| {
            serde_json::json!({
                "kind": "genesis/diagnostic-catalog-v0.1",
                "version": "0.1.0",
                "catalogIdentitySha256": "invalid-embedded-catalog",
                "diagnostics": []
            })
        })
    })
}

pub(crate) fn embedded_diagnostic_catalog() -> Result<&'static Value, &'static str> {
    let catalog = diagnostic_catalog();
    let entries = catalog
        .get("diagnostics")
        .and_then(Value::as_array)
        .ok_or("embedded diagnostic catalog has no diagnostics array")?;
    let declared = catalog
        .get("diagnosticCount")
        .and_then(Value::as_u64)
        .ok_or("embedded diagnostic catalog has no diagnostic count")?;
    if usize::try_from(declared).ok() != Some(entries.len()) {
        return Err("embedded diagnostic catalog count does not match its entries");
    }
    Ok(catalog)
}

fn catalog_entry(code: &str) -> Option<&'static Value> {
    embedded_diagnostic_catalog()
        .ok()?
        .get("diagnostics")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("code").and_then(Value::as_str) == Some(code))
}

fn suggested_fix(code: &str) -> Option<&'static str> {
    if code.starts_with("parse/") || code.starts_with("manifest/") {
        return Some("verify syntax and canonicalize with `genesis fmt --check <file>`.");
    }
    if code.starts_with("io/") {
        return Some("verify path existence, permissions, and sandbox policy.");
    }
    if code.starts_with("caps/") {
        return Some("review the denial and make any capability change as a separate policy diff.");
    }
    if code.starts_with("replay/") {
        return Some(
            "preserve the mismatched log, then regenerate it from the exact current inputs.",
        );
    }
    if code.starts_with("obligation/")
        || code.starts_with("test/")
        || code.starts_with("typecheck/")
    {
        return Some("run `genesis test --pkg <package.toml>` and inspect obligation artifacts.");
    }
    None
}

fn error_class(code: &str) -> String {
    code.split('/')
        .next()
        .filter(|part| !part.trim().is_empty())
        .unwrap_or("error")
        .to_string()
}

fn first_repair_description(entry: &Value) -> &str {
    entry
        .get("safeRepairActions")
        .and_then(Value::as_array)
        .and_then(|repairs| repairs.first())
        .and_then(|repair| repair.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("inspect structured parameters, satisfy the reported precondition, and retry")
}

#[derive(Debug, Default)]
struct BlockingCapability {
    value: Option<String>,
    concrete: bool,
}

fn blocking_capability_from_context(error: &Map<String, Value>, code: &str) -> BlockingCapability {
    if let Some(ctx) = error.get("context").and_then(Value::as_object) {
        let facts = ctx.get("facts").and_then(Value::as_object);
        for values in [
            Some(ctx),
            facts,
            facts
                .and_then(|values| values.get("legacy_context"))
                .and_then(Value::as_object),
        ]
        .into_iter()
        .flatten()
        {
            for key in [
                "blocking_capability",
                "capability",
                "required_capability",
                "effect_op",
                "op",
            ] {
                if let Some(capability) = values.get(key).and_then(Value::as_str) {
                    let trimmed = capability.trim();
                    if !trimmed.is_empty() {
                        return BlockingCapability {
                            value: Some(trimmed.to_string()),
                            concrete: true,
                        };
                    }
                }
            }
        }
    }
    if code.starts_with("caps/") {
        let normalized = code.trim_start_matches("caps/").replace('/', "-");
        return BlockingCapability {
            value: Some(format!("capability::{normalized}")),
            concrete: false,
        };
    }
    BlockingCapability::default()
}

fn structured_fields(
    context: Option<&Value>,
    blocking_capability: Option<&str>,
) -> (Value, Value, Value) {
    let mut parameters = Map::new();
    let (primary_span, related_spans) = match context {
        Some(Value::Object(values)) => {
            let primary = values.get("primary_span").cloned().unwrap_or(Value::Null);
            let related = values
                .get("related_spans")
                .filter(|value| value.is_array())
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new()));
            let mut context_parameters = values.clone();
            context_parameters.remove("primary_span");
            context_parameters.remove("related_spans");
            if !context_parameters.is_empty() {
                parameters.insert("context".to_string(), Value::Object(context_parameters));
            }
            (primary, related)
        }
        Some(value) => {
            parameters.insert("context".to_string(), serde_json::json!({"value": value}));
            (Value::Null, Value::Array(Vec::new()))
        }
        None => (Value::Null, Value::Array(Vec::new())),
    };
    if let Some(capability) = blocking_capability {
        parameters.insert(
            "blocking_capability".to_string(),
            Value::String(capability.to_string()),
        );
    }
    (primary_span, related_spans, Value::Object(parameters))
}

fn build_diagnostic(
    requested_code: &str,
    message: &str,
    exit_code: u8,
    context: Option<Value>,
    blocking_capability: BlockingCapability,
) -> Value {
    let (entry, catalog_miss) = match catalog_entry(requested_code) {
        Some(entry) => (entry, false),
        None => (
            catalog_entry(CATALOG_MISS_CODE).unwrap_or(&Value::Null),
            true,
        ),
    };
    let code = entry
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or(CATALOG_MISS_CODE);
    let (primary_span, related_spans, mut parameters) =
        structured_fields(context.as_ref(), blocking_capability.value.as_deref());
    if catalog_miss && let Some(values) = parameters.as_object_mut() {
        values.insert(
            "reported_code".to_string(),
            Value::String(requested_code.to_string()),
        );
    }

    let catalog = diagnostic_catalog();
    let repair = first_repair_description(entry);
    let repair_plan = crate::repair_hints::build_repair_plan(
        entry,
        catalog,
        blocking_capability.value.as_deref(),
        blocking_capability.concrete,
    );
    let mut diag = Map::new();
    diag.insert("version".to_string(), Value::String("v1".to_string()));
    diag.insert(
        "id".to_string(),
        entry.get("id").cloned().unwrap_or(Value::Null),
    );
    diag.insert(
        "catalog_version".to_string(),
        catalog.get("version").cloned().unwrap_or(Value::Null),
    );
    diag.insert(
        "catalog_identity_sha256".to_string(),
        catalog
            .get("catalogIdentitySha256")
            .cloned()
            .unwrap_or(Value::Null),
    );
    diag.insert(
        "severity".to_string(),
        entry
            .get("severity")
            .cloned()
            .unwrap_or_else(|| Value::String("error".to_string())),
    );
    diag.insert(
        "phase".to_string(),
        entry.get("phase").cloned().unwrap_or(Value::Null),
    );
    diag.insert("code".to_string(), Value::String(code.to_string()));
    diag.insert("error_class".to_string(), Value::String(error_class(code)));
    diag.insert("message".to_string(), Value::String(message.to_string()));
    diag.insert(
        "exit_code".to_string(),
        Value::Number(serde_json::Number::from(exit_code)),
    );
    diag.insert(
        "candidate_fix".to_string(),
        Value::String(repair.to_string()),
    );
    diag.insert(
        "next_safe_action".to_string(),
        Value::String(repair.to_string()),
    );
    diag.insert(
        "blocking_capability".to_string(),
        blocking_capability
            .value
            .map(Value::String)
            .unwrap_or(Value::Null),
    );
    diag.insert("repair_plan".to_string(), repair_plan);
    diag.insert("primary_span".to_string(), primary_span);
    diag.insert("related_spans".to_string(), related_spans);
    diag.insert("parameters".to_string(), parameters);
    for field in ["likelyCauses", "safeRepairActions", "documentation"] {
        let runtime_field = match field {
            "likelyCauses" => "likely_causes",
            "safeRepairActions" => "safe_repair_actions",
            _ => "documentation",
        };
        diag.insert(
            runtime_field.to_string(),
            entry
                .get(field)
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
        );
    }
    if let Some(ctx) = context {
        diag.insert("context".to_string(), ctx);
    }
    if let Some(fix) = suggested_fix(code) {
        diag.insert("suggested_fix".to_string(), Value::String(fix.to_string()));
    }
    Value::Object(diag)
}

fn error_diagnostic(error: &Map<String, Value>, exit_code: u8) -> Value {
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("error/unknown");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("command failed");
    build_diagnostic(
        code,
        message,
        exit_code,
        error.get("context").cloned(),
        blocking_capability_from_context(error, code),
    )
}

pub(crate) fn annotate_envelope(mut envelope: Value, exit_code: u8) -> Value {
    let Some(obj) = envelope.as_object_mut() else {
        return envelope;
    };
    obj.insert(
        "diagnostics_schema".to_string(),
        Value::String(DIAGNOSTICS_SCHEMA_V1.to_string()),
    );
    obj.insert(
        "diagnostic_catalog".to_string(),
        serde_json::json!({
            "path": DIAGNOSTIC_CATALOG_PATH,
            "version": diagnostic_catalog().get("version"),
            "identity_sha256": diagnostic_catalog().get("catalogIdentitySha256"),
        }),
    );

    let ok = obj
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(exit_code == 0);
    if !ok && !obj.get("error").is_some_and(Value::is_object) {
        obj.insert(
            "error".to_string(),
            serde_json::json!({
                "code": "error/unknown",
                "message": "command reported failure without a producer diagnostic",
            }),
        );
    }
    if !ok && let Some(error) = obj.get_mut("error").and_then(Value::as_object_mut) {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("error/unknown")
            .to_string();
        if let Some(Value::String(message)) = error.get_mut("message") {
            *message = crate::structured_failures::scrub_absolute_paths(message);
        }
        let context = error.remove("context");
        error.insert(
            "context".to_string(),
            crate::structured_failures::normalize_context(&code, context),
        );
    }
    let diagnostics = if ok {
        Vec::new()
    } else if let Some(err) = obj.get("error").and_then(Value::as_object) {
        vec![error_diagnostic(err, exit_code)]
    } else {
        vec![build_diagnostic(
            "error/unknown",
            "command failed",
            exit_code,
            None,
            BlockingCapability::default(),
        )]
    };

    obj.insert("diagnostics".to_string(), Value::Array(diagnostics));
    envelope
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{DIAGNOSTICS_SCHEMA_V1, annotate_envelope, embedded_diagnostic_catalog};

    #[test]
    fn embedded_catalog_is_closed_and_counted() {
        let catalog = embedded_diagnostic_catalog().expect("valid embedded catalog");
        assert_eq!(
            catalog.get("kind").and_then(|value| value.as_str()),
            Some("genesis/diagnostic-catalog-v0.1")
        );
        assert!(
            catalog
                .get("diagnosticCount")
                .and_then(|value| value.as_u64())
                .is_some_and(|count| count >= 100)
        );
    }

    #[test]
    fn annotate_envelope_adds_schema_and_empty_diagnostics_for_success() {
        let out = annotate_envelope(
            json!({"ok": true, "kind": "genesis/eval-v0.2", "data": {"value": "42"}}),
            0,
        );
        assert_eq!(
            out.get("diagnostics_schema")
                .and_then(|value| value.as_str()),
            Some(DIAGNOSTICS_SCHEMA_V1)
        );
        assert_eq!(
            out.get("diagnostics")
                .and_then(|value| value.as_array())
                .map(Vec::len),
            Some(0)
        );
        assert!(out.pointer("/diagnostic_catalog/identity_sha256").is_some());
    }

    #[test]
    fn annotate_envelope_adds_cataloged_error_diagnostic() {
        let out = annotate_envelope(
            json!({
                "ok": false,
                "kind": "genesis/error-v0.2",
                "error": {"code": "io/read", "message": "missing file"}
            }),
            70,
        );
        let diag = out
            .get("diagnostics")
            .and_then(|value| value.as_array())
            .and_then(|values| values.first())
            .expect("diagnostic entry");
        assert_eq!(
            diag.get("code").and_then(|value| value.as_str()),
            Some("io/read")
        );
        assert_eq!(
            diag.get("id").and_then(|value| value.as_str()),
            Some("genesis/diagnostic/v1/io/read")
        );
        assert_eq!(
            diag.get("phase").and_then(|value| value.as_str()),
            Some("io")
        );
        for field in [
            "catalog_identity_sha256",
            "primary_span",
            "related_spans",
            "parameters",
            "likely_causes",
            "safe_repair_actions",
            "documentation",
        ] {
            assert!(diag.get(field).is_some(), "missing {field}");
        }
    }

    #[test]
    fn failure_without_producer_error_is_fail_closed() {
        let out = annotate_envelope(
            json!({"ok": false, "kind": "genesis/test-v0.2", "data": {"ok": false}}),
            30,
        );
        assert_eq!(out["error"]["code"], "error/unknown");
        assert_eq!(
            out["error"]["context"]["schema"],
            "genesis/failure-context-v0.1"
        );
        assert_eq!(out["error"]["context"]["domain"], "package");
        assert_eq!(out["diagnostics"][0]["code"], "error/unknown");
    }

    #[test]
    fn unknown_code_fails_closed_to_catalog_miss() {
        let out = annotate_envelope(
            json!({
                "ok": false,
                "kind": "genesis/error-v0.2",
                "error": {"code": "prompt/injected-code", "message": "untrusted"}
            }),
            70,
        );
        let diag = &out["diagnostics"][0];
        assert_eq!(diag["code"], "diagnostic/catalog-miss");
        assert_eq!(diag["parameters"]["reported_code"], "prompt/injected-code");
    }

    #[test]
    fn context_parameters_remain_namespaced_and_spans_are_extracted() {
        let out = annotate_envelope(
            json!({
                "ok": false,
                "kind": "genesis/error-v0.2",
                "error": {
                    "code": "caps/missing",
                    "message": "denied",
                    "context": {
                        "capability": "sys/time::now",
                        "primary_span": {"source": "main.gc", "startLine": 1, "startColumn": 1, "endLine": 1, "endColumn": 4},
                        "related_spans": []
                    }
                }
            }),
            77,
        );
        let diag = &out["diagnostics"][0];
        assert_eq!(diag["parameters"]["blocking_capability"], "sys/time::now");
        assert_eq!(
            diag["parameters"]["context"]["schema"],
            "genesis/failure-context-v0.1"
        );
        assert_eq!(
            diag["parameters"]["context"]["facts"]["legacy_context"]["capability"],
            "sys/time::now"
        );
        assert!(diag["parameters"].get("capability").is_none());
        assert_eq!(diag["primary_span"]["source"], "main.gc");
        assert_eq!(diag["related_spans"], json!([]));
        assert_eq!(
            diag["repair_plan"]["schema"],
            "genesis/diagnostic-repair-plan-v0.1"
        );
        assert_eq!(
            diag["repair_plan"]["authorization"]["automatic_allowed"],
            false
        );
        assert_eq!(
            diag["repair_plan"]["authorization"]["policy_change_allowed"],
            false
        );
        assert_eq!(
            diag["repair_plan"]["authorization"]["obligation_suppression_allowed"],
            false
        );
        assert_eq!(diag["repair_plan"]["policy_diff"]["status"], "proposal");
        assert_eq!(
            diag["repair_plan"]["policy_diff"]["capability"],
            "sys/time::now"
        );
        assert_eq!(diag["repair_plan"]["policy_diff"]["auto_apply"], false);
    }

    #[test]
    fn obligation_repair_requires_rerun_and_cannot_suppress() {
        let out = annotate_envelope(
            json!({
                "ok": false,
                "kind": "genesis/error-v0.2",
                "error": {"code": "typecheck/error", "message": "mismatch"}
            }),
            30,
        );
        let plan = &out["diagnostics"][0]["repair_plan"];
        assert_eq!(plan["action"]["obligationEffect"], "rerun-required");
        assert_eq!(
            plan["authorization"]["obligation_suppression_allowed"],
            false
        );
        assert_eq!(plan["policy_diff"], Value::Null);
    }

    #[test]
    fn inferred_capability_placeholder_never_becomes_a_policy_diff() {
        let out = annotate_envelope(
            json!({
                "ok": false,
                "kind": "genesis/error-v0.2",
                "error": {"code": "caps/missing", "message": "missing capability"}
            }),
            77,
        );
        let diagnostic = &out["diagnostics"][0];
        assert_eq!(diagnostic["blocking_capability"], "capability::missing");
        assert_eq!(diagnostic["repair_plan"]["policy_diff"], Value::Null);
        assert_eq!(
            diagnostic["repair_plan"]["authorization"]["requires_separate_policy_diff"],
            true
        );
        assert_eq!(
            diagnostic["repair_plan"]["authorization"]["policy_change_allowed"],
            false
        );
    }
}
