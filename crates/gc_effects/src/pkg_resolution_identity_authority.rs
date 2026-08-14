use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_with_mode};

use crate::EffectsError;
use crate::policy::SelfhostAuthorityConfig;

#[path = "pkg_resolution_plan_authority.rs"]
mod plan;
pub(crate) use plan::{
    PkgResolutionPlan, PkgResolutionPlanError, PkgResolutionSelector, SemverSelectionPolicy,
};
#[path = "pkg_semver_select_authority.rs"]
mod semver_select;
pub(crate) use semver_select::PkgSemverCandidate;
#[path = "pkg_install_authority.rs"]
mod install;
#[path = "pkg_resolution_workflow_authority.rs"]
mod workflow;
pub(crate) use install::{
    PkgInstallHashObservation, PkgInstallObservation, PkgInstallPlanDecision, PkgInstallResolution,
};
#[cfg(any(test, feature = "parity-oracle"))]
pub(crate) use workflow::PkgWorkflowObject;
pub(crate) use workflow::{
    PkgResolutionWorkflow, PkgWorkflowAction, PkgWorkflowDecision, PkgWorkflowFinalized,
    PkgWorkflowObservation, PkgWorkflowPlan, PkgWorkflowStep,
};

const IDENTITY_BINDING: &str = "core/pkg::resolution-identity-authority";
const IDENTITY_REQUEST_KIND: &str = "genesis/pkg-resolution-identity-request-v0.1";
const IDENTITY_RESULT_KIND: &str = "genesis/pkg-resolution-identity-result-v0.1";
const PLAN_BINDING: &str = "core/pkg::resolution-plan-authority";
const SEMVER_SELECT_BINDING: &str = "core/pkg::semver-select-authority";
const WORKFLOW_BINDING: &str = "core/pkg::resolution-workflow-authority";
const INSTALL_BINDING: &str = "core/pkg::install-authority";
const STEP_LIMIT: u64 = 20_000_000;
const ALLOC_LIMIT: u64 = 40_000_000;

pub(crate) struct PkgResolutionIdentityAuthority {
    context: EvalCtx,
    identity_authority: Value,
    plan_authority: Value,
    semver_select_authority: Value,
    workflow_authority: Value,
    install_authority: Value,
}

impl PkgResolutionIdentityAuthority {
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
        let identity_authority = environment
            .get(IDENTITY_BINDING)
            .ok_or_else(|| authority_error(format!("missing binding {IDENTITY_BINDING}")))?;
        let plan_authority = environment
            .get(PLAN_BINDING)
            .ok_or_else(|| authority_error(format!("missing binding {PLAN_BINDING}")))?;
        let semver_select_authority = environment
            .get(SEMVER_SELECT_BINDING)
            .ok_or_else(|| authority_error(format!("missing binding {SEMVER_SELECT_BINDING}")))?;
        let workflow_authority = environment
            .get(WORKFLOW_BINDING)
            .ok_or_else(|| authority_error(format!("missing binding {WORKFLOW_BINDING}")))?;
        let install_authority = environment
            .get(INSTALL_BINDING)
            .ok_or_else(|| authority_error(format!("missing binding {INSTALL_BINDING}")))?;
        context.reset_counters();
        context.step_limit = Some(STEP_LIMIT);
        Ok(Self {
            context,
            identity_authority,
            plan_authority,
            semver_select_authority,
            workflow_authority,
            install_authority,
        })
    }

    pub(crate) fn fingerprint(
        &mut self,
        requirement: &gc_pkg::Requirement,
        snapshot: Option<&str>,
        commit: Option<&str>,
    ) -> Result<String, EffectsError> {
        let request = map([
            (
                ":commit",
                commit
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (":kind", Term::Str(IDENTITY_REQUEST_KIND.to_string())),
            (":op", Term::symbol(":requirement-fingerprint")),
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
                ":snapshot",
                snapshot
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
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
            (
                ":update-policy",
                Term::symbol(match requirement.update_policy {
                    gc_pkg::UpdatePolicy::Manual => ":manual",
                    gc_pkg::UpdatePolicy::Auto => ":auto",
                }),
            ),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .identity_authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("apply failed: {error}")))?;
        decode_identity_result(plain_result(value, &self.context)?, request_hash)
    }
}

