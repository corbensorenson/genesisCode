use std::path::Path;

use serde_json::{Map, Value, json};

pub(super) const FAILURE_CONTEXT_SCHEMA_V1: &str = "genesis/failure-context-v0.1";

#[derive(Debug)]
pub(super) struct FailureContext {
    domain: &'static str,
    kind: &'static str,
    operation: &'static str,
    facts: Map<String, Value>,
    primary_span: Option<Value>,
    related_spans: Vec<Value>,
}

impl FailureContext {
    pub(super) fn new(domain: &'static str, kind: &'static str, operation: &'static str) -> Self {
        debug_assert!(matches!(
            domain,
            "parser"
                | "typechecker"
                | "evaluator"
                | "package"
                | "policy"
                | "replay"
                | "patch"
                | "build"
                | "deployment"
        ));
        Self {
            domain,
            kind,
            operation,
            facts: Map::new(),
            primary_span: None,
            related_spans: Vec::new(),
        }
    }

    pub(super) fn fact(mut self, name: &'static str, value: impl Into<Value>) -> Self {
        let mut value = value.into();
        if name == "reason"
            && let Value::String(reason) = &mut value
        {
            *reason = scrub_absolute_paths(reason);
        }
        self.facts.insert(name.to_string(), value);
        self
    }

    pub(super) fn primary_span(mut self, span: Value) -> Self {
        self.primary_span = Some(span);
        self
    }

    pub(super) fn into_value(self) -> Value {
        json!({
            "schema": FAILURE_CONTEXT_SCHEMA_V1,
            "domain": self.domain,
            "kind": self.kind,
            "operation": self.operation,
            "facts": self.facts,
            "primary_span": self.primary_span,
            "related_spans": self.related_spans,
        })
    }
}

