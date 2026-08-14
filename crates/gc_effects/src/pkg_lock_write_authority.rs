use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_with_mode};

use crate::EffectsError;
use crate::policy::SelfhostAuthorityConfig;

const BINDING: &str = "core/pkg::lock-write-authority";
const REQUEST_KIND: &str = "genesis/pkg-lock-write-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-lock-write-authority-result-v0.1";
const STEP_LIMIT: u64 = 20_000_000;
const ALLOC_LIMIT: u64 = 80_000_000;

pub(crate) struct PkgLockWriteAuthority {
    context: EvalCtx,
    authority: Value,
}

#[derive(Debug)]
pub(crate) enum PkgLockWriteDecision {
    Write { bytes: Vec<u8>, lock_hash: String },
    Error { code: String, message: String },
}

impl PkgLockWriteAuthority {
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
        context.reset_counters();
        context.step_limit = Some(STEP_LIMIT);
        Ok(Self { context, authority })
    }

    pub(crate) fn write(&mut self, payload: &Term) -> Result<PkgLockWriteDecision, EffectsError> {
        let request = map([
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":op", Term::symbol(":write")),
            (":payload", payload.clone()),
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

    pub(crate) fn write_model(
        &mut self,
        lock_path: &str,
        model: &gc_pkg::GenesisLock,
    ) -> Result<PkgLockWriteDecision, EffectsError> {
        self.write(&lock_model_payload(lock_path, model)?)
    }
}

pub(crate) fn lock_model_payload(
    lock_path: &str,
    model: &gc_pkg::GenesisLock,
) -> Result<Term, EffectsError> {
    let version = i64::try_from(model.version)
        .map_err(|_| authority_error("lock model version exceeds protocol integer range"))?;
    let registries = model
        .registries
        .iter()
        .map(|(name, remote)| {
            (
                TermOrdKey(Term::Str(name.clone())),
                Term::Str(remote.clone()),
            )
        })
        .collect();
    let requirements = model
        .requirements
        .iter()
        .map(|(name, requirement)| {
            let update_policy = match requirement.update_policy {
                gc_pkg::UpdatePolicy::Manual => ":manual",
                gc_pkg::UpdatePolicy::Auto => ":auto",
            };
            let value = map([
                (
                    ":registry",
                    requirement
                        .registry
                        .clone()
                        .map(Term::Str)
                        .unwrap_or(Term::Nil),
                ),
                (":selector", Term::Str(requirement.selector.clone())),
                (
                    ":strategy",
                    Term::symbol(format!(":{}", requirement.strategy.as_str())),
                ),
                (
                    ":tag-policy",
                    requirement
                        .tag_policy
                        .clone()
                        .map(Term::Str)
                        .unwrap_or(Term::Nil),
                ),
                (":update-policy", Term::symbol(update_policy)),
            ]);
            (TermOrdKey(Term::Str(name.clone())), value)
        })
        .collect();
    let locked = model
        .locked
        .iter()
        .map(|(name, entry)| {
            let value = locked_entry_payload(entry);
            (TermOrdKey(Term::Str(name.clone())), value)
        })
        .collect();
    let artifacts = model
        .artifacts
        .iter()
        .map(|(name, hash)| (TermOrdKey(Term::Str(name.clone())), Term::Str(hash.clone())))
        .collect();
    Ok(map([
        (":artifacts", Term::Map(artifacts)),
        (":lock", Term::Str(lock_path.to_string())),
        (":locked", Term::Map(locked)),
        (":policy", Term::Str(model.policy.clone())),
        (":registries", Term::Map(registries)),
        (":requirements", Term::Map(requirements)),
        (":version", Term::Int(version.into())),
        (":workspace", Term::Str(model.workspace.clone())),
    ]))
}

pub(crate) fn locked_entry_payload(entry: &gc_pkg::LockedEntry) -> Term {
    map([
        (
            ":commit",
            entry.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
        ),
        (
            ":environment-fingerprint",
            entry
                .environment_fingerprint
                .clone()
                .map(Term::Str)
                .unwrap_or(Term::Nil),
        ),
        (
            ":exports_hash",
            entry
                .exports_hash
                .clone()
                .map(Term::Str)
                .unwrap_or(Term::Nil),
        ),
        (
            ":registry",
            entry.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
        ),
        (
            ":resolved-ref",
            entry
                .resolved_ref
                .clone()
                .map(Term::Str)
                .unwrap_or(Term::Nil),
        ),
        (":snapshot", Term::Str(entry.snapshot.clone())),
        (":source_selector", Term::Str(entry.source_selector.clone())),
    ])
}

fn decode_result(term: Term, request_hash: [u8; 32]) -> Result<PkgLockWriteDecision, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":bytes",
            ":code",
            ":kind",
            ":lock-h",
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
        let bytes = required_bytes(fields, ":bytes")?;
        std::str::from_utf8(&bytes)
            .map_err(|_| authority_error("result :bytes must be canonical UTF-8 TOML"))?;
        let lock_hash = required_string(fields, ":lock-h")?.to_string();
        if !is_hash(&lock_hash) {
            return Err(authority_error(
                "result :lock-h must be lowercase BLAKE3 hex64",
            ));
        }
        if blake3::hash(&bytes).to_hex().as_str() != lock_hash {
            return Err(authority_error("result bytes and :lock-h contradict"));
        }
        Ok(PkgLockWriteDecision::Write { bytes, lock_hash })
    } else {
        require_nil(fields, ":bytes")?;
        require_nil(fields, ":lock-h")?;
        Ok(PkgLockWriteDecision::Error {
            code: required_string(fields, ":code")?.to_string(),
            message: required_string(fields, ":message")?.to_string(),
        })
    }
}

