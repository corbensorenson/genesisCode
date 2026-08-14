use num_traits::ToPrimitive;

use super::*;

const REQUEST_KIND: &str = "genesis/pkg-install-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-install-result-v0.1";

#[derive(Debug, Clone)]
pub(crate) struct PkgInstallStep {
    pub(crate) commit: Option<String>,
    pub(crate) name: String,
    pub(crate) registry: Option<String>,
    pub(crate) resolve_if_missing: bool,
    pub(crate) snapshot: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PkgInstallPlan {
    pub(crate) frozen: bool,
    pub(crate) hash: String,
    pub(crate) model: Term,
    pub(crate) refs_available: bool,
    pub(crate) steps: Vec<PkgInstallStep>,
    pub(crate) strict: bool,
    pub(crate) term: Term,
}

#[derive(Debug)]
pub(crate) enum PkgInstallPlanDecision {
    Admit(PkgInstallPlan),
    FrozenMissing(Vec<String>),
    Error { code: String, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PkgInstallResolution {
    NotNeeded,
    Resolved,
    NotFound,
    Unavailable,
}

impl PkgInstallResolution {
    fn term(self) -> Term {
        Term::symbol(match self {
            Self::NotNeeded => ":not-needed",
            Self::Resolved => ":resolved",
            Self::NotFound => ":not-found",
            Self::Unavailable => ":unavailable",
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PkgInstallHashObservation {
    pub(crate) hash: String,
    pub(crate) present: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PkgInstallObservation {
    pub(crate) closure_checked: u64,
    pub(crate) commit_present: Option<bool>,
    pub(crate) hashes: Vec<PkgInstallHashObservation>,
    pub(crate) initially_present: bool,
    pub(crate) name: String,
    pub(crate) resolution: PkgInstallResolution,
}

#[derive(Debug)]
pub(crate) struct PkgInstallFinalized {
    pub(crate) report: Term,
}

impl PkgResolutionIdentityAuthority {
    pub(crate) fn plan_install(
        &mut self,
        model: Term,
        frozen: bool,
        strict: bool,
        refs_available: bool,
    ) -> Result<PkgInstallPlanDecision, EffectsError> {
        let retained_model = model.clone();
        let request = map([
            (":frozen", Term::Bool(frozen)),
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":model", model),
            (":op", Term::symbol(":plan")),
            (":refs-available", Term::Bool(refs_available)),
            (":strict", Term::Bool(strict)),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .install_authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("install plan apply failed: {error}")))?;
        decode_plan_result(
            plain_result(value, &self.context)?,
            request_hash,
            retained_model,
            frozen,
            strict,
            refs_available,
        )
    }

    pub(crate) fn finalize_install(
        &mut self,
        plan: &PkgInstallPlan,
        observations: &[PkgInstallObservation],
        commit_observations: Vec<Term>,
    ) -> Result<Result<PkgInstallFinalized, (String, String)>, EffectsError> {
        let request = map([
            (":commit-observations", Term::Vector(commit_observations)),
            (":frozen", Term::Bool(plan.frozen)),
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":model", plan.model.clone()),
            (":observations", observations_term(observations)),
            (":op", Term::symbol(":finalize")),
            (":plan", plan.term.clone()),
            (":plan-h", Term::Str(plan.hash.clone())),
            (":refs-available", Term::Bool(plan.refs_available)),
            (":strict", Term::Bool(plan.strict)),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .install_authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("install finalize apply failed: {error}")))?;
        decode_finalize_result(plain_result(value, &self.context)?, request_hash)
    }
}

fn observations_term(observations: &[PkgInstallObservation]) -> Term {
    Term::Vector(
        observations
            .iter()
            .map(|observation| {
                map([
                    (
                        ":closure-checked",
                        Term::Int(observation.closure_checked.into()),
                    ),
                    (
                        ":commit-present",
                        observation
                            .commit_present
                            .map(Term::Bool)
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        ":hashes",
                        Term::Vector(
                            observation
                                .hashes
                                .iter()
                                .map(|hash| {
                                    map([
                                        (":h", Term::Str(hash.hash.clone())),
                                        (":present", Term::Bool(hash.present)),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                    (
                        ":initially-present",
                        Term::Bool(observation.initially_present),
                    ),
                    (":name", Term::Str(observation.name.clone())),
                    (":resolution", observation.resolution.term()),
                ])
            })
            .collect(),
    )
}

fn decode_envelope<'a>(
    term: &'a Term,
    request_hash: [u8; 32],
) -> Result<Result<&'a Term, (String, String)>, EffectsError> {
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
        Ok(Ok(field(fields, ":value")?))
    } else {
        require_nil(fields, ":value")?;
        let code = required_string(fields, ":code")?.to_string();
        if code != "core/pkg/bad-authority-request" {
            return Err(authority_error(
                "install result :code is outside the closed rejection inventory",
            ));
        }
        Ok(Err((
            code,
            required_string(fields, ":message")?.to_string(),
        )))
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "request facts remain explicit during contradiction checks"
)]
fn decode_plan_result(
    term: Term,
    request_hash: [u8; 32],
    model: Term,
    frozen: bool,
    strict: bool,
    refs_available: bool,
) -> Result<PkgInstallPlanDecision, EffectsError> {
    let value = match decode_envelope(&term, request_hash)? {
        Ok(value) => value,
        Err((code, message)) => return Ok(PkgInstallPlanDecision::Error { code, message }),
    };
    let fields = exact_map(value, &[":admit", ":missing-locks", ":plan", ":plan-h"])?;
    let admit = required_bool(fields, ":admit")?;
    let missing = string_vector(fields, ":missing-locks", false)?;
    let plan_term = field(fields, ":plan")?.clone();
    let plan_hash = required_string(fields, ":plan-h")?.to_string();
    if !is_hash(&plan_hash) || hex32(hash_term(&plan_term)) != plan_hash {
        return Err(authority_error("install plan term and :plan-h contradict"));
    }
    let plan_fields = exact_map(
        &plan_term,
        &[
            ":dependencies",
            ":frozen",
            ":refs-available",
            ":strict",
            ":workspace-root",
        ],
    )?;
    if required_bool(plan_fields, ":frozen")? != frozen
        || required_bool(plan_fields, ":strict")? != strict
        || required_bool(plan_fields, ":refs-available")? != refs_available
    {
        return Err(authority_error("install plan flags contradict request"));
    }
    optional_hash(plan_fields, ":workspace-root")?;
    let Term::Vector(step_terms) = field(plan_fields, ":dependencies")? else {
        return Err(authority_error("install plan :dependencies must be vector"));
    };
    let steps = step_terms
        .iter()
        .map(decode_step)
        .collect::<Result<Vec<_>, _>>()?;
    if !admit {
        if !frozen || missing.is_empty() {
            return Err(authority_error(
                "install rejection contradicts frozen missing-lock facts",
            ));
        }
        return Ok(PkgInstallPlanDecision::FrozenMissing(missing));
    }
    if frozen && !missing.is_empty() {
        return Err(authority_error(
            "admitted frozen install retains missing lock requirements",
        ));
    }
    Ok(PkgInstallPlanDecision::Admit(PkgInstallPlan {
        frozen,
        hash: plan_hash,
        model,
        refs_available,
        steps,
        strict,
        term: plan_term,
    }))
}

fn decode_step(term: &Term) -> Result<PkgInstallStep, EffectsError> {
    let fields = exact_map(
        term,
        &[
            ":commit",
            ":name",
            ":registry",
            ":resolve-if-missing",
            ":snapshot",
        ],
    )?;
    let commit = optional_hash(fields, ":commit")?;
    let registry = optional_string(fields, ":registry")?;
    let snapshot = required_string(fields, ":snapshot")?.to_string();
    if !is_hash(&snapshot) {
        return Err(authority_error("install step :snapshot must be hash"));
    }
    Ok(PkgInstallStep {
        commit,
        name: required_string(fields, ":name")?.to_string(),
        registry,
        resolve_if_missing: required_bool(fields, ":resolve-if-missing")?,
        snapshot,
    })
}

fn decode_finalize_result(
    term: Term,
    request_hash: [u8; 32],
) -> Result<Result<PkgInstallFinalized, (String, String)>, EffectsError> {
    let value = match decode_envelope(&term, request_hash)? {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let fields = exact_map(
        value,
        &[
            ":checked",
            ":lock",
            ":missing",
            ":ok",
            ":provenance",
            ":workspace-root",
        ],
    )?;
    required_u64(fields, ":checked")?;
    required_string(fields, ":lock")?;
    string_vector(fields, ":missing", true)?;
    required_bool(fields, ":ok")?;
    let workspace = optional_hash(fields, ":workspace-root")?;
    let provenance = exact_map(field(fields, ":provenance")?, &[":deps", ":workspace-root"])?;
    let Term::Vector(_) = field(provenance, ":deps")? else {
        return Err(authority_error("install provenance :deps must be vector"));
    };
    if optional_hash(provenance, ":workspace-root")? != workspace {
        return Err(authority_error(
            "install provenance workspace root contradicts report",
        ));
    }
    Ok(Ok(PkgInstallFinalized {
        report: value.clone(),
    }))
}

fn required_u64(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<u64, EffectsError> {
    match field(fields, name)? {
        Term::Int(value) => value
            .to_u64()
            .ok_or_else(|| authority_error(format!("install {name} must be u64"))),
        _ => Err(authority_error(format!("install {name} must be integer"))),
    }
}

fn optional_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, EffectsError> {
    match field(fields, name)? {
        Term::Nil => Ok(None),
        Term::Str(value) => Ok(Some(value.clone())),
        _ => Err(authority_error(format!(
            "install {name} must be string or nil"
        ))),
    }
}

fn optional_hash(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, EffectsError> {
    let value = optional_string(fields, name)?;
    if value.as_deref().is_some_and(|value| !is_hash(value)) {
        return Err(authority_error(format!(
            "install {name} must be lowercase hash or nil"
        )));
    }
    Ok(value)
}

fn string_vector(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    hashes: bool,
) -> Result<Vec<String>, EffectsError> {
    let Term::Vector(values) = field(fields, name)? else {
        return Err(authority_error(format!("install {name} must be vector")));
    };
    values
        .iter()
        .map(|value| match value {
            Term::Str(value) if !hashes || is_hash(value) => Ok(value.clone()),
            _ => Err(authority_error(format!(
                "install {name} contains invalid string"
            ))),
        })
        .collect()
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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

    fn requirement(registry: Option<&str>) -> gc_pkg::Requirement {
        gc_pkg::Requirement {
            selector: "snapshot:".to_string() + &"a".repeat(64),
            update_policy: gc_pkg::UpdatePolicy::Manual,
            registry: registry.map(str::to_string),
            strategy: gc_pkg::ResolutionStrategy::Pinned,
            tag_policy: None,
        }
    }

    fn locked(registry: Option<&str>) -> gc_pkg::LockedEntry {
        gc_pkg::LockedEntry {
            commit: None,
            snapshot: "a".repeat(64),
            registry: registry.map(str::to_string),
            source_selector: "snapshot:".to_string() + &"a".repeat(64),
            resolved_ref: None,
            exports_hash: None,
            environment_fingerprint: None,
        }
    }

    fn model() -> gc_pkg::GenesisLock {
        let mut requirements = BTreeMap::new();
        requirements.insert("dep".to_string(), requirement(Some("requirement-registry")));
        gc_pkg::GenesisLock {
            version: 2,
            workspace: "workspace".to_string(),
            policy: "policy:default-v0.1".to_string(),
            registries: BTreeMap::new(),
            requirements,
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
    fn authority_rejects_frozen_missing_and_owns_registry_precedence() {
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let unlocked = model();
        let unlocked_term =
            crate::pkg_lock_write_authority::lock_model_payload("genesis.lock", &unlocked).unwrap();
        match authority
            .plan_install(unlocked_term, true, false, true)
            .unwrap()
        {
            PkgInstallPlanDecision::FrozenMissing(missing) => {
                assert_eq!(missing, vec!["dep".to_string()]);
            }
            other => panic!("unexpected frozen decision: {other:?}"),
        }

        let mut admitted = model();
        admitted
            .locked
            .insert("dep".to_string(), locked(Some("locked-registry")));
        let admitted_term =
            crate::pkg_lock_write_authority::lock_model_payload("genesis.lock", &admitted).unwrap();
        let PkgInstallPlanDecision::Admit(plan) = authority
            .plan_install(admitted_term, true, true, true)
            .unwrap()
        else {
            panic!("complete frozen install was not admitted");
        };
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].registry.as_deref(), Some("locked-registry"));
        assert!(plan.steps[0].resolve_if_missing);
    }

    #[test]
    fn authority_finalizes_exact_report_and_rejects_observation_substitution() {
        let mut model = model();
        model.locked.insert("dep".to_string(), locked(None));
        model
            .artifacts
            .insert("root_workspace_snapshot".to_string(), "b".repeat(64));
        let model_term =
            crate::pkg_lock_write_authority::lock_model_payload("genesis.lock", &model).unwrap();
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let PkgInstallPlanDecision::Admit(plan) = authority
            .plan_install(model_term, false, true, true)
            .unwrap()
        else {
            panic!("install plan rejected");
        };
        let observation = PkgInstallObservation {
            closure_checked: 0,
            commit_present: None,
            hashes: vec![PkgInstallHashObservation {
                hash: "a".repeat(64),
                present: true,
            }],
            initially_present: true,
            name: "dep".to_string(),
            resolution: PkgInstallResolution::NotNeeded,
        };
        let finalized = authority
            .finalize_install(
                &plan,
                std::slice::from_ref(&observation),
                vec![absent_commit("dep")],
            )
            .unwrap()
            .expect("valid install finalization");
        let report = exact_map(
            &finalized.report,
            &[
                ":checked",
                ":lock",
                ":missing",
                ":ok",
                ":provenance",
                ":workspace-root",
            ],
        )
        .unwrap();
        assert_eq!(field(report, ":checked").unwrap(), &Term::Int(1.into()));
        assert_eq!(field(report, ":ok").unwrap(), &Term::Bool(true));
        assert_eq!(
            field(report, ":workspace-root").unwrap(),
            &Term::Str("b".repeat(64))
        );

        let mut substituted = observation;
        substituted.name = "other".to_string();
        assert!(
            authority
                .finalize_install(&plan, &[substituted], vec![absent_commit("dep")])
                .unwrap()
                .is_err()
        );
    }
}
