use super::*;

#[cfg(any(test, feature = "parity-oracle"))]
#[path = "workflow/parity.rs"]
mod parity;
#[cfg(any(test, feature = "parity-oracle"))]
use parity::{finalize_workflow_parity, plan_workflow_parity};

pub(super) struct ExecutedWorkflow {
    pub(super) observations: Vec<PkgWorkflowObservation>,
    pub(super) plan: PkgWorkflowPlan,
    pub(super) resolved: BTreeMap<String, gc_pkg::LockedEntry>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "package mechanism dependencies remain explicit at the authority boundary"
)]
pub(super) fn execute_workflow(
    mut authority: Option<&mut PkgResolutionIdentityAuthority>,
    workflow: PkgResolutionWorkflow,
    only: &[String],
    lock_path: &str,
    model: &gc_pkg::GenesisLock,
    store: &ArtifactStore,
    refs: &RefsDb,
    policy: &CapsPolicy,
    pol: Option<&OpPolicy>,
    budget: &mut ArtifactBudgetState,
    timeout_ms: Option<u64>,
    error_tok: SealId,
    op: &str,
) -> Result<ExecutedWorkflow, Value> {
    let plan = if let Some(workflow_authority) = authority.as_deref_mut() {
        let model_term = crate::pkg_lock_write_authority::lock_model_payload(lock_path, model)
            .map_err(|error| {
                mk_error(
                    error_tok,
                    "core/pkg/authority-error",
                    error.to_string(),
                    Some(op),
                )
            })?;
        match workflow_authority
            .plan_workflow(model_term, workflow, only)
            .map_err(|error| {
                mk_error(
                    error_tok,
                    "core/pkg/authority-error",
                    error.to_string(),
                    Some(op),
                )
            })? {
            PkgWorkflowDecision::Accept(plan) => plan,
            PkgWorkflowDecision::Error { code, message } => {
                return Err(mk_error(error_tok, &code, message, Some(op)));
            }
        }
    } else {
        #[cfg(any(test, feature = "parity-oracle"))]
        {
            plan_workflow_parity(model, workflow, only)
        }
        #[cfg(not(any(test, feature = "parity-oracle")))]
        {
            return Err(mk_error(
                error_tok,
                "core/pkg/authority-error",
                "package resolution requires the artifact-loaded GenesisCode workflow authority"
                    .to_string(),
                Some(op),
            ));
        }
    };

    // Preserve existing lock validation semantics, including unselected requirements,
    // while ensuring the authority plan is issued before any resolver mechanism runs.
    for (name, requirement) in &model.requirements {
        validate_requirement_registry_alias(model, name, requirement, error_tok, op)?;
    }

    let mut resolved = if workflow == PkgResolutionWorkflow::Update {
        model.locked.clone()
    } else {
        BTreeMap::new()
    };
    let mut observations = Vec::with_capacity(plan.steps.len());
    for PkgWorkflowStep { action, name } in &plan.steps {
        let requirement = model.requirements.get(name);
        match action {
            PkgWorkflowAction::Resolve => {
                let requirement = requirement.ok_or_else(|| {
                    mk_error(
                        error_tok,
                        "core/pkg/authority-error",
                        format!("workflow resolve step references missing requirement: {name}"),
                        Some(op),
                    )
                })?;
                let resolution_plan =
                    plan_requirement(authority.as_deref_mut(), requirement, false, error_tok, op)
                        .map_err(|value| {
                        annotate_requirement_resolution_error(value, name, requirement)
                    })?;
                let entry = resolve_requirement(
                    store,
                    refs,
                    &model.registries,
                    policy,
                    pol,
                    budget,
                    timeout_ms,
                    name,
                    requirement,
                    resolution_plan,
                    authority.as_deref_mut(),
                    error_tok,
                    op,
                )
                .map_err(|value| annotate_requirement_resolution_error(value, name, requirement))?;
                resolved.insert(name.clone(), entry.clone());
                observations.push(PkgWorkflowObservation {
                    name: name.clone(),
                    resolved: Some(entry),
                    should_resolve: Some(true),
                });
            }
            PkgWorkflowAction::Consider => {
                let requirement = requirement.ok_or_else(|| {
                    mk_error(
                        error_tok,
                        "core/pkg/authority-error",
                        format!("workflow consider step references missing requirement: {name}"),
                        Some(op),
                    )
                })?;
                let resolution_plan = plan_requirement(
                    authority.as_deref_mut(),
                    requirement,
                    resolved.contains_key(name),
                    error_tok,
                    op,
                )
                .map_err(|value| annotate_requirement_resolution_error(value, name, requirement))?;
                if resolution_plan.should_resolve {
                    let entry = resolve_requirement(
                        store,
                        refs,
                        &model.registries,
                        policy,
                        pol,
                        budget,
                        timeout_ms,
                        name,
                        requirement,
                        resolution_plan,
                        authority.as_deref_mut(),
                        error_tok,
                        op,
                    )
                    .map_err(|value| {
                        annotate_requirement_resolution_error(value, name, requirement)
                    })?;
                    resolved.insert(name.clone(), entry.clone());
                    observations.push(PkgWorkflowObservation {
                        name: name.clone(),
                        resolved: Some(entry),
                        should_resolve: Some(true),
                    });
                } else {
                    observations.push(PkgWorkflowObservation {
                        name: name.clone(),
                        resolved: None,
                        should_resolve: Some(false),
                    });
                }
            }
            PkgWorkflowAction::SkipUnselected | PkgWorkflowAction::MissingRequirement => {
                observations.push(PkgWorkflowObservation {
                    name: name.clone(),
                    resolved: None,
                    should_resolve: None,
                });
            }
        }
    }
    Ok(ExecutedWorkflow {
        observations,
        plan,
        resolved,
    })
}

