use super::*;

pub(super) const MODEL_BINDING: &str = "core/pkg::lock-model-authority";
const MODEL_REQUEST_KIND: &str = "genesis/pkg-lock-model-authority-request-v0.1";
const MODEL_RESULT_KIND: &str = "genesis/pkg-lock-model-authority-result-v0.1";

#[derive(Debug)]
pub(crate) enum PkgLockModelDecision {
    Lock(gc_pkg::GenesisLock),
    Error { code: String, message: String },
}

impl PkgLockReadAuthority {
    pub(crate) fn read_model_toml(
        &mut self,
        bytes: &[u8],
    ) -> Result<PkgLockModelDecision, EffectsError> {
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                return Ok(PkgLockModelDecision::Error {
                    code: "core/pkg/bad-lock".to_string(),
                    message: "lock file is not UTF-8".to_string(),
                });
            }
        };
        let document: toml::Value = match toml::from_str(text) {
            Ok(document) => document,
            Err(_) => {
                return Ok(PkgLockModelDecision::Error {
                    code: "core/pkg/bad-lock".to_string(),
                    message: "lock file is not valid TOML".to_string(),
                });
            }
        };
        let request = map([
            (":document", toml_to_term(document)),
            (":kind", Term::Str(MODEL_REQUEST_KIND.to_string())),
            (":op", Term::symbol(":read-model")),
            (":v", Term::Int(1.into())),
        ]);
        self.read_model_request(request)
    }

    fn read_model_request(&mut self, request: Term) -> Result<PkgLockModelDecision, EffectsError> {
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let authority = self
            .model_authority
            .clone()
            .ok_or_else(|| authority_error(format!("missing binding {MODEL_BINDING}")))?;
        let value = authority
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("model apply failed: {error}")))?;
        decode_model_result(plain_result(value, &self.context)?, request_hash)
    }
}

fn decode_model_result(
    term: Term,
    request_hash: [u8; 32],
) -> Result<PkgLockModelDecision, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":code",
            ":kind",
            ":message",
            ":model",
            ":ok",
            ":request-h",
            ":v",
        ],
    )?;
    require_string(fields, ":kind", MODEL_RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if required_bool(fields, ":ok")? {
        require_nil(fields, ":code")?;
        require_nil(fields, ":message")?;
        Ok(PkgLockModelDecision::Lock(decode_model(field(
            fields, ":model",
        )?)?))
    } else {
        require_nil(fields, ":model")?;
        let code = required_string(fields, ":code")?;
        if !matches!(code, "core/pkg/bad-lock" | "core/pkg/bad-authority-request") {
            return Err(authority_error(
                "model result :code is outside the closed rejection inventory",
            ));
        }
        Ok(PkgLockModelDecision::Error {
            code: code.to_string(),
            message: required_string(fields, ":message")?.to_string(),
        })
    }
}

fn decode_model(term: &Term) -> Result<gc_pkg::GenesisLock, EffectsError> {
    let fields = exact_map(
        term,
        &[
            ":artifacts",
            ":locked",
            ":policy",
            ":registries",
            ":requirements",
            ":version",
            ":workspace",
        ],
    )?;
    let version = match field(fields, ":version")? {
        Term::Int(value) if value == &1.into() => 1,
        Term::Int(value) if value == &2.into() => 2,
        _ => return Err(authority_error("model :version must be 1 or 2")),
    };
    Ok(gc_pkg::GenesisLock {
        version,
        workspace: required_string(fields, ":workspace")?.to_string(),
        policy: required_string(fields, ":policy")?.to_string(),
        registries: decode_string_map(field(fields, ":registries")?, ":registries")?,
        requirements: decode_requirements(field(fields, ":requirements")?)?,
        locked: decode_locked(field(fields, ":locked")?)?,
        artifacts: decode_string_map(field(fields, ":artifacts")?, ":artifacts")?,
    })
}

fn decode_string_map(term: &Term, name: &str) -> Result<BTreeMap<String, String>, EffectsError> {
    let Term::Map(entries) = term else {
        return Err(authority_error(format!("model {name} must be map")));
    };
    entries
        .iter()
        .map(|(key, value)| match (&key.0, value) {
            (Term::Str(key), Term::Str(value)) => Ok((key.clone(), value.clone())),
            _ => Err(authority_error(format!(
                "model {name} entries must be string/string"
            ))),
        })
        .collect()
}