fn decode_identity_result(term: Term, request_hash: [u8; 32]) -> Result<String, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":code",
            ":fingerprint",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
        ],
    )?;
    require_string(fields, ":kind", IDENTITY_RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if required_bool(fields, ":ok")? {
        require_nil(fields, ":code")?;
        require_nil(fields, ":message")?;
        let fingerprint = required_string(fields, ":fingerprint")?.to_string();
        if !is_hash(&fingerprint) {
            return Err(authority_error(
                "result :fingerprint must be lowercase BLAKE3 hex64",
            ));
        }
        Ok(fingerprint)
    } else {
        require_nil(fields, ":fingerprint")?;
        Err(authority_error(format!(
            "authority rejected typed request: {}: {}",
            required_string(fields, ":code")?,
            required_string(fields, ":message")?
        )))
    }
}

fn authority_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!(
        "selfhost package resolution identity authority: {}",
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
        return Err(authority_error(format!(
            "result field set mismatch: actual={actual:?} expected={wanted:?}"
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
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let artifact = std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
            .map(std::path::PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    workspace.join(path)
                }
            })
            .unwrap_or_else(|| workspace.join("selfhost/toolchain.gc"))
            .canonicalize()
            .expect("canonical selfhost artifact path");
        SelfhostAuthorityConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        }
    }

    fn requirement() -> gc_pkg::Requirement {
        gc_pkg::Requirement {
            selector: "semver:^1.2".to_string(),
            update_policy: gc_pkg::UpdatePolicy::Auto,
            registry: Some("default".to_string()),
            strategy: gc_pkg::ResolutionStrategy::TagPolicy,
            tag_policy: Some("lowest".to_string()),
        }
    }

    fn legacy_fingerprint(
        requirement: &gc_pkg::Requirement,
        snapshot: Option<&str>,
        commit: Option<&str>,
    ) -> String {
        let identity = map([
            (
                ":commit",
                commit
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
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
                ":snapshot",
                snapshot
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
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
            (":update-policy", Term::symbol(":auto")),
        ]);
        blake3::hash((print_term(&identity) + "\n").as_bytes())
            .to_hex()
            .to_string()
    }

    #[test]
    fn fingerprint_matches_legacy_identity() {
        let requirement = requirement();
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let actual = authority
            .fingerprint(&requirement, Some("snapshot"), Some("commit"))
            .unwrap();
        assert_eq!(
            actual,
            legacy_fingerprint(&requirement, Some("snapshot"), Some("commit"))
        );
    }

    #[test]
    fn decoder_rejects_open_and_unbound_results() {
        let request_hash = [9_u8; 32];
        let base = map([
            (":code", Term::Nil),
            (":fingerprint", Term::Str("a".repeat(64))),
            (":kind", Term::Str(IDENTITY_RESULT_KIND.to_string())),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":request-h", Term::Str(hex32(request_hash))),
            (":v", Term::Int(1.into())),
        ]);
        let mut open = match base.clone() {
            Term::Map(fields) => fields,
            _ => return,
        };
        open.insert(TermOrdKey(Term::symbol(":extra")), Term::Nil);
        assert!(decode_identity_result(Term::Map(open), request_hash).is_err());
        let mut unbound = match base {
            Term::Map(fields) => fields,
            _ => return,
        };
        unbound.insert(
            TermOrdKey(Term::symbol(":request-h")),
            Term::Str("0".repeat(64)),
        );
        assert!(decode_identity_result(Term::Map(unbound), request_hash).is_err());
    }

    #[test]
    fn authority_rejects_optional_field_replaced_by_unknown_field() {
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let request = map([
            (":extra", Term::Nil),
            (":kind", Term::Str(IDENTITY_REQUEST_KIND.to_string())),
            (":op", Term::symbol(":requirement-fingerprint")),
            (":registry", Term::Nil),
            (":selector", Term::Str("snapshot:a".to_string())),
            (":snapshot", Term::Str("a".to_string())),
            (":strategy", Term::symbol(":pinned")),
            (":tag-policy", Term::Nil),
            (":update-policy", Term::symbol(":manual")),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        let result = authority
            .identity_authority
            .clone()
            .apply(&mut authority.context, Value::data(request))
            .unwrap();
        assert!(
            decode_identity_result(
                plain_result(result, &authority.context).unwrap(),
                request_hash
            )
            .is_err()
        );
    }
}
