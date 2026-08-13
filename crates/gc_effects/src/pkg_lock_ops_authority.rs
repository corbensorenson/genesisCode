use super::*;

pub(super) const OPS_BINDING: &str = "core/pkg::lock-ops-authority";
const OPS_REQUEST_KIND: &str = "genesis/pkg-lock-ops-authority-request-v0.1";
const OPS_RESULT_KIND: &str = "genesis/pkg-lock-ops-authority-result-v0.1";

#[derive(Debug)]
pub(crate) enum PkgLockOpsDecision {
    Write { bytes: Vec<u8>, lock_hash: String },
    List { locked: Term, requirements: Term },
    Error { code: String, message: String },
}

pub(crate) struct PkgBridgeLockFacts<'a> {
    pub(crate) dep: &'a str,
    pub(crate) registry: Option<&'a str>,
    pub(crate) commit: &'a str,
    pub(crate) snapshot: &'a str,
    pub(crate) provenance_root: &'a str,
    pub(crate) conversion_evidence: &'a str,
    pub(crate) attestation: &'a str,
}

impl PkgLockReadAuthority {
    pub(crate) fn init_lock(&mut self, payload: &Term) -> Result<PkgLockOpsDecision, EffectsError> {
        self.apply_lock_op(":init", Term::Nil, payload.clone())
    }

    pub(crate) fn add_lock_toml(
        &mut self,
        bytes: &[u8],
        payload: &Term,
    ) -> Result<PkgLockOpsDecision, EffectsError> {
        let Some(document) = decode_toml_document(bytes)? else {
            return Ok(PkgLockOpsDecision::Error {
                code: "core/pkg/bad-lock".to_string(),
                message: "lock file is not valid UTF-8 TOML".to_string(),
            });
        };
        self.apply_lock_op(":add", document, payload.clone())
    }

    pub(crate) fn list_lock_toml(
        &mut self,
        bytes: &[u8],
        payload: &Term,
    ) -> Result<PkgLockOpsDecision, EffectsError> {
        let Some(document) = decode_toml_document(bytes)? else {
            return Ok(PkgLockOpsDecision::Error {
                code: "core/pkg/bad-lock".to_string(),
                message: "lock file is not valid UTF-8 TOML".to_string(),
            });
        };
        self.apply_lock_op(":list", document, payload.clone())
    }

    pub(crate) fn bridge_lock_toml(
        &mut self,
        bytes: &[u8],
        facts: PkgBridgeLockFacts<'_>,
    ) -> Result<PkgLockOpsDecision, EffectsError> {
        let Some(document) = decode_toml_document(bytes)? else {
            return Ok(PkgLockOpsDecision::Error {
                code: "core/pkg/bad-lock".to_string(),
                message: "lock file is not valid UTF-8 TOML".to_string(),
            });
        };
        let payload = map([
            (":attestation", Term::Str(facts.attestation.to_string())),
            (":commit", Term::Str(facts.commit.to_string())),
            (
                ":conversion-evidence",
                Term::Str(facts.conversion_evidence.to_string()),
            ),
            (":dep", Term::Str(facts.dep.to_string())),
            (
                ":provenance-root",
                Term::Str(facts.provenance_root.to_string()),
            ),
            (
                ":registry",
                facts
                    .registry
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (":snapshot", Term::Str(facts.snapshot.to_string())),
        ]);
        self.apply_lock_op(":bridge-lock", document, payload)
    }

    fn apply_lock_op(
        &mut self,
        operation: &'static str,
        document: Term,
        payload: Term,
    ) -> Result<PkgLockOpsDecision, EffectsError> {
        let request = map([
            (":document", document),
            (":kind", Term::Str(OPS_REQUEST_KIND.to_string())),
            (":op", Term::symbol(operation)),
            (":payload", payload),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let authority = self
            .ops_authority
            .clone()
            .ok_or_else(|| authority_error(format!("missing binding {OPS_BINDING}")))?;
        let value = authority
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("lock ops apply failed: {error}")))?;
        decode_ops_result(plain_result(value, &self.context)?, request_hash, operation)
    }
}

fn decode_toml_document(bytes: &[u8]) -> Result<Option<Term>, EffectsError> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Ok(None);
    };
    let Ok(document) = toml::from_str::<toml::Value>(text) else {
        return Ok(None);
    };
    Ok(Some(toml_to_term(document)))
}

