use std::collections::BTreeMap;

use num_traits::ToPrimitive;

use super::*;
use crate::pkg_lock_write_authority::locked_entry_payload;

const REQUEST_KIND: &str = "genesis/pkg-resolution-workflow-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-resolution-workflow-result-v0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PkgResolutionWorkflow {
    Lock,
    Update,
}

impl PkgResolutionWorkflow {
    fn term(self) -> Term {
        Term::symbol(match self {
            Self::Lock => ":lock",
            Self::Update => ":update",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PkgWorkflowAction {
    Resolve,
    Consider,
    SkipUnselected,
    MissingRequirement,
}

#[derive(Debug, Clone)]
pub(crate) struct PkgWorkflowStep {
    pub(crate) action: PkgWorkflowAction,
    pub(crate) name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PkgWorkflowPlan {
    pub(crate) hash: String,
    pub(crate) model: Term,
    pub(crate) steps: Vec<PkgWorkflowStep>,
    pub(crate) term: Term,
}

#[derive(Debug, Clone)]
pub(crate) struct PkgWorkflowObservation {
    pub(crate) name: String,
    pub(crate) resolved: Option<gc_pkg::LockedEntry>,
    pub(crate) should_resolve: Option<bool>,
}

#[derive(Debug, Clone)]
pub(crate) struct PkgWorkflowObject {
    pub(crate) bytes: Vec<u8>,
    pub(crate) hash: String,
}

#[derive(Debug)]
pub(crate) struct PkgWorkflowFinalized {
    pub(crate) locked: BTreeMap<String, gc_pkg::LockedEntry>,
    pub(crate) locked_count: u64,
    pub(crate) provenance: Vec<Term>,
    pub(crate) rationale: Vec<Term>,
    pub(crate) rationale_object: PkgWorkflowObject,
    pub(crate) selected_count: u64,
    pub(crate) updated_count: u64,
    pub(crate) workspace_object: PkgWorkflowObject,
}

#[derive(Debug)]
pub(crate) enum PkgWorkflowDecision<T> {
    Accept(T),
    Error { code: String, message: String },
}

impl PkgResolutionIdentityAuthority {
    pub(crate) fn plan_workflow(
        &mut self,
        model: Term,
        workflow: PkgResolutionWorkflow,
        only: &[String],
    ) -> Result<PkgWorkflowDecision<PkgWorkflowPlan>, EffectsError> {
        let planned_model = model.clone();
        let request = map([
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":model", model),
            (":only", string_vector(only)),
            (":op", Term::symbol(":plan")),
            (":v", Term::Int(1.into())),
            (":workflow", workflow.term()),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .workflow_authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("workflow plan apply failed: {error}")))?;
        decode_plan_result(
            plain_result(value, &self.context)?,
            request_hash,
            workflow,
            planned_model,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "closed workflow protocol fields are explicit"
    )]
    pub(crate) fn finalize_workflow(
        &mut self,
        workflow: PkgResolutionWorkflow,
        only: &[String],
        plan: &PkgWorkflowPlan,
        observations: &[PkgWorkflowObservation],
        commit_observations: Vec<Term>,
        strict: bool,
    ) -> Result<PkgWorkflowDecision<PkgWorkflowFinalized>, EffectsError> {
        let request = map([
            (":commit-observations", Term::Vector(commit_observations)),
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":model", plan.model.clone()),
            (":observations", observations_term(observations)),
            (":only", string_vector(only)),
            (":op", Term::symbol(":finalize")),
            (":plan", plan.term.clone()),
            (":plan-h", Term::Str(plan.hash.clone())),
            (":strict", Term::Bool(strict)),
            (":v", Term::Int(1.into())),
            (":workflow", workflow.term()),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .workflow_authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("workflow finalize apply failed: {error}")))?;
        decode_finalize_result(plain_result(value, &self.context)?, request_hash)
    }
}

fn string_vector(values: &[String]) -> Term {
    Term::Vector(values.iter().cloned().map(Term::Str).collect())
}

fn observations_term(observations: &[PkgWorkflowObservation]) -> Term {
    Term::Vector(
        observations
            .iter()
            .map(|observation| {
                map([
                    (":name", Term::Str(observation.name.clone())),
                    (
                        ":resolved",
                        observation
                            .resolved
                            .as_ref()
                            .map(locked_entry_payload)
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        ":should-resolve",
                        observation
                            .should_resolve
                            .map(Term::Bool)
                            .unwrap_or(Term::Nil),
                    ),
                ])
            })
            .collect(),
    )
}

fn decode_envelope(term: &Term, request_hash: [u8; 32]) -> Result<Option<&Term>, EffectsError> {
    let fields = exact_map(
        term,
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
    require_string(fields, ":kind", RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if required_bool(fields, ":ok")? {
        require_nil(fields, ":code")?;
        require_nil(fields, ":message")?;
        Ok(Some(field(fields, ":value")?))
    } else {
        require_nil(fields, ":value")?;
        let code = required_string(fields, ":code")?;
        if code != "core/pkg/bad-authority-request" {
            return Err(authority_error(
                "workflow result :code is outside the closed rejection inventory",
            ));
        }
        Ok(None)
    }
}

fn rejected(term: &Term) -> Result<PkgWorkflowDecision<PkgWorkflowPlan>, EffectsError> {
    let fields = exact_map(
        term,
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
    Ok(PkgWorkflowDecision::Error {
        code: required_string(fields, ":code")?.to_string(),
        message: required_string(fields, ":message")?.to_string(),
    })
}

fn decode_plan_result(
    term: Term,
    request_hash: [u8; 32],
    workflow: PkgResolutionWorkflow,
    model: Term,
) -> Result<PkgWorkflowDecision<PkgWorkflowPlan>, EffectsError> {
    let Some(value) = decode_envelope(&term, request_hash)? else {
        return rejected(&term);
    };
    let value = exact_map(value, &[":plan", ":plan-h"])?;
    let plan_term = field(value, ":plan")?.clone();
    let plan_hash = required_string(value, ":plan-h")?.to_string();
    if !is_hash(&plan_hash) || hex32(hash_term(&plan_term)) != plan_hash {
        return Err(authority_error("workflow plan term and :plan-h contradict"));
    }
    let plan = exact_map(&plan_term, &[":steps", ":workflow"])?;
    let expected_workflow = workflow.term();
    if field(plan, ":workflow")? != &expected_workflow {
        return Err(authority_error(
            "workflow plan :workflow contradicts request",
        ));
    }
    let Term::Vector(step_terms) = field(plan, ":steps")? else {
        return Err(authority_error("workflow plan :steps must be vector"));
    };
    let steps = step_terms
        .iter()
        .map(decode_step)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PkgWorkflowDecision::Accept(PkgWorkflowPlan {
        hash: plan_hash,
        model,
        steps,
        term: plan_term,
    }))
}

fn decode_step(term: &Term) -> Result<PkgWorkflowStep, EffectsError> {
    let fields = exact_map(term, &[":action", ":name", ":requirement"])?;
    let name = required_string(fields, ":name")?.to_string();
    let action = match field(fields, ":action")? {
        Term::Symbol(value) if value == ":resolve" => PkgWorkflowAction::Resolve,
        Term::Symbol(value) if value == ":consider" => PkgWorkflowAction::Consider,
        Term::Symbol(value) if value == ":skip-unselected" => PkgWorkflowAction::SkipUnselected,
        Term::Symbol(value) if value == ":missing-requirement" => {
            PkgWorkflowAction::MissingRequirement
        }
        _ => {
            return Err(authority_error(
                "workflow step :action is outside closed inventory",
            ));
        }
    };
    match action {
        PkgWorkflowAction::MissingRequirement => require_nil(fields, ":requirement")?,
        _ => {
            exact_map(
                field(fields, ":requirement")?,
                &[
                    ":registry",
                    ":selector",
                    ":strategy",
                    ":tag-policy",
                    ":update-policy",
                ],
            )?;
        }
    }
    Ok(PkgWorkflowStep { action, name })
}

fn decode_finalize_result(
    term: Term,
    request_hash: [u8; 32],
) -> Result<PkgWorkflowDecision<PkgWorkflowFinalized>, EffectsError> {
    let Some(value) = decode_envelope(&term, request_hash)? else {
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
        return Ok(PkgWorkflowDecision::Error {
            code: required_string(fields, ":code")?.to_string(),
            message: required_string(fields, ":message")?.to_string(),
        });
    };
    let fields = exact_map(
        value,
        &[
            ":locked",
            ":locked-count",
            ":provenance",
            ":rationale",
            ":rationale-object",
            ":selected-count",
            ":updated-count",
            ":workspace-object",
        ],
    )?;
    let locked = decode_locked(field(fields, ":locked")?)?;
    let locked_count = required_u64(fields, ":locked-count")?;
    if locked_count != locked.len() as u64 {
        return Err(authority_error(
            "workflow :locked-count contradicts :locked",
        ));
    }
    let provenance = required_vector(fields, ":provenance")?.clone();
    let rationale = required_vector(fields, ":rationale")?.clone();
    let rationale_object = decode_object(field(fields, ":rationale-object")?, true)?;
    let workspace_object = decode_object(field(fields, ":workspace-object")?, false)?;
    Ok(PkgWorkflowDecision::Accept(PkgWorkflowFinalized {
        locked,
        locked_count,
        provenance,
        rationale,
        rationale_object,
        selected_count: required_u64(fields, ":selected-count")?,
        updated_count: required_u64(fields, ":updated-count")?,
        workspace_object,
    }))
}

fn decode_locked(term: &Term) -> Result<BTreeMap<String, gc_pkg::LockedEntry>, EffectsError> {
    let Term::Map(entries) = term else {
        return Err(authority_error("workflow :locked must be map"));
    };
    entries
        .iter()
        .map(|(key, value)| {
            let Term::Str(name) = &key.0 else {
                return Err(authority_error("workflow locked key must be string"));
            };
            Ok((name.clone(), decode_locked_entry(value)?))
        })
        .collect()
}

fn decode_locked_entry(term: &Term) -> Result<gc_pkg::LockedEntry, EffectsError> {
    let fields = exact_map(
        term,
        &[
            ":commit",
            ":environment-fingerprint",
            ":exports_hash",
            ":registry",
            ":resolved-ref",
            ":snapshot",
            ":source_selector",
        ],
    )?;
    Ok(gc_pkg::LockedEntry {
        commit: optional_string(fields, ":commit")?,
        environment_fingerprint: optional_string(fields, ":environment-fingerprint")?,
        exports_hash: optional_string(fields, ":exports_hash")?,
        registry: optional_string(fields, ":registry")?,
        resolved_ref: optional_string(fields, ":resolved-ref")?,
        snapshot: required_string(fields, ":snapshot")?.to_string(),
        source_selector: required_string(fields, ":source_selector")?.to_string(),
    })
}

fn decode_object(term: &Term, newline: bool) -> Result<PkgWorkflowObject, EffectsError> {
    let fields = exact_map(term, &[":bytes", ":h", ":term"])?;
    let Term::Bytes(bytes) = field(fields, ":bytes")? else {
        return Err(authority_error("workflow object :bytes must be bytes"));
    };
    let bytes = bytes.to_vec();
    let hash = required_string(fields, ":h")?.to_string();
    let mut expected = print_term(field(fields, ":term")?).into_bytes();
    if newline {
        expected.push(b'\n');
    }
    if bytes != expected || !is_hash(&hash) || blake3::hash(&bytes).to_hex().as_str() != hash {
        return Err(authority_error(
            "workflow object :term, :bytes, and :h are malformed or contradictory",
        ));
    }
    Ok(PkgWorkflowObject { bytes, hash })
}

fn optional_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, EffectsError> {
    match field(fields, name)? {
        Term::Nil => Ok(None),
        Term::Str(value) => Ok(Some(value.clone())),
        _ => Err(authority_error(format!(
            "workflow {name} must be string or nil"
        ))),
    }
}

fn required_vector<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a Vec<Term>, EffectsError> {
    match field(fields, name)? {
        Term::Vector(values) => Ok(values),
        _ => Err(authority_error(format!("workflow {name} must be vector"))),
    }
}

fn required_u64(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<u64, EffectsError> {
    match field(fields, name)? {
        Term::Int(value) => value
            .to_u64()
            .ok_or_else(|| authority_error(format!("workflow {name} must be u64"))),
        _ => Err(authority_error(format!("workflow {name} must be integer"))),
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

    fn requirement(selector: &str, update_policy: gc_pkg::UpdatePolicy) -> gc_pkg::Requirement {
        gc_pkg::Requirement {
            selector: selector.to_string(),
            update_policy,
            registry: None,
            strategy: gc_pkg::ResolutionStrategy::Pinned,
            tag_policy: None,
        }
    }

    fn entry(selector: &str, snapshot: char) -> gc_pkg::LockedEntry {
        gc_pkg::LockedEntry {
            commit: None,
            snapshot: snapshot.to_string().repeat(64),
            registry: None,
            source_selector: selector.to_string(),
            resolved_ref: None,
            exports_hash: None,
            environment_fingerprint: None,
        }
    }

    fn model() -> gc_pkg::GenesisLock {
        gc_pkg::GenesisLock {
            version: 2,
            workspace: "workspace".to_string(),
            policy: "policy:default-v0.1".to_string(),
            registries: BTreeMap::new(),
            requirements: BTreeMap::new(),
            locked: BTreeMap::new(),
            artifacts: BTreeMap::new(),
        }
    }

    fn absent_commit(name: &str) -> Term {
        map([
            (":commit", Term::Nil),
            (":evidence", Term::Vector(Vec::new())),
            (":name", Term::Str(name.to_string())),
            (":obligations", Term::Vector(Vec::new())),
            (":status", Term::symbol(":absent")),
        ])
    }

    #[test]
    fn authority_plans_normalized_only_filter_and_missing_selection() {
        let mut model = model();
        model.requirements.insert(
            "alpha".to_string(),
            requirement(
                &format!("snapshot:{}", "a".repeat(64)),
                gc_pkg::UpdatePolicy::Manual,
            ),
        );
        model.requirements.insert(
            "beta".to_string(),
            requirement(
                &format!("snapshot:{}", "b".repeat(64)),
                gc_pkg::UpdatePolicy::Manual,
            ),
        );
        let payload =
            crate::pkg_lock_write_authority::lock_model_payload("genesis.lock", &model).unwrap();
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let plan = match authority
            .plan_workflow(
                payload,
                PkgResolutionWorkflow::Update,
                &[
                    " beta ".to_string(),
                    "missing".to_string(),
                    "beta".to_string(),
                ],
            )
            .unwrap()
        {
            PkgWorkflowDecision::Accept(plan) => plan,
            PkgWorkflowDecision::Error { code, message } => {
                panic!("unexpected rejection {code}: {message}")
            }
        };
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].name, "alpha");
        assert_eq!(plan.steps[0].action, PkgWorkflowAction::SkipUnselected);
        assert_eq!(plan.steps[1].name, "beta");
        assert_eq!(plan.steps[1].action, PkgWorkflowAction::Consider);
        assert_eq!(plan.steps[2].name, "missing");
        assert_eq!(plan.steps[2].action, PkgWorkflowAction::MissingRequirement);
    }

    #[test]
    fn authority_finalizes_exact_objects_and_rejects_observation_substitution() {
        let selector = format!("snapshot:{}", "a".repeat(64));
        let resolved = entry(&selector, 'a');
        let mut model = model();
        model.requirements.insert(
            "dep".to_string(),
            requirement(&selector, gc_pkg::UpdatePolicy::Manual),
        );
        let payload =
            crate::pkg_lock_write_authority::lock_model_payload("genesis.lock", &model).unwrap();
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let plan = match authority
            .plan_workflow(payload, PkgResolutionWorkflow::Lock, &[])
            .unwrap()
        {
            PkgWorkflowDecision::Accept(plan) => plan,
            PkgWorkflowDecision::Error { code, message } => {
                panic!("unexpected rejection {code}: {message}")
            }
        };
        let accepted = authority
            .finalize_workflow(
                PkgResolutionWorkflow::Lock,
                &[],
                &plan,
                &[PkgWorkflowObservation {
                    name: "dep".to_string(),
                    resolved: Some(resolved.clone()),
                    should_resolve: Some(true),
                }],
                vec![absent_commit("dep")],
                false,
            )
            .unwrap();
        let PkgWorkflowDecision::Accept(finalized) = accepted else {
            panic!("valid finalization rejected")
        };
        assert_eq!(finalized.locked_count, 1);
        assert_eq!(finalized.rationale.len(), 1);
        assert_eq!(finalized.provenance.len(), 1);
        assert_eq!(
            finalized.rationale_object.hash,
            blake3::hash(&finalized.rationale_object.bytes)
                .to_hex()
                .to_string()
        );
        assert_eq!(
            finalized.workspace_object.hash,
            blake3::hash(&finalized.workspace_object.bytes)
                .to_hex()
                .to_string()
        );

        let rejected = authority
            .finalize_workflow(
                PkgResolutionWorkflow::Lock,
                &[],
                &plan,
                &[PkgWorkflowObservation {
                    name: "other".to_string(),
                    resolved: Some(resolved),
                    should_resolve: Some(true),
                }],
                vec![absent_commit("dep")],
                false,
            )
            .unwrap();
        assert!(matches!(
            rejected,
            PkgWorkflowDecision::Error { ref code, .. }
                if code == "core/pkg/bad-authority-request"
        ));
    }

    #[test]
    fn object_decoder_rejects_hash_substitution() {
        let term = Term::Vector(vec![Term::symbol(":x")]);
        let object = map([
            (":bytes", Term::Bytes(print_term(&term).into_bytes().into())),
            (":h", Term::Str("0".repeat(64))),
            (":term", term),
        ]);
        assert!(decode_object(&object, false).is_err());
    }
}
