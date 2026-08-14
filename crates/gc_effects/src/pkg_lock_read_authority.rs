use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_with_mode};

use crate::EffectsError;
use crate::policy::SelfhostAuthorityConfig;

#[path = "pkg_lock_model_authority.rs"]
mod model;
pub(crate) use model::PkgLockModelDecision;
#[path = "pkg_lock_ops_authority.rs"]
mod ops;
pub(crate) use ops::{PkgBridgeLockFacts, PkgLockOpsDecision};
#[path = "pkg_bridge_authority.rs"]
mod bridge;
pub(crate) use bridge::{PkgBridgeDecision, PkgBridgeFacts, PkgBridgeObject};
#[path = "pkg_snapshot_authority.rs"]
mod snapshot;
pub(crate) use snapshot::PkgSnapshotDecision;
#[cfg(test)]
#[path = "pkg_publish_authority_inspect_tests.rs"]
mod publish_authority_inspect_tests;
#[cfg(test)]
#[path = "pkg_publish_authority_prepare_tests.rs"]
mod publish_authority_prepare_tests;
#[cfg(test)]
#[path = "pkg_publish_glob_tests.rs"]
mod publish_glob_tests;
#[cfg(test)]
#[path = "pkg_publish_policy_tests.rs"]
mod publish_policy_tests;

const BINDING: &str = "core/pkg::lock-read-authority";
const REQUEST_KIND: &str = "genesis/pkg-lock-read-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-lock-read-authority-result-v0.1";
const STEP_LIMIT: u64 = 20_000_000;
const ALLOC_LIMIT: u64 = 80_000_000;

pub(crate) struct PkgLockReadAuthority {
    context: EvalCtx,
    authority: Value,
    model_authority: Option<Value>,
    ops_authority: Option<Value>,
    bridge_authority: Option<Value>,
    snapshot_authority: Option<Value>,
}

#[derive(Debug)]
pub(crate) enum PkgLockReadDecision {
    Lock(Term),
    Error { code: String, message: String },
}

impl PkgLockReadAuthority {
    pub(crate) fn required_for_request(op: &str, _payload: &Term) -> bool {
        matches!(
            op,
            "core/pkg-low::load-lock"
                | "core/pkg-low::init"
                | "core/pkg-low::add"
                | "core/pkg-low::list"
                | "core/pkg-low::info"
                | "core/pkg-low::lock"
                | "core/pkg-low::update"
                | "core/pkg-low::install"
                | "core/pkg-low::verify"
        ) || matches!(op, "core/pkg-low::bridge" | "core/pkg-low::snapshot")
    }

    pub(crate) fn load(config: &SelfhostAuthorityConfig) -> Result<Self, EffectsError> {
        let mut context = EvalCtx::with_step_limit(None);
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(ALLOC_LIMIT),
            max_bytes_len: Some(4 * 1024 * 1024),
            max_map_len: Some(65_536),
            max_string_len: Some(4 * 1024 * 1024),
            max_vec_len: Some(65_536),
            ..MemLimits::default()
        });
        let prelude = build_prelude(&mut context);
        let mut environment = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut context,
            &mut environment,
            config.bootstrap_mode,
            config.artifact.as_deref(),
        )
        .map_err(|error| authority_error(format!("artifact bootstrap failed: {error:#}")))?;
        let authority = environment
            .get(BINDING)
            .ok_or_else(|| authority_error(format!("missing binding {BINDING}")))?;
        let model_authority = environment.get(model::MODEL_BINDING);
        let ops_authority = environment.get(ops::OPS_BINDING);
        let bridge_authority = environment.get(bridge::BRIDGE_BINDING);
        let snapshot_authority = environment.get(snapshot::SNAPSHOT_BINDING);
        context.reset_counters();
        context.step_limit = Some(STEP_LIMIT);
        Ok(Self {
            context,
            authority,
            model_authority,
            ops_authority,
            bridge_authority,
            snapshot_authority,
        })
    }

    pub(crate) fn read_toml(&mut self, bytes: &[u8]) -> Result<PkgLockReadDecision, EffectsError> {
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                return Ok(PkgLockReadDecision::Error {
                    code: "core/pkg/bad-lock".to_string(),
                    message: "lock file is not UTF-8".to_string(),
                });
            }
        };
        let document: toml::Value = match toml::from_str(text) {
            Ok(document) => document,
            Err(_) => {
                return Ok(PkgLockReadDecision::Error {
                    code: "core/pkg/bad-lock".to_string(),
                    message: "lock file is not valid TOML".to_string(),
                });
            }
        };
        self.read_document(toml_to_term(document))
    }

    fn read_document(&mut self, document: Term) -> Result<PkgLockReadDecision, EffectsError> {
        let request = map([
            (":document", document),
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":op", Term::symbol(":read")),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("apply failed: {error}")))?;
        decode_result(plain_result(value, &self.context)?, request_hash)
    }
}

