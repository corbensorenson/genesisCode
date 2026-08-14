use super::*;

pub(super) const SNAPSHOT_BINDING: &str = "core/pkg::snapshot-authority";
const SNAPSHOT_REQUEST_KIND: &str = "genesis/pkg-snapshot-authority-request-v0.1";
const SNAPSHOT_RESULT_KIND: &str = "genesis/pkg-snapshot-authority-result-v0.1";

#[derive(Debug, Clone)]
pub(crate) struct PkgSnapshotObject {
    pub(crate) bytes: Vec<u8>,
    pub(crate) hash: String,
}

#[derive(Debug)]
pub(crate) struct PkgSnapshotPlan {
    pub(crate) artifacts: Vec<PkgSnapshotObject>,
    pub(crate) modules: Vec<Term>,
    pub(crate) snapshot: String,
}

#[derive(Debug)]
pub(crate) enum PkgSnapshotDecision {
    Accept(PkgSnapshotPlan),
    Error { code: String, message: String },
}

impl PkgLockReadAuthority {
    pub(crate) fn construct_snapshot(
        &mut self,
        facts: Term,
    ) -> Result<PkgSnapshotDecision, EffectsError> {
        let request = map([
            (":facts", facts),
            (":kind", Term::Str(SNAPSHOT_REQUEST_KIND.to_string())),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let authority = self
            .snapshot_authority
            .clone()
            .ok_or_else(|| authority_error(format!("missing binding {SNAPSHOT_BINDING}")))?;
        let value = authority
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("snapshot apply failed: {error}")))?;
        decode_snapshot_result(plain_result(value, &self.context)?, request_hash)
    }
}

fn decode_snapshot_result(
    term: Term,
    request_hash: [u8; 32],
) -> Result<PkgSnapshotDecision, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":code",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
            ":value",
        ],
    )?;
    require_string(fields, ":kind", SNAPSHOT_RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if !required_bool(fields, ":ok")? {
        require_nil(fields, ":value")?;
        let code = required_string(fields, ":code")?;
        if !matches!(
            code,
            "core/pkg/bad-authority-request" | "core/pkg/bad-package"
        ) {
            return Err(authority_error(
                "snapshot result :code is outside the closed rejection inventory",
            ));
        }
        return Ok(PkgSnapshotDecision::Error {
            code: code.to_string(),
            message: required_string(fields, ":message")?.to_string(),
        });
    }
    require_nil(fields, ":code")?;
    require_nil(fields, ":message")?;
    let value = exact_map(
        field(fields, ":value")?,
        &[":artifacts", ":modules", ":snapshot"],
    )?;
    let Term::Vector(artifact_terms) = field(value, ":artifacts")? else {
        return Err(authority_error("snapshot result :artifacts must be vector"));
    };
    let Term::Vector(modules) = field(value, ":modules")? else {
        return Err(authority_error("snapshot result :modules must be vector"));
    };
    if artifact_terms.len() != modules.len().saturating_add(1) {
        return Err(authority_error(
            "snapshot result must contain one artifact per module plus the snapshot",
        ));
    }
    let artifacts = artifact_terms
        .iter()
        .map(decode_snapshot_object)
        .collect::<Result<Vec<_>, _>>()?;
    for (index, module) in modules.iter().enumerate() {
        let module = exact_map(module, &[":hash", ":module-h", ":path"])?;
        require_string(module, ":hash", &artifacts[index].hash)?;
        required_string(module, ":path")?;
        let Term::Bytes(module_hash) = field(module, ":module-h")? else {
            return Err(authority_error("snapshot module :module-h must be bytes"));
        };
        if module_hash.len() != 32 {
            return Err(authority_error(
                "snapshot module :module-h must contain 32 bytes",
            ));
        }
    }
    let snapshot = required_string(value, ":snapshot")?.to_string();
    if artifacts.last().map(|artifact| artifact.hash.as_str()) != Some(snapshot.as_str()) {
        return Err(authority_error(
            "snapshot result :snapshot contradicts the final artifact",
        ));
    }
    Ok(PkgSnapshotDecision::Accept(PkgSnapshotPlan {
        artifacts,
        modules: modules.clone(),
        snapshot,
    }))
}

fn decode_snapshot_object(term: &Term) -> Result<PkgSnapshotObject, EffectsError> {
    let fields = exact_map(term, &[":bytes", ":h", ":term"])?;
    let Term::Bytes(bytes) = field(fields, ":bytes")? else {
        return Err(authority_error("snapshot object :bytes must be bytes"));
    };
    let bytes = bytes.to_vec();
    let hash = required_string(fields, ":h")?.to_string();
    let artifact = field(fields, ":term")?.clone();
    if print_term(&artifact).as_bytes() != bytes.as_slice()
        || !is_lower_hash(&hash)
        || blake3::hash(&bytes).to_hex().as_str() != hash
    {
        return Err(authority_error(
            "snapshot object :term, :bytes, and :h are malformed or contradictory",
        ));
    }
    Ok(PkgSnapshotObject { bytes, hash })
}

fn is_lower_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gc_coreform::parse_term;

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

    fn facts(forms: Vec<Term>, module_hash: [u8; 32]) -> Term {
        map([
            (
                ":modules",
                Term::Vector(vec![map([
                    (":module", Term::Vector(forms)),
                    (":module-h", Term::Bytes(module_hash.to_vec().into())),
                    (":path", Term::Str("src/main.gc".to_string())),
                ])]),
            ),
            (":name", Term::Str("demo".to_string())),
            (
                ":obligations",
                Term::Vector(vec![Term::symbol("core/obligation::unit-tests")]),
            ),
            (":pkg", Term::Str("genesis.pkg".to_string())),
            (":version", Term::Str("1.0.0".to_string())),
        ])
    }

    #[test]
    fn authority_constructs_exact_module_and_snapshot_objects() {
        let forms = gc_coreform::parse_module("(quote ok)").unwrap();
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        let plan = match authority
            .construct_snapshot(facts(forms.clone(), gc_coreform::hash_module(&forms)))
            .unwrap()
        {
            PkgSnapshotDecision::Accept(plan) => plan,
            PkgSnapshotDecision::Error { code, message } => {
                panic!("unexpected authority rejection {code}: {message}");
            }
        };
        assert_eq!(plan.artifacts.len(), 2);
        assert_eq!(plan.modules.len(), 1);
        assert_eq!(plan.snapshot, plan.artifacts[1].hash);
        assert_eq!(
            parse_term(std::str::from_utf8(&plan.artifacts[0].bytes).unwrap()).unwrap(),
            Term::Vector(forms)
        );
    }

    #[test]
    fn authority_rejects_module_identity_substitution() {
        let mut authority = PkgLockReadAuthority::load(&artifact_config()).unwrap();
        let forms = gc_coreform::parse_module("(quote ok)").unwrap();
        assert!(matches!(
            authority.construct_snapshot(facts(forms, [0; 32])).unwrap(),
            PkgSnapshotDecision::Error { ref code, .. } if code == "core/pkg/bad-package"
        ));
    }

    #[test]
    fn object_decoder_rejects_bytes_and_hash_substitution() {
        let artifact = Term::Vector(vec![Term::symbol("ok")]);
        let bytes = print_term(&artifact).into_bytes();
        let substituted = map([
            (":bytes", Term::Bytes(bytes.into())),
            (":h", Term::Str("0".repeat(64))),
            (":term", artifact),
        ]);
        assert!(decode_snapshot_object(&substituted).is_err());
    }
}