fn authority_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!(
        "selfhost package lock write authority: {}",
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
        return Err(authority_error("result must be a map"));
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
        return Err(authority_error("result field set mismatch"));
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

fn required_bytes(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Vec<u8>, EffectsError> {
    match field(fields, name)? {
        Term::Bytes(value) => Ok(value.to_vec()),
        _ => Err(authority_error(format!("result {name} must be bytes"))),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), EffectsError> {
    if matches!(field(fields, name)?, Term::Nil) {
        Ok(())
    } else {
        Err(authority_error(format!("result {name} must be nil")))
    }
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

    fn full_payload(strategy: Term) -> Term {
        let requirement = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":registry")),
                    Term::Str("default".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":selector")),
                    Term::Str("semver:^1".to_string()),
                ),
                (TermOrdKey(Term::symbol(":strategy")), strategy),
                (
                    TermOrdKey(Term::symbol(":tag-policy")),
                    Term::Str("^1".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":update-policy")),
                    Term::symbol(":manual"),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let locked = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":commit")),
                    Term::Str("a".repeat(64)),
                ),
                (
                    TermOrdKey(Term::symbol(":environment-fingerprint")),
                    Term::Str("env".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":exports_hash")),
                    Term::Str("b".repeat(64)),
                ),
                (
                    TermOrdKey(Term::symbol(":registry")),
                    Term::Str("default".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":resolved-ref")),
                    Term::Str("refs/tags/v1.0.0".to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":snapshot")),
                    Term::Str("d".repeat(64)),
                ),
                (
                    TermOrdKey(Term::symbol(":source_selector")),
                    Term::Str("semver:^1".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        map([
            (
                ":artifacts",
                Term::Map(
                    [(
                        TermOrdKey(Term::Str("demo".to_string())),
                        Term::Str("c".repeat(64)),
                    )]
                    .into_iter()
                    .collect(),
                ),
            ),
            (":lock", Term::Str("genesis.lock".to_string())),
            (
                ":locked",
                Term::Map(
                    [(TermOrdKey(Term::Str("dep".to_string())), locked)]
                        .into_iter()
                        .collect(),
                ),
            ),
            (":policy", Term::Str("policy:test".to_string())),
            (
                ":registries",
                Term::Map(
                    [(
                        TermOrdKey(Term::Str("default".to_string())),
                        Term::Str("https://example.invalid".to_string()),
                    )]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                ":requirements",
                Term::Map(
                    [(TermOrdKey(Term::Str("dep".to_string())), requirement)]
                        .into_iter()
                        .collect(),
                ),
            ),
            (":version", Term::Int(2.into())),
            (":workspace", Term::Str("demo".to_string())),
        ])
    }

    fn valid_result(request_hash: [u8; 32], bytes: &[u8]) -> Term {
        map([
            (":bytes", Term::Bytes(bytes.to_vec().into())),
            (":code", Term::Nil),
            (":kind", Term::Str(RESULT_KIND.to_string())),
            (
                ":lock-h",
                Term::Str(blake3::hash(bytes).to_hex().to_string()),
            ),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":request-h", Term::Str(hex32(request_hash))),
            (":v", Term::Int(1.into())),
        ])
    }

    #[test]
    fn decoder_rejects_open_unbound_and_substituted_results() {
        let request_hash = [7_u8; 32];
        let valid = valid_result(request_hash, b"version = 2\n");
        assert!(decode_result(valid.clone(), request_hash).is_ok());
        assert!(decode_result(valid.clone(), [8_u8; 32]).is_err());

        let mut open = match valid.clone() {
            Term::Map(fields) => fields,
            _ => BTreeMap::new(),
        };
        open.insert(TermOrdKey(Term::symbol(":extra")), Term::Nil);
        assert!(decode_result(Term::Map(open), request_hash).is_err());

        let mut substituted = match valid {
            Term::Map(fields) => fields,
            _ => BTreeMap::new(),
        };
        substituted.insert(
            TermOrdKey(Term::symbol(":bytes")),
            Term::Bytes(b"version = 3\n".to_vec().into()),
        );
        assert!(decode_result(Term::Map(substituted), request_hash).is_err());
    }

    #[test]
    fn decoder_rejects_malformed_rejection_shape() {
        let request_hash = [9_u8; 32];
        let mut malformed = match valid_result(request_hash, b"version = 2\n") {
            Term::Map(fields) => fields,
            _ => BTreeMap::new(),
        };
        malformed.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(false));
        malformed.insert(
            TermOrdKey(Term::symbol(":code")),
            Term::Str("core/pkg/bad-payload".to_string()),
        );
        malformed.insert(
            TermOrdKey(Term::symbol(":message")),
            Term::Str("bad".to_string()),
        );
        assert!(decode_result(Term::Map(malformed), request_hash).is_err());
    }

    #[test]
    fn artifact_authority_matches_legacy_canonical_writer() {
        let mut authority = PkgLockWriteAuthority::load(&artifact_config()).expect("authority");
        let decision = authority
            .write(&full_payload(Term::symbol(":tag-policy")))
            .expect("decision");
        let PkgLockWriteDecision::Write { bytes, lock_hash } = decision else {
            panic!("valid payload must authorize a write: {decision:?}");
        };

        let mut expected = gc_pkg::GenesisLock::empty("demo");
        expected.policy = "policy:test".to_string();
        expected
            .registries
            .insert("default".to_string(), "https://example.invalid".to_string());
        expected.requirements.insert(
            "dep".to_string(),
            gc_pkg::Requirement {
                selector: "semver:^1".to_string(),
                update_policy: gc_pkg::UpdatePolicy::Manual,
                registry: Some("default".to_string()),
                strategy: gc_pkg::ResolutionStrategy::TagPolicy,
                tag_policy: Some("^1".to_string()),
            },
        );
        expected.locked.insert(
            "dep".to_string(),
            gc_pkg::LockedEntry {
                commit: Some("a".repeat(64)),
                snapshot: "d".repeat(64),
                registry: Some("default".to_string()),
                source_selector: "semver:^1".to_string(),
                resolved_ref: Some("refs/tags/v1.0.0".to_string()),
                exports_hash: Some("b".repeat(64)),
                environment_fingerprint: Some("env".to_string()),
            },
        );
        expected
            .artifacts
            .insert("demo".to_string(), "c".repeat(64));
        let expected_bytes = expected.to_toml_canonical().into_bytes();
        assert_eq!(bytes, expected_bytes);
        assert_eq!(
            lock_hash,
            blake3::hash(&expected_bytes).to_hex().to_string()
        );
    }

    #[test]
    fn artifact_authority_serializes_typed_resolution_model() {
        let mut authority = PkgLockWriteAuthority::load(&artifact_config()).expect("authority");
        let mut model = gc_pkg::GenesisLock::empty("typed-workspace");
        model
            .artifacts
            .insert("resolution".to_string(), "e".repeat(64));
        let decision = authority
            .write_model("genesis.lock", &model)
            .expect("decision");
        let PkgLockWriteDecision::Write { bytes, lock_hash } = decision else {
            panic!("typed model must authorize a write: {decision:?}");
        };
        let expected = model.to_toml_canonical().into_bytes();
        assert_eq!(bytes, expected);
        assert_eq!(lock_hash, blake3::hash(&bytes).to_hex().to_string());
    }

    #[test]
    fn artifact_authority_preserves_symbol_string_strategy_distinction() {
        let mut authority = PkgLockWriteAuthority::load(&artifact_config()).expect("authority");
        let decision = authority
            .write(&full_payload(Term::Str(":tag-policy".to_string())))
            .expect("decision");
        assert!(matches!(decision, PkgLockWriteDecision::Error { .. }));
    }
}