pub(super) fn finalize_workflow(
    mut authority: Option<&mut PkgResolutionIdentityAuthority>,
    workflow: PkgResolutionWorkflow,
    only: &[String],
    _lock_path: &str,
    model: &mut gc_pkg::GenesisLock,
    executed: ExecutedWorkflow,
    store: &ArtifactStore,
    strict: bool,
    error_tok: SealId,
    op: &str,
) -> Result<PkgWorkflowFinalized, Value> {
    let finalized = if let Some(workflow_authority) = authority.as_deref_mut() {
        let commit_observations = commit_observations(store, &executed.resolved);
        match workflow_authority
            .finalize_workflow(
                workflow,
                only,
                &executed.plan,
                &executed.observations,
                commit_observations,
                strict,
            )
            .map_err(|error| {
                mk_error(
                    error_tok,
                    "core/pkg/authority-error",
                    error.to_string(),
                    Some(op),
                )
            })? {
            PkgWorkflowDecision::Accept(finalized) => finalized,
            PkgWorkflowDecision::Error { code, message } => {
                return Err(mk_error(error_tok, &code, message, Some(op)));
            }
        }
    } else {
        #[cfg(any(test, feature = "parity-oracle"))]
        {
            finalize_workflow_parity(model, workflow, &executed, store, strict, error_tok, op)?
        }
        #[cfg(not(any(test, feature = "parity-oracle")))]
        {
            return Err(mk_error(
                error_tok,
                "core/pkg/authority-error",
                "package resolution requires the artifact-loaded GenesisCode workflow authority"
                    .to_string(),
                Some(op),
            ));
        }
    };

    if strict {
        validate_locked_entries_strict(
            authority.as_deref_mut(),
            store,
            &model.requirements,
            &finalized.locked,
            true,
            error_tok,
            op,
        )?;
    }
    let rationale_hash = persist_object(
        store,
        &finalized.rationale_object.bytes,
        &finalized.rationale_object.hash,
        error_tok,
        op,
    )?;
    let workspace_hash = persist_object(
        store,
        &finalized.workspace_object.bytes,
        &finalized.workspace_object.hash,
        error_tok,
        op,
    )?;
    let rationale_key = match workflow {
        PkgResolutionWorkflow::Lock => "lock_resolution_rationale",
        PkgResolutionWorkflow::Update => "update_resolution_rationale",
    };
    model.locked = finalized.locked.clone();
    model
        .artifacts
        .insert(rationale_key.to_string(), rationale_hash);
    model
        .artifacts
        .insert("root_workspace_snapshot".to_string(), workspace_hash);
    Ok(finalized)
}

fn commit_observations(
    store: &ArtifactStore,
    locked: &BTreeMap<String, gc_pkg::LockedEntry>,
) -> Vec<Term> {
    locked
        .iter()
        .map(|(name, entry)| {
            let (status, evidence, obligations) = match entry.commit.as_deref() {
                None => (Term::symbol(":absent"), Vec::new(), Vec::new()),
                Some(commit_hash) => match store_get_term(store, commit_hash).and_then(|term| {
                    gc_vcs::Commit::from_term(&term)
                        .map_err(|error| EffectsError::Log(format!("bad commit: {error}")))
                }) {
                    Ok(commit) => (
                        Term::symbol(":valid"),
                        commit.evidence.into_iter().map(Term::Str).collect(),
                        commit.obligations.into_iter().map(Term::Str).collect(),
                    ),
                    Err(_) => (Term::symbol(":invalid"), Vec::new(), Vec::new()),
                },
            };
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":commit")),
                        entry.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":evidence")),
                        Term::Vector(evidence),
                    ),
                    (TermOrdKey(Term::symbol(":name")), Term::Str(name.clone())),
                    (
                        TermOrdKey(Term::symbol(":obligations")),
                        Term::Vector(obligations),
                    ),
                    (TermOrdKey(Term::symbol(":status")), status),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect()
}

fn persist_object(
    store: &ArtifactStore,
    bytes: &[u8],
    expected_hash: &str,
    error_tok: SealId,
    op: &str,
) -> Result<String, Value> {
    let actual = store.put_bytes(bytes).map_err(|error| {
        mk_error(
            error_tok,
            "core/store/io-error",
            error.to_string(),
            Some(op),
        )
    })?;
    if actual != expected_hash {
        return Err(mk_error(
            error_tok,
            "core/store/corruption",
            "artifact store identity contradicts the authorized package workflow object"
                .to_string(),
            Some(op),
        ));
    }
    Ok(actual)
}