fn decode_requirements(term: &Term) -> Result<BTreeMap<String, gc_pkg::Requirement>, EffectsError> {
    let Term::Map(entries) = term else {
        return Err(authority_error("model :requirements must be map"));
    };
    entries
        .iter()
        .map(|(key, value)| {
            let Term::Str(name) = &key.0 else {
                return Err(authority_error("model requirement name must be string"));
            };
            let fields = exact_map(
                value,
                &[
                    ":registry",
                    ":selector",
                    ":strategy",
                    ":tag-policy",
                    ":update-policy",
                ],
            )?;
            let update_policy = match field(fields, ":update-policy")? {
                Term::Symbol(value) if value == ":manual" => gc_pkg::UpdatePolicy::Manual,
                Term::Symbol(value) if value == ":auto" => gc_pkg::UpdatePolicy::Auto,
                _ => {
                    return Err(authority_error(
                        "model requirement update policy is invalid",
                    ));
                }
            };
            let strategy = match field(fields, ":strategy")? {
                Term::Symbol(value) if value == ":pinned" => gc_pkg::ResolutionStrategy::Pinned,
                Term::Symbol(value) if value == ":track-ref" => {
                    gc_pkg::ResolutionStrategy::TrackRef
                }
                Term::Symbol(value) if value == ":tag-policy" => {
                    gc_pkg::ResolutionStrategy::TagPolicy
                }
                _ => return Err(authority_error("model requirement strategy is invalid")),
            };
            Ok((
                name.clone(),
                gc_pkg::Requirement {
                    selector: required_string(fields, ":selector")?.to_string(),
                    update_policy,
                    registry: cloned_optional_string(fields, ":registry")?,
                    strategy,
                    tag_policy: cloned_optional_string(fields, ":tag-policy")?,
                },
            ))
        })
        .collect()
}

fn decode_locked(term: &Term) -> Result<BTreeMap<String, gc_pkg::LockedEntry>, EffectsError> {
    let Term::Map(entries) = term else {
        return Err(authority_error("model :locked must be map"));
    };
    entries
        .iter()
        .map(|(key, value)| {
            let Term::Str(name) = &key.0 else {
                return Err(authority_error("model locked name must be string"));
            };
            let fields = exact_map(
                value,
                &[
                    ":commit",
                    ":environment-fingerprint",
                    ":exports-hash",
                    ":registry",
                    ":resolved-ref",
                    ":snapshot",
                    ":source-selector",
                ],
            )?;
            Ok((
                name.clone(),
                gc_pkg::LockedEntry {
                    commit: cloned_optional_string(fields, ":commit")?,
                    snapshot: required_string(fields, ":snapshot")?.to_string(),
                    registry: cloned_optional_string(fields, ":registry")?,
                    source_selector: required_string(fields, ":source-selector")?.to_string(),
                    resolved_ref: cloned_optional_string(fields, ":resolved-ref")?,
                    exports_hash: cloned_optional_string(fields, ":exports-hash")?,
                    environment_fingerprint: cloned_optional_string(
                        fields,
                        ":environment-fingerprint",
                    )?,
                },
            ))
        })
        .collect()
}

fn cloned_optional_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, EffectsError> {
    match field(fields, name)? {
        Term::Nil => Ok(None),
        Term::Str(value) => Ok(Some(value.clone())),
        _ => Err(authority_error(format!(
            "model {name} must be string or nil"
        ))),
    }
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
    fn model_preserves_resolution_metadata() {
        let source = r#"
version = 2
workspace = "demo"
policy = "policy:test"
[requirements]
dep = { selector = "semver:^1", update_policy = "auto", strategy = "tag-policy", tag_policy = "lowest" }
[locked]
dep = { snapshot = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", source_selector = "semver:^1", environment_fingerprint = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }
"#;
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        let PkgLockModelDecision::Lock(model) =
            authority.read_model_toml(source.as_bytes()).unwrap()
        else {
            panic!("expected model");
        };
        let requirement = model.requirements.get("dep").unwrap();
        assert_eq!(requirement.strategy, gc_pkg::ResolutionStrategy::TagPolicy);
        assert_eq!(requirement.tag_policy.as_deref(), Some("lowest"));
        assert_eq!(
            model.locked["dep"].environment_fingerprint.as_deref(),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
        );
    }

    #[test]
    fn model_rejects_same_cardinality_request_field_substitution() {
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        let request = map([
            (":kind", Term::Str(MODEL_REQUEST_KIND.to_string())),
            (":op", Term::symbol(":read-model")),
            (":unexpected", Term::Map(BTreeMap::new())),
            (":v", Term::Int(1.into())),
        ]);
        let decision = authority.read_model_request(request).unwrap();
        assert!(matches!(
            decision,
            PkgLockModelDecision::Error { ref code, .. }
                if code == "core/pkg/bad-authority-request"
        ));
    }
}