fn decode_result(term: Term, request_hash: [u8; 32]) -> Result<PkgLockReadDecision, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":code",
            ":kind",
            ":lock",
            ":message",
            ":ok",
            ":request-h",
            ":v",
        ],
    )?;
    require_string(fields, ":kind", RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if required_bool(fields, ":ok")? {
        require_nil(fields, ":code")?;
        require_nil(fields, ":message")?;
        let lock = field(fields, ":lock")?.clone();
        validate_lock(&lock)?;
        Ok(PkgLockReadDecision::Lock(lock))
    } else {
        require_nil(fields, ":lock")?;
        Ok(PkgLockReadDecision::Error {
            code: required_string(fields, ":code")?.to_string(),
            message: required_string(fields, ":message")?.to_string(),
        })
    }
}

fn validate_lock(lock: &Term) -> Result<(), EffectsError> {
    let fields = exact_map(
        lock,
        &[
            ":artifacts",
            ":locked",
            ":policy",
            ":registries",
            ":requirements",
            ":workspace",
        ],
    )?;
    required_string(fields, ":workspace")?;
    required_string(fields, ":policy")?;
    validate_string_map(field(fields, ":registries")?, ":registries")?;
    validate_string_map(field(fields, ":artifacts")?, ":artifacts")?;
    validate_requirements(field(fields, ":requirements")?)?;
    validate_locked(field(fields, ":locked")?)
}

fn validate_string_map(term: &Term, name: &str) -> Result<(), EffectsError> {
    let Term::Map(entries) = term else {
        return Err(authority_error(format!("result {name} must be map")));
    };
    for (key, value) in entries {
        if !matches!((&key.0, value), (Term::Str(_), Term::Str(_))) {
            return Err(authority_error(format!(
                "result {name} entries must be string/string"
            )));
        }
    }
    Ok(())
}

fn validate_requirements(term: &Term) -> Result<(), EffectsError> {
    let Term::Map(entries) = term else {
        return Err(authority_error("result :requirements must be map"));
    };
    for (key, value) in entries {
        if !matches!(key.0, Term::Str(_)) {
            return Err(authority_error("result requirement name must be string"));
        }
        let fields = exact_map(value, &[":registry", ":selector", ":update-policy"])?;
        required_string(fields, ":selector")?;
        optional_string(fields, ":registry")?;
        match field(fields, ":update-policy")? {
            Term::Symbol(value) if value == ":manual" || value == ":auto" => {}
            _ => {
                return Err(authority_error(
                    "result requirement :update-policy must be :manual or :auto",
                ));
            }
        }
    }
    Ok(())
}

fn validate_locked(term: &Term) -> Result<(), EffectsError> {
    let Term::Map(entries) = term else {
        return Err(authority_error("result :locked must be map"));
    };
    for (key, value) in entries {
        if !matches!(key.0, Term::Str(_)) {
            return Err(authority_error("result locked name must be string"));
        }
        let fields = exact_map(
            value,
            &[
                ":commit",
                ":exports_hash",
                ":registry",
                ":resolved-ref",
                ":snapshot",
                ":source_selector",
            ],
        )?;
        required_string(fields, ":snapshot")?;
        for name in [
            ":commit",
            ":exports_hash",
            ":registry",
            ":resolved-ref",
            ":source_selector",
        ] {
            optional_string(fields, name)?;
        }
    }
    Ok(())
}