fn decode_ops_result(
    term: Term,
    request_hash: [u8; 32],
    operation: &str,
) -> Result<PkgLockOpsDecision, EffectsError> {
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
            ":value",
        ],
    )?;
    require_string(fields, ":kind", OPS_RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if !required_bool(fields, ":ok")? {
        require_nil(fields, ":bytes")?;
        require_nil(fields, ":lock-h")?;
        require_nil(fields, ":value")?;
        let code = required_string(fields, ":code")?;
        if !matches!(
            code,
            "core/pkg/bad-authority-request" | "core/pkg/bad-lock" | "core/pkg/bad-payload"
        ) {
            return Err(authority_error(
                "lock ops result :code is outside the closed rejection inventory",
            ));
        }
        return Ok(PkgLockOpsDecision::Error {
            code: code.to_string(),
            message: required_string(fields, ":message")?.to_string(),
        });
    }

    require_nil(fields, ":code")?;
    require_nil(fields, ":message")?;
    match operation {
        ":init" | ":add" | ":bridge-lock" => {
            require_nil(fields, ":value")?;
            let bytes = required_bytes(fields, ":bytes")?;
            std::str::from_utf8(&bytes)
                .map_err(|_| authority_error("lock ops :bytes must be canonical UTF-8 TOML"))?;
            let lock_hash = required_string(fields, ":lock-h")?.to_string();
            if !is_hash(&lock_hash) || blake3::hash(&bytes).to_hex().as_str() != lock_hash {
                return Err(authority_error(
                    "lock ops :bytes and :lock-h are malformed or contradictory",
                ));
            }
            Ok(PkgLockOpsDecision::Write { bytes, lock_hash })
        }
        ":list" => {
            require_nil(fields, ":bytes")?;
            require_nil(fields, ":lock-h")?;
            let value = exact_map(field(fields, ":value")?, &[":locked", ":requirements"])?;
            validate_list_entries(field(value, ":locked")?, true)?;
            validate_list_entries(field(value, ":requirements")?, false)?;
            Ok(PkgLockOpsDecision::List {
                locked: field(value, ":locked")?.clone(),
                requirements: field(value, ":requirements")?.clone(),
            })
        }
        _ => Err(authority_error("unknown requested lock operation")),
    }
}

fn validate_list_entries(term: &Term, locked: bool) -> Result<(), EffectsError> {
    let Term::Vector(entries) = term else {
        return Err(authority_error("lock ops list result must contain vectors"));
    };
    for entry in entries {
        let fields = if locked {
            exact_map(
                entry,
                &[":commit", ":environment-fingerprint", ":name", ":snapshot"],
            )?
        } else {
            exact_map(
                entry,
                &[
                    ":name",
                    ":registry",
                    ":selector",
                    ":strategy",
                    ":tag-policy",
                    ":update-policy",
                ],
            )?
        };
        required_string(fields, ":name")?;
        if locked {
            required_string(fields, ":snapshot")?;
            optional_string(fields, ":commit")?;
            optional_string(fields, ":environment-fingerprint")?;
        } else {
            required_string(fields, ":selector")?;
            optional_string(fields, ":registry")?;
            optional_string(fields, ":tag-policy")?;
            require_closed_symbol(
                fields,
                ":strategy",
                &[":pinned", ":track-ref", ":tag-policy"],
            )?;
            require_closed_symbol(fields, ":update-policy", &[":manual", ":auto"])?;
        }
    }
    Ok(())
}

fn require_closed_symbol(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    allowed: &[&str],
) -> Result<(), EffectsError> {
    match field(fields, name)? {
        Term::Symbol(value) if allowed.contains(&value.as_str()) => Ok(()),
        _ => Err(authority_error(format!(
            "lock ops result {name} is outside its closed symbol inventory"
        ))),
    }
}

