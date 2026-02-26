use serde_json::{Map, Value};

pub(crate) const DIAGNOSTICS_SCHEMA_V1: &str = "genesis/diagnostics-schema-v1";

fn suggested_fix(code: &str) -> Option<&'static str> {
    if code.starts_with("parse/") || code.starts_with("manifest/") {
        return Some("verify syntax and canonicalize with `genesis fmt --check <file>`.");
    }
    if code.starts_with("io/") {
        return Some("verify path existence, permissions, and sandbox policy.");
    }
    if code.starts_with("caps/") {
        return Some("review caps policy allowlist and per-op configuration.");
    }
    if code.starts_with("replay/") {
        return Some("re-run `genesis run` to regenerate a log for the current program hash.");
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
    let class = code
        .split('/')
        .next()
        .filter(|part| !part.trim().is_empty())
        .unwrap_or("error");
    class.to_string()
}

fn candidate_fix(code: &str) -> &'static str {
    suggested_fix(code).unwrap_or(
        "inspect `error.context`, satisfy missing preconditions, and retry the same command.",
    )
}

fn next_safe_action(code: &str) -> &'static str {
    if code.starts_with("parse/") || code.starts_with("manifest/") {
        return "run `genesis fmt --check <file>` and retry once syntax/canonicalization issues are resolved.";
    }
    if code.starts_with("io/") {
        return "confirm paths/permissions exist in the active sandbox, then retry.";
    }
    if code.starts_with("caps/") {
        return "adjust caps policy allowlist or run with a profile that permits the requested capability.";
    }
    if code.starts_with("replay/") {
        return "re-generate the execution log with `genesis run`, then replay that exact log.";
    }
    if code.starts_with("obligation/")
        || code.starts_with("test/")
        || code.starts_with("typecheck/")
    {
        return "run `genesis test --pkg <package.toml>` and resolve failing obligations before retrying.";
    }
    "inspect diagnostics/context and rerun once the reported precondition is fixed."
}

fn blocking_capability_from_context(error: &Map<String, Value>, code: &str) -> Option<String> {
    if let Some(ctx) = error.get("context").and_then(Value::as_object) {
        for key in [
            "blocking_capability",
            "capability",
            "required_capability",
            "effect_op",
            "op",
        ] {
            if let Some(capability) = ctx.get(key).and_then(Value::as_str) {
                let trimmed = capability.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    if code.starts_with("caps/") {
        let normalized = code.trim_start_matches("caps/").replace('/', "-");
        return Some(format!("capability::{normalized}"));
    }
    None
}

fn build_diagnostic(
    code: &str,
    message: &str,
    exit_code: u8,
    context: Option<Value>,
    blocking_capability: Option<String>,
) -> Value {
    let mut diag = Map::new();
    diag.insert("version".to_string(), Value::String("v1".to_string()));
    diag.insert("severity".to_string(), Value::String("error".to_string()));
    diag.insert("code".to_string(), Value::String(code.to_string()));
    diag.insert("error_class".to_string(), Value::String(error_class(code)));
    diag.insert("message".to_string(), Value::String(message.to_string()));
    diag.insert(
        "exit_code".to_string(),
        Value::Number(serde_json::Number::from(exit_code)),
    );
    diag.insert(
        "candidate_fix".to_string(),
        Value::String(candidate_fix(code).to_string()),
    );
    diag.insert(
        "next_safe_action".to_string(),
        Value::String(next_safe_action(code).to_string()),
    );
    match blocking_capability {
        Some(capability) => {
            diag.insert("blocking_capability".to_string(), Value::String(capability));
        }
        None => {
            diag.insert("blocking_capability".to_string(), Value::Null);
        }
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

    let ok = obj
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(exit_code == 0);
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
            None,
        )]
    };

    obj.insert("diagnostics".to_string(), Value::Array(diagnostics));
    envelope
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DIAGNOSTICS_SCHEMA_V1, annotate_envelope};

    #[test]
    fn annotate_envelope_adds_schema_and_empty_diagnostics_for_success() {
        let v = json!({
            "ok": true,
            "kind": "genesis/eval-v0.2",
            "data": {"value": "42"}
        });
        let out = annotate_envelope(v, 0);
        assert_eq!(
            out.get("diagnostics_schema").and_then(|x| x.as_str()),
            Some(DIAGNOSTICS_SCHEMA_V1)
        );
        assert_eq!(
            out.get("diagnostics")
                .and_then(|x| x.as_array())
                .map(|xs| xs.len()),
            Some(0)
        );
    }

    #[test]
    fn annotate_envelope_adds_error_diagnostic() {
        let v = json!({
            "ok": false,
            "kind": "genesis/error-v0.2",
            "error": {
                "code": "io/read",
                "message": "missing file"
            }
        });
        let out = annotate_envelope(v, 70);
        let diag = out
            .get("diagnostics")
            .and_then(|x| x.as_array())
            .and_then(|xs| xs.first())
            .expect("diagnostic entry");
        assert_eq!(diag.get("code").and_then(|x| x.as_str()), Some("io/read"));
        assert_eq!(diag.get("exit_code").and_then(|x| x.as_u64()), Some(70));
        assert!(diag.get("suggested_fix").and_then(|x| x.as_str()).is_some());
        assert_eq!(diag.get("error_class").and_then(|x| x.as_str()), Some("io"));
        assert!(
            diag.get("candidate_fix")
                .and_then(|x| x.as_str())
                .is_some_and(|x| !x.trim().is_empty())
        );
        assert!(
            diag.get("next_safe_action")
                .and_then(|x| x.as_str())
                .is_some_and(|x| !x.trim().is_empty())
        );
        assert!(
            diag.get("blocking_capability").is_some(),
            "diagnostics must always include blocking_capability (nullable)"
        );
    }
}