fn toml_to_term(value: toml::Value) -> Term {
    match value {
        toml::Value::String(value) => Term::Str(value),
        toml::Value::Integer(value) => Term::Int(value.into()),
        toml::Value::Boolean(value) => Term::Bool(value),
        toml::Value::Float(value) => map([(":toml-float", Term::Str(format!("{value:e}")))]),
        toml::Value::Datetime(value) => map([(":toml-datetime", Term::Str(value.to_string()))]),
        toml::Value::Array(values) => Term::Vector(values.into_iter().map(toml_to_term).collect()),
        toml::Value::Table(values) => Term::Map(
            values
                .into_iter()
                .map(|(key, value)| (TermOrdKey(Term::Str(key)), toml_to_term(value)))
                .collect(),
        ),
    }
}

fn authority_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!(
        "selfhost package lock read authority: {}",
        message.into()
    ))
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn plain_result(value: Value, context: &EvalCtx) -> Result<Term, EffectsError> {
    if let Value::Sealed { token, payload } = &value
        && context
            .protocol
            .is_some_and(|protocol| *token == protocol.error)
    {
        let detail = payload
            .to_plain_term()
            .map(|term| print_term(&term))
            .unwrap_or_else(|| "<opaque-error-payload>".to_string());
        return Err(authority_error(format!("returned sealed ERROR {detail}")));
    }
    value
        .to_plain_term()
        .ok_or_else(|| authority_error(format!("returned opaque value: {value:?}")))
}

fn exact_map<'a>(
    term: &'a Term,
    expected: &[&str],
) -> Result<&'a BTreeMap<TermOrdKey, Term>, EffectsError> {
    let Term::Map(fields) = term else {
        return Err(authority_error(format!(
            "result must be a map, got {}",
            print_term(term)
        )));
    };
    let actual: Vec<String> = fields
        .keys()
        .map(|entry| match &entry.0 {
            Term::Symbol(value) => value.clone(),
            other => print_term(other),
        })
        .collect();
    let wanted: Vec<String> = expected.iter().map(|value| (*value).to_string()).collect();
    if actual != wanted {
        return Err(authority_error(format!(
            "result field set mismatch: actual={actual:?} expected={wanted:?} term={}",
            print_term(term)
        )));
    }
    Ok(fields)
}

fn field<'a>(fields: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> Result<&'a Term, EffectsError> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| authority_error(format!("result missing {name}")))
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, EffectsError> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(authority_error(format!("result {name} must be string"))),
    }
}

fn optional_string(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), EffectsError> {
    match field(fields, name)? {
        Term::Str(_) | Term::Nil => Ok(()),
        _ => Err(authority_error(format!(
            "result {name} must be string or nil"
        ))),
    }
}

fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), EffectsError> {
    if required_string(fields, name)? == expected {
        Ok(())
    } else {
        Err(authority_error(format!("result {name} mismatch")))
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), EffectsError> {
    match field(fields, name)? {
        Term::Int(value) if value == &expected.into() => Ok(()),
        _ => Err(authority_error(format!("result {name} mismatch"))),
    }
}

fn required_bool(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<bool, EffectsError> {
    match field(fields, name)? {
        Term::Bool(value) => Ok(*value),
        _ => Err(authority_error(format!("result {name} must be bool"))),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), EffectsError> {
    if matches!(field(fields, name)?, Term::Nil) {
        Ok(())
    } else {
        Err(authority_error(format!("result {name} must be nil")))
    }
}

fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact_config() -> SelfhostAuthorityConfig {
        let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../selfhost/toolchain.gc")
            .canonicalize()
            .expect("canonical selfhost artifact path");
        SelfhostAuthorityConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        }
    }

    #[test]
    fn canonical_document_matches_legacy_public_term() {
        let source = r#"
version = 2
workspace = "demo"
policy = "policy:test"
ignored = 1.5

[registries]
default = "https://example.invalid"

[requirements]
dep = { selector = "semver:^1", update_policy = "auto", registry = "default", strategy = "tag-policy", tag_policy = "^1" }

[locked]
dep = { commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", snapshot = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", registry = "default", source_selector = "", resolved_ref = "refs/tags/v1", exports_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc" }

[artifacts]
rationale = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
"#;
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        let PkgLockReadDecision::Lock(lock) = authority.read_toml(source.as_bytes()).unwrap()
        else {
            panic!("expected normalized lock");
        };
        validate_lock(&lock).unwrap();
        let fields = exact_map(
            &lock,
            &[
                ":artifacts",
                ":locked",
                ":policy",
                ":registries",
                ":requirements",
                ":workspace",
            ],
        )
        .unwrap();
        assert_eq!(required_string(fields, ":workspace").unwrap(), "demo");
        let locked = field(fields, ":locked").unwrap();
        let Term::Map(locked) = locked else {
            return;
        };
        let dep = locked
            .get(&TermOrdKey(Term::Str("dep".to_string())))
            .unwrap();
        let dep = exact_map(
            dep,
            &[
                ":commit",
                ":exports_hash",
                ":registry",
                ":resolved-ref",
                ":snapshot",
                ":source_selector",
            ],
        )
        .unwrap();
        assert!(matches!(field(dep, ":source_selector").unwrap(), Term::Nil));
    }

    #[test]
    fn malformed_and_semantically_invalid_documents_reject() {
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        for source in [
            "not = [toml",
            "version = 3\nworkspace = \"demo\"\n",
            "version = 2\nworkspace = \"demo\"\n[requirements]\ndep = { selector = \"refs/heads/main\", strategy = \"tag-policy\" }\n",
            "version = 2\nworkspace = \"demo\"\n[requirements]\ndep = { selector = \"abc\", update_policy = 1 }\n",
            "version = 2\nworkspace = \"demo\"\n[requirements]\ndep = { selector = \"abc\", strategy = 1 }\n",
            "version = 2\nworkspace = \"demo\"\n[requirements]\ndep = { selector = \"abc\", tag_policy = 1 }\n",
            "version = 2\nworkspace = \"demo\"\n[locked]\ndep = { snapshot = \"abc\", environment_fingerprint = 1 }\n",
        ] {
            assert!(matches!(
                authority.read_toml(source.as_bytes()).unwrap(),
                PkgLockReadDecision::Error { .. }
            ));
        }
    }

    #[test]
    fn decoder_rejects_open_and_unbound_results() {
        let request_hash = [7_u8; 32];
        let base = map([
            (":code", Term::Str("core/pkg/bad-lock".to_string())),
            (":kind", Term::Str(RESULT_KIND.to_string())),
            (":lock", Term::Nil),
            (":message", Term::Str("bad".to_string())),
            (":ok", Term::Bool(false)),
            (":request-h", Term::Str(hex32(request_hash))),
            (":v", Term::Int(1.into())),
        ]);
        let mut open = match base.clone() {
            Term::Map(fields) => fields,
            _ => return,
        };
        open.insert(TermOrdKey(Term::symbol(":extra")), Term::Nil);
        assert!(decode_result(Term::Map(open), request_hash).is_err());
        let mut unbound = match base {
            Term::Map(fields) => fields,
            _ => return,
        };
        unbound.insert(
            TermOrdKey(Term::symbol(":request-h")),
            Term::Str("0".repeat(64)),
        );
        assert!(decode_result(Term::Map(unbound), request_hash).is_err());
    }

    #[test]
    fn bridge_always_requires_object_authority() {
        assert!(PkgLockReadAuthority::required_for_request(
            "core/pkg-low::bridge",
            &map([(":lock", Term::Str("genesis.lock".to_string()))]),
        ));
        assert!(PkgLockReadAuthority::required_for_request(
            "core/pkg-low::bridge",
            &map([]),
        ));
        assert!(PkgLockReadAuthority::required_for_request(
            "core/pkg-low::bridge",
            &map([(":lock", Term::Int(1.into()))]),
        ));
    }
}