fn required_bytes(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Vec<u8>, EffectsError> {
    match field(fields, name)? {
        Term::Bytes(value) => Ok(value.to_vec()),
        _ => Err(authority_error(format!(
            "lock ops result {name} must be bytes"
        ))),
    }
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact_config() -> SelfhostAuthorityConfig {
        let artifact = std::env::var_os("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/toolchain.gc")
            })
            .canonicalize()
            .expect("canonical selfhost artifact path");
        SelfhostAuthorityConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        }
    }

    fn payload(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
        map(entries)
    }

    #[test]
    fn add_and_list_match_legacy_lock_behavior() {
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        let mut legacy = gc_pkg::GenesisLock::empty("demo".to_string());
        legacy.policy = "policy:test".to_string();
        legacy
            .registries
            .insert("default".to_string(), "https://example.invalid".to_string());
        let bytes = legacy.to_toml_canonical().into_bytes();
        let PkgLockModelDecision::Lock(model) = authority.read_model_toml(&bytes).unwrap() else {
            panic!("expected normalized model");
        };
        assert_eq!(model.registries, legacy.registries);

        let add = payload([
            (":lock", Term::Str("genesis.lock".to_string())),
            (":name", Term::Str("dep".to_string())),
            (":selector", Term::Str("semver:^1".to_string())),
            (":strategy", Term::symbol(":tag-policy")),
            (":tag-policy", Term::Nil),
            (":update-policy", Term::Str("auto".to_string())),
        ]);
        let PkgLockOpsDecision::Write { bytes, .. } =
            authority.add_lock_toml(&bytes, &add).unwrap()
        else {
            panic!("expected add write");
        };
        legacy.set_requirement_with_metadata(
            "dep",
            "semver:^1",
            gc_pkg::UpdatePolicy::Auto,
            None,
            Some(gc_pkg::ResolutionStrategy::TagPolicy),
            None,
        );
        assert_eq!(bytes, legacy.to_toml_canonical().into_bytes());

        let PkgLockOpsDecision::List {
            locked,
            requirements,
        } = authority.list_lock_toml(&bytes, &payload([])).unwrap()
        else {
            panic!("expected list value");
        };
        assert!(matches!(locked, Term::Vector(values) if values.is_empty()));
        assert!(matches!(requirements, Term::Vector(values) if values.len() == 1));
    }

    #[test]
    fn init_preserves_registry_and_matches_legacy_lock_behavior() {
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        let init = payload([
            (":lock", Term::Str("genesis.lock".to_string())),
            (":workspace", Term::Str("demo".to_string())),
            (":policy", Term::Str("policy:test".to_string())),
            (
                ":registry-default",
                Term::Str("https://example.invalid".to_string()),
            ),
        ]);
        let PkgLockOpsDecision::Write { bytes, lock_hash } = authority.init_lock(&init).unwrap()
        else {
            panic!("expected init write");
        };

        let mut legacy = gc_pkg::GenesisLock::empty("demo".to_string());
        legacy.policy = "policy:test".to_string();
        legacy
            .registries
            .insert("default".to_string(), "https://example.invalid".to_string());
        let expected = legacy.to_toml_canonical().into_bytes();
        assert_eq!(bytes, expected);
        assert_eq!(lock_hash, blake3::hash(&expected).to_hex().to_string());

        let defaults = payload([
            (":workspace", Term::Str("defaults".to_string())),
            (":policy", Term::Bool(false)),
            (":registry-default", Term::Int(7.into())),
        ]);
        let PkgLockOpsDecision::Write { bytes, .. } = authority.init_lock(&defaults).unwrap()
        else {
            panic!("expected defaulted init write");
        };
        assert_eq!(
            bytes,
            gc_pkg::GenesisLock::empty("defaults")
                .to_toml_canonical()
                .into_bytes()
        );
    }

    #[test]
    fn bridge_lock_matches_legacy_behavior_for_unicode_dependency_names() {
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        let dep = "dep/é.包";
        let commit = "a".repeat(64);
        let snapshot = "b".repeat(64);
        let provenance_root = "c".repeat(64);
        let conversion_evidence = "d".repeat(64);
        let attestation = "e".repeat(64);
        let mut legacy = gc_pkg::GenesisLock::empty("demo");
        let source = legacy.to_toml_canonical().into_bytes();

        let PkgLockOpsDecision::Write { bytes, lock_hash } = authority
            .bridge_lock_toml(
                &source,
                PkgBridgeLockFacts {
                    dep,
                    registry: Some("upstream"),
                    commit: &commit,
                    snapshot: &snapshot,
                    provenance_root: &provenance_root,
                    conversion_evidence: &conversion_evidence,
                    attestation: &attestation,
                },
            )
            .unwrap()
        else {
            panic!("expected bridge lock write");
        };

        let selector = format!("commit:{commit}");
        legacy.set_requirement_with_metadata(
            dep,
            &selector,
            gc_pkg::UpdatePolicy::Manual,
            Some("upstream".to_string()),
            Some(gc_pkg::ResolutionStrategy::Pinned),
            None,
        );
        legacy.locked.insert(
            dep.to_string(),
            gc_pkg::LockedEntry {
                commit: Some(commit.clone()),
                snapshot: snapshot.clone(),
                registry: Some("upstream".to_string()),
                source_selector: selector,
                resolved_ref: None,
                exports_hash: None,
                environment_fingerprint: None,
            },
        );
        let dep_fragment: String = dep
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        let dep_hash = blake3::hash(dep.as_bytes()).to_hex().to_string();
        let dep_key = format!("{dep_fragment}_{}", &dep_hash[..8]);
        legacy
            .artifacts
            .insert(format!("bridge_{dep_key}_provenance_root"), provenance_root);
        legacy.artifacts.insert(
            format!("bridge_{dep_key}_conversion_evidence"),
            conversion_evidence,
        );
        legacy
            .artifacts
            .insert(format!("bridge_{dep_key}_attestation"), attestation);
        legacy
            .artifacts
            .insert(format!("bridge_{dep_key}_commit"), commit);
        let expected = legacy.to_toml_canonical().into_bytes();
        assert_eq!(bytes, expected);
        assert_eq!(lock_hash, blake3::hash(&expected).to_hex().to_string());
    }

    #[test]
    fn malformed_inputs_reject_without_host_fallback() {
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        assert!(matches!(
            authority.init_lock(&payload([])).unwrap(),
            PkgLockOpsDecision::Error { ref code, .. } if code == "core/pkg/bad-payload"
        ));
        assert!(matches!(
            authority
                .add_lock_toml(b"not = [toml", &payload([]))
                .unwrap(),
            PkgLockOpsDecision::Error { .. }
        ));
        let source = gc_pkg::GenesisLock::empty("demo")
            .to_toml_canonical()
            .into_bytes();
        assert!(matches!(
            authority
                .bridge_lock_toml(
                    &source,
                    PkgBridgeLockFacts {
                        dep: "dep",
                        registry: None,
                        commit: &"A".repeat(64),
                        snapshot: &"b".repeat(64),
                        provenance_root: &"c".repeat(64),
                        conversion_evidence: &"d".repeat(64),
                        attestation: &"e".repeat(64),
                    },
                )
                .unwrap(),
            PkgLockOpsDecision::Error { ref code, .. } if code == "core/pkg/bad-payload"
        ));
        assert!(matches!(
            authority
                .list_lock_toml(b"version = 3\nworkspace = \"x\"\n", &payload([]))
                .unwrap(),
            PkgLockOpsDecision::Error { .. }
        ));
    }

    #[test]
    fn decoder_rejects_open_unbound_and_contradictory_results() {
        let request_hash = [9_u8; 32];
        let base = map([
            (":bytes", Term::Bytes(b"x".to_vec().into())),
            (":code", Term::Nil),
            (":kind", Term::Str(OPS_RESULT_KIND.to_string())),
            (":lock-h", Term::Str("0".repeat(64))),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":request-h", Term::Str(hex32(request_hash))),
            (":v", Term::Int(1.into())),
            (":value", Term::Nil),
        ]);
        assert!(decode_ops_result(base.clone(), request_hash, ":add").is_err());
        let Term::Map(mut open) = base.clone() else {
            return;
        };
        open.insert(TermOrdKey(Term::symbol(":extra")), Term::Nil);
        assert!(decode_ops_result(Term::Map(open), request_hash, ":add").is_err());
        let Term::Map(mut unbound) = base else {
            return;
        };
        unbound.insert(
            TermOrdKey(Term::symbol(":request-h")),
            Term::Str("1".repeat(64)),
        );
        assert!(decode_ops_result(Term::Map(unbound), request_hash, ":add").is_err());
    }
}