pub(super) fn scrub_absolute_paths(message: &str) -> String {
    message
        .split_whitespace()
        .map(|token| {
            let candidate = token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '`' | '\'' | '"' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ':'
                )
            });
            if Path::new(candidate).is_absolute() {
                let replacement = Path::new(candidate)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "<absolute-path>".to_string());
                token.replacen(candidate, &replacement, 1)
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn scrub_value(value: &mut Value) {
    match value {
        Value::String(text) => *text = scrub_absolute_paths(text),
        Value::Array(values) => values.iter_mut().for_each(scrub_value),
        Value::Object(values) => values.values_mut().for_each(scrub_value),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn failure_domain(code: &str) -> &'static str {
    let family = code.split('/').next().unwrap_or_default();
    match family {
        "parse" | "canon" => "parser",
        "typecheck" => "typechecker",
        "eval" | "prelude" | "test" | "kernel" => "evaluator",
        "caps" | "policy" | "effects" => "policy",
        "replay" => "replay",
        "patch" | "semantic-edit" => "patch",
        "build" | "opt" | "stage1" | "stage2" | "wasm" => "build",
        "deploy" | "deployment" | "registry" | "remote" | "publish" | "sync" => "deployment",
        _ => "package",
    }
}

fn is_failure_context(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let expected = [
        "schema",
        "domain",
        "kind",
        "operation",
        "facts",
        "primary_span",
        "related_spans",
    ];
    object.len() == expected.len()
        && expected.iter().all(|field| object.contains_key(*field))
        && object.get("schema").and_then(Value::as_str) == Some(FAILURE_CONTEXT_SCHEMA_V1)
        && object
            .get("domain")
            .and_then(Value::as_str)
            .is_some_and(|domain| {
                matches!(
                    domain,
                    "parser"
                        | "typechecker"
                        | "evaluator"
                        | "package"
                        | "policy"
                        | "replay"
                        | "patch"
                        | "build"
                        | "deployment"
                )
            })
        && object.get("kind").and_then(Value::as_str).is_some()
        && object.get("operation").and_then(Value::as_str).is_some()
        && object.get("facts").is_some_and(Value::is_object)
        && object
            .get("primary_span")
            .is_some_and(|span| span.is_null() || span.is_object())
        && object.get("related_spans").is_some_and(Value::is_array)
}

pub(super) fn normalize_context(code: &str, context: Option<Value>) -> Value {
    if let Some(existing) = context.as_ref().filter(|value| is_failure_context(value)) {
        return existing.clone();
    }

    let mut facts = Map::new();
    facts.insert(
        "requested_code".to_string(),
        Value::String(code.to_string()),
    );
    let mut primary_span = Value::Null;
    let mut related_spans = Value::Array(Vec::new());
    if let Some(mut legacy) = context {
        scrub_value(&mut legacy);
        if let Value::Object(mut object) = legacy {
            if object
                .get("primary_span")
                .is_some_and(|span| span.is_null() || span.is_object())
            {
                primary_span = object.remove("primary_span").unwrap_or(Value::Null);
            }
            if object.get("related_spans").is_some_and(Value::is_array) {
                related_spans = object
                    .remove("related_spans")
                    .unwrap_or_else(|| Value::Array(Vec::new()));
            }
            if !object.is_empty() {
                facts.insert("legacy_context".to_string(), Value::Object(object));
            }
        } else {
            facts.insert("legacy_context".to_string(), legacy);
        }
    }

    json!({
        "schema": FAILURE_CONTEXT_SCHEMA_V1,
        "domain": failure_domain(code),
        "kind": "legacy-error",
        "operation": "diagnostics/normalize",
        "facts": facts,
        "primary_span": primary_span,
        "related_spans": related_spans,
    })
}

fn source_name(path: &Path) -> String {
    let stable = if path.is_absolute() {
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
    } else {
        path.as_os_str().to_string_lossy()
    };
    stable.replace('\\', "/")
}

fn stable_path(path: &str) -> String {
    source_name(Path::new(path))
}

fn byte_position(source: &str, byte_offset: usize) -> (u64, u64) {
    let bounded = byte_offset.min(source.len());
    let mut line = 1_u64;
    let mut column = 1_u64;
    for (offset, ch) in source.char_indices() {
        if offset >= bounded {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn point_span(path: &Path, source: &str, byte_offset: usize) -> Value {
    let (line, column) = byte_position(source, byte_offset);
    json!({
        "source": source_name(path),
        "startLine": line,
        "startColumn": column,
        "endLine": line,
        "endColumn": column,
    })
}

pub(super) fn parser_context(
    operation: &'static str,
    path: &Path,
    source: &str,
    error: &gc_coreform::ParseError,
) -> Value {
    let (kind, offset, reason) = match error {
        gc_coreform::ParseError::Eof => ("unexpected-eof", source.len(), None),
        gc_coreform::ParseError::Unexpected { at, msg } => {
            ("unexpected-token", *at, Some(msg.as_str()))
        }
        gc_coreform::ParseError::Escape { at, msg } => ("invalid-escape", *at, Some(msg.as_str())),
        gc_coreform::ParseError::Int { at, msg } => ("invalid-integer", *at, Some(msg.as_str())),
    };
    let mut context = FailureContext::new("parser", kind, operation)
        .fact("source", source_name(path))
        .fact("byte_offset", u64::try_from(offset).unwrap_or(u64::MAX))
        .primary_span(point_span(path, source, offset));
    if let Some(reason) = reason {
        context = context.fact("reason", reason);
    }
    context.into_value()
}

pub(super) fn evaluator_context(operation: &'static str, error: &gc_kernel::KernelError) -> Value {
    let kind = match error.kind {
        gc_kernel::KernelErrorKind::BadForm => "bad-form",
        gc_kernel::KernelErrorKind::Unbound => "unbound-symbol",
        gc_kernel::KernelErrorKind::Type => "type-error",
        gc_kernel::KernelErrorKind::NotCallable => "not-callable",
        gc_kernel::KernelErrorKind::Internal => "internal",
        gc_kernel::KernelErrorKind::StepLimit => "step-limit",
        gc_kernel::KernelErrorKind::MemoryLimit => "memory-limit",
    };
    FailureContext::new("evaluator", kind, operation)
        .fact("kernel_kind", format!("{:?}", error.kind))
        .fact("reason", error.msg.clone())
        .into_value()
}

pub(super) fn effects_context(operation: &'static str, error: &gc_effects::EffectsError) -> Value {
    match error {
        gc_effects::EffectsError::MissingProtocol => {
            FailureContext::new("policy", "missing-protocol", operation).into_value()
        }
        gc_effects::EffectsError::NotAnEffectProgram => {
            FailureContext::new("policy", "not-effect-program", operation).into_value()
        }
        gc_effects::EffectsError::BadEffectSeal => {
            FailureContext::new("policy", "bad-effect-seal", operation).into_value()
        }
        gc_effects::EffectsError::BadPayload(reason) => {
            FailureContext::new("policy", "bad-payload", operation)
                .fact("reason", reason.clone())
                .into_value()
        }
        gc_effects::EffectsError::Denied { op } => {
            FailureContext::new("policy", "capability-denied", operation)
                .fact("effect_op", op.clone())
                .fact("blocking_capability", op.clone())
                .into_value()
        }
        gc_effects::EffectsError::UnknownOp { op } => {
            FailureContext::new("policy", "unknown-operation", operation)
                .fact("effect_op", op.clone())
                .into_value()
        }
        gc_effects::EffectsError::Kernel(error) => evaluator_context(operation, error),
        gc_effects::EffectsError::Log(reason) => {
            FailureContext::new("replay", "invalid-log", operation)
                .fact("reason", reason.clone())
                .into_value()
        }
        gc_effects::EffectsError::ReplayMismatch(reason) => {
            FailureContext::new("replay", "fact-mismatch", operation)
                .fact("reason", reason.clone())
                .into_value()
        }
        gc_effects::EffectsError::Io(error) => FailureContext::new("replay", "io", operation)
            .fact("io_kind", format!("{:?}", error.kind()))
            .into_value(),
    }
}

pub(super) fn manifest_context(operation: &'static str, error: &gc_pkg::ManifestError) -> Value {
    match error {
        gc_pkg::ManifestError::Io(error) => FailureContext::new("package", "io", operation)
            .fact("io_kind", format!("{:?}", error.kind()))
            .into_value(),
        gc_pkg::ManifestError::Parse { path, msg } => {
            FailureContext::new("package", "manifest-parse", operation)
                .fact("path", stable_path(path))
                .fact("reason", msg.clone())
                .into_value()
        }
        gc_pkg::ManifestError::Invalid { path, msg } => {
            FailureContext::new("package", "manifest-invalid", operation)
                .fact("path", stable_path(path))
                .fact("reason", msg.clone())
                .into_value()
        }
    }
}

pub(super) fn patch_context(operation: &'static str, error: &gc_patches::PatchError) -> Value {
    match error {
        gc_patches::PatchError::Parse(reason) => FailureContext::new("patch", "parse", operation)
            .fact("reason", reason.clone())
            .into_value(),
        gc_patches::PatchError::Validate(reason) => {
            FailureContext::new("patch", "validation", operation)
                .fact("reason", reason.clone())
                .into_value()
        }
        gc_patches::PatchError::Io(error) => FailureContext::new("patch", "io", operation)
            .fact("io_kind", format!("{:?}", error.kind()))
            .into_value(),
        gc_patches::PatchError::Obligations(error) => {
            FailureContext::new("patch", "obligation", operation)
                .fact("reason", error.to_string())
                .into_value()
        }
    }
}

pub(super) fn optimize_context(
    operation: &'static str,
    error: &gc_opt::OptimizeCommandError,
) -> Value {
    match error {
        gc_opt::OptimizeCommandError::Stage1Build(reason) => {
            FailureContext::new("build", "stage1-build", operation)
                .fact("reason", reason.clone())
                .into_value()
        }
        gc_opt::OptimizeCommandError::Stage1Gate(outcome) => {
            FailureContext::new("build", "stage1-gate", operation)
                .fact("obligation", outcome.gate_report.obligation.clone())
                .fact("errors", json!(outcome.gate_report.errors))
                .into_value()
        }
        gc_opt::OptimizeCommandError::Stage2Gate(report) => {
            FailureContext::new("build", "translation-gate", operation)
                .fact("obligation", report.obligation.clone())
                .fact("errors", json!(report.errors))
                .into_value()
        }
        gc_opt::OptimizeCommandError::Stage2Compile(error) => {
            let (kind, reason) = match error {
                gc_opt::Stage2CompileError::Unsupported(reason) => ("stage2-unsupported", reason),
                gc_opt::Stage2CompileError::Internal(reason) => ("stage2-internal", reason),
            };
            FailureContext::new("build", kind, operation)
                .fact("reason", reason.clone())
                .into_value()
        }
    }
}

pub(super) fn generic_context(
    domain: &'static str,
    kind: &'static str,
    operation: &'static str,
) -> Value {
    FailureContext::new(domain, kind, operation).into_value()
}

pub(super) fn protocol_context(
    domain: &'static str,
    operation: &'static str,
    protocol_code: &str,
    payload: Option<&str>,
) -> Value {
    let mut context = FailureContext::new(domain, "selfhost-protocol", operation)
        .fact("protocol_code", protocol_code);
    if let Some(payload) = payload {
        context = context.fact("payload", payload);
    }
    context.into_value()
}

pub(super) fn obligation_context(
    operation: &'static str,
    error: &gc_obligations::ObligationError,
) -> Value {
    let (domain, kind, reason) = match error {
        gc_obligations::ObligationError::Manifest(reason) => {
            ("package", "manifest", reason.as_str())
        }
        gc_obligations::ObligationError::Module(reason) => ("package", "module", reason.as_str()),
        gc_obligations::ObligationError::Test(reason) => ("evaluator", "test", reason.as_str()),
        gc_obligations::ObligationError::Typecheck(reason) => {
            ("typechecker", "typecheck", reason.as_str())
        }
        gc_obligations::ObligationError::Opt(reason) => ("build", "optimize", reason.as_str()),
        gc_obligations::ObligationError::Store(reason) => {
            ("package", "evidence-store", reason.as_str())
        }
        gc_obligations::ObligationError::Io(error) => {
            return FailureContext::new("package", "io", operation)
                .fact("io_kind", format!("{:?}", error.kind()))
                .into_value();
        }
    };
    FailureContext::new(domain, kind, operation)
        .fact("reason", reason)
        .into_value()
}

pub(super) fn typecheck_diagnostics_json(
    diagnostics: &[gc_types::TypecheckDiagnostic],
) -> Vec<Value> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            json!({
                "id": diagnostic.id,
                "code": diagnostic.code,
                "severity": diagnostic.severity,
                "domain": "typechecker",
                "module": diagnostic.module_path,
                "ordinal": diagnostic.ordinal,
                "message": diagnostic.message,
            })
        })
        .collect()
}

pub(super) fn typecheck_failure_context(diagnostics: &[Value]) -> Value {
    FailureContext::new("typechecker", "diagnostics", "typechecker/check-package")
        .fact(
            "diagnostic_count",
            u64::try_from(diagnostics.len()).unwrap_or(u64::MAX),
        )
        .fact("diagnostics", json!(diagnostics))
        .into_value()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use gc_coreform::ParseError;
    use serde_json::Value;

    use super::{FAILURE_CONTEXT_SCHEMA_V1, FailureContext, normalize_context, parser_context};

    #[test]
    fn parser_context_localizes_utf8_byte_offsets() {
        let source = "alpha\n\u{03b2}x";
        let context = parser_context(
            "parse/module",
            Path::new("src/main.gc"),
            source,
            &ParseError::Unexpected {
                at: source.find('x').expect("x offset"),
                msg: "unexpected".to_string(),
            },
        );
        assert_eq!(context["schema"], FAILURE_CONTEXT_SCHEMA_V1);
        assert_eq!(context["domain"], "parser");
        assert_eq!(context["kind"], "unexpected-token");
        assert_eq!(context["primary_span"]["startLine"], 2);
        assert_eq!(context["primary_span"]["startColumn"], 2);
        assert_eq!(context["facts"]["byte_offset"], 8);
        assert!(matches!(context["related_spans"], Value::Array(_)));
    }

    #[test]
    fn reason_facts_scrub_absolute_paths() {
        let context = FailureContext::new("package", "io", "package/load")
            .fact("reason", "read /private/tmp/work/package.toml: missing")
            .into_value();
        assert_eq!(context["facts"]["reason"], "read package.toml: missing");
        assert!(!context["facts"]["reason"].to_string().contains("/private/"));
    }

    #[test]
    fn legacy_contexts_are_closed_and_path_scrubbed() {
        let context = normalize_context(
            "io/read",
            Some(serde_json::json!({
                "path": "/private/tmp/work/input.gc",
                "primary_span": null,
                "related_spans": [],
            })),
        );
        assert_eq!(context["schema"], FAILURE_CONTEXT_SCHEMA_V1);
        assert_eq!(context["domain"], "package");
        assert_eq!(context["facts"]["legacy_context"]["path"], "input.gc");
        assert_eq!(context.as_object().map(|object| object.len()), Some(7));
    }
}
