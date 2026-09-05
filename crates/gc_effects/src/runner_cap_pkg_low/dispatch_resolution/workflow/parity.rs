use super::*;

use crate::pkg_resolution_identity_authority::PkgWorkflowObject;

#[cfg(any(test, feature = "parity-oracle"))]
pub(super) fn plan_workflow_parity(
    model: &gc_pkg::GenesisLock,
    workflow: PkgResolutionWorkflow,
    only: &[String],
) -> PkgWorkflowPlan {
    let only_set = only
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    let mut steps = model
        .requirements
        .iter()
        .map(|(name, requirement)| {
            let action = match workflow {
                PkgResolutionWorkflow::Lock => PkgWorkflowAction::Resolve,
                PkgResolutionWorkflow::Update if only_set.is_empty() || only_set.contains(name) => {
                    PkgWorkflowAction::Consider
                }
                PkgResolutionWorkflow::Update => PkgWorkflowAction::SkipUnselected,
            };
            (
                PkgWorkflowStep {
                    action,
                    name: name.clone(),
                },
                requirement_term(requirement),
            )
        })
        .collect::<Vec<_>>();
    if workflow == PkgResolutionWorkflow::Update {
        for name in only_set {
            if !model.requirements.contains_key(&name) {
                steps.push((
                    PkgWorkflowStep {
                        action: PkgWorkflowAction::MissingRequirement,
                        name,
                    },
                    Term::Nil,
                ));
            }
        }
    }
    let step_terms = steps
        .iter()
        .map(|(step, requirement)| {
            term_map([
                (":action", action_term(step.action)),
                (":name", Term::Str(step.name.clone())),
                (":requirement", requirement.clone()),
            ])
        })
        .collect();
    let term = term_map([
        (":steps", Term::Vector(step_terms)),
        (":workflow", workflow_term(workflow)),
    ]);
    PkgWorkflowPlan {
        hash: hash_term_hex(&term),
        model: crate::pkg_lock_write_authority::lock_model_payload("", model)
            .expect("parity lock model transport"),
        steps: steps.into_iter().map(|(step, _)| step).collect(),
        term,
    }
}

#[cfg(any(test, feature = "parity-oracle"))]
pub(super) fn finalize_workflow_parity(
    model: &gc_pkg::GenesisLock,
    workflow: PkgResolutionWorkflow,
    executed: &ExecutedWorkflow,
    store: &ArtifactStore,
    policy: &CapsPolicy,
    commit_authority: &mut Option<CommitAuthority>,
    strict: bool,
    error_tok: SealId,
    op: &str,
) -> Result<PkgWorkflowFinalized, Value> {
    let locked = executed.resolved.clone();
    let mut selected_count = 0_u64;
    let mut updated_count = 0_u64;
    let mut rationale = Vec::with_capacity(executed.plan.steps.len());
    for (step, observation) in executed.plan.steps.iter().zip(&executed.observations) {
        let requirement = model.requirements.get(&step.name);
        let previous = model.locked.get(&step.name);
        let (action, reason, resolved) = match step.action {
            PkgWorkflowAction::Resolve => (
                ":resolved",
                resolution_reason(observation.resolved.as_ref()),
                observation.resolved.as_ref(),
            ),
            PkgWorkflowAction::Consider => {
                selected_count = selected_count.saturating_add(1);
                if observation.should_resolve == Some(false) {
                    (
                        ":kept-existing",
                        "GenesisCode resolution plan retained the existing locked entry"
                            .to_string(),
                        previous,
                    )
                } else {
                    let resolved = observation.resolved.as_ref();
                    let changed = resolved.is_some_and(|entry| {
                        previous.is_none_or(|old| !locked_entry_eq(old, entry))
                    });
                    if changed {
                        updated_count = updated_count.saturating_add(1);
                    }
                    (
                        if changed { ":updated" } else { ":no-change" },
                        if changed {
                            if previous.is_some() {
                                "resolved new lock entry for selected dependency"
                            } else {
                                "resolved missing locked entry"
                            }
                        } else {
                            "resolved dependency equals existing lock entry"
                        }
                        .to_string(),
                        resolved,
                    )
                }
            }
            PkgWorkflowAction::SkipUnselected => (
                ":skipped-unselected",
                "not selected by --only filter".to_string(),
                previous,
            ),
            PkgWorkflowAction::MissingRequirement => (
                ":missing-requirement",
                "selected dependency is not present in lock requirements".to_string(),
                None,
            ),
        };
        rationale.push(rationale_term(
            &step.name,
            requirement,
            action,
            &reason,
            resolved,
        ));
    }
    let provenance = locked_dependency_provenance(
        store,
        policy,
        commit_authority,
        &locked,
        strict,
        error_tok,
        op,
    )?;
    let rationale_artifact = term_map([
        (
            ":data",
            term_map([
                (":entries", Term::Vector(rationale.clone())),
                (":entry-count", Term::Int((rationale.len() as u64).into())),
                (":workflow", workflow_term(workflow)),
            ]),
        ),
        (":inputs", Term::Vector(Vec::new())),
        (":kind", Term::symbol(":pkg-resolution-rationale")),
        (":outputs", Term::Vector(Vec::new())),
        (
            ":produced-by",
            term_map([
                (":tool", Term::Str("genesis".to_string())),
                (":tool-version", Term::Str("v0.2".to_string())),
            ]),
        ),
        (":type", Term::symbol(":vcs/evidence")),
        (":v", Term::Int(1.into())),
    ]);
    let workspace_artifact = term_map([
        (":kind", Term::symbol(":workspace")),
        (":lock", Term::Nil),
        (
            ":modules",
            Term::Map(
                locked
                    .iter()
                    .map(|(name, entry)| {
                        (
                            TermOrdKey(Term::Str(name.clone())),
                            Term::Str(entry.snapshot.clone()),
                        )
                    })
                    .collect(),
            ),
        ),
        (":type", Term::symbol(":vcs/snapshot")),
        (":v", Term::Int(1.into())),
        (":workspace", Term::Str(model.workspace.clone())),
    ]);
    Ok(PkgWorkflowFinalized {
        locked_count: locked.len() as u64,
        locked,
        provenance,
        rationale,
        rationale_object: workflow_object(rationale_artifact, true),
        selected_count,
        updated_count,
        workspace_object: workflow_object(workspace_artifact, false),
    })
}

#[cfg(any(test, feature = "parity-oracle"))]
fn requirement_term(requirement: &gc_pkg::Requirement) -> Term {
    term_map([
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
        (
            ":update-policy",
            Term::symbol(match requirement.update_policy {
                gc_pkg::UpdatePolicy::Manual => ":manual",
                gc_pkg::UpdatePolicy::Auto => ":auto",
            }),
        ),
    ])
}

#[cfg(any(test, feature = "parity-oracle"))]
fn locked_entry_eq(left: &gc_pkg::LockedEntry, right: &gc_pkg::LockedEntry) -> bool {
    left.commit == right.commit
        && left.snapshot == right.snapshot
        && left.registry == right.registry
        && left.source_selector == right.source_selector
        && left.resolved_ref == right.resolved_ref
        && left.exports_hash == right.exports_hash
        && left.environment_fingerprint == right.environment_fingerprint
}

#[cfg(any(test, feature = "parity-oracle"))]
fn rationale_term(
    name: &str,
    requirement: Option<&gc_pkg::Requirement>,
    action: &str,
    reason: &str,
    resolved: Option<&gc_pkg::LockedEntry>,
) -> Term {
    term_map([
        (":action", Term::symbol(action)),
        (
            ":commit",
            resolved
                .and_then(|entry| entry.commit.clone())
                .map(Term::Str)
                .unwrap_or(Term::Nil),
        ),
        (":name", Term::Str(name.to_string())),
        (":reason", Term::Str(reason.to_string())),
        (
            ":registry",
            requirement
                .and_then(|value| value.registry.clone())
                .or_else(|| resolved.and_then(|entry| entry.registry.clone()))
                .map(Term::Str)
                .unwrap_or(Term::Nil),
        ),
        (
            ":resolved-ref",
            resolved
                .and_then(|entry| entry.resolved_ref.clone())
                .map(Term::Str)
                .unwrap_or(Term::Nil),
        ),
        (
            ":selector",
            requirement
                .map(|value| Term::Str(value.selector.clone()))
                .unwrap_or(Term::Nil),
        ),
        (
            ":snapshot",
            resolved
                .map(|entry| Term::Str(entry.snapshot.clone()))
                .unwrap_or(Term::Nil),
        ),
        (
            ":strategy",
            requirement
                .map(|value| Term::symbol(format!(":{}", value.strategy.as_str())))
                .unwrap_or(Term::Nil),
        ),
        (
            ":tag-policy",
            requirement
                .and_then(|value| value.tag_policy.clone())
                .map(Term::Str)
                .unwrap_or(Term::Nil),
        ),
        (
            ":update-policy",
            requirement
                .map(|value| {
                    Term::symbol(match value.update_policy {
                        gc_pkg::UpdatePolicy::Manual => ":manual",
                        gc_pkg::UpdatePolicy::Auto => ":auto",
                    })
                })
                .unwrap_or(Term::Nil),
        ),
    ])
}

#[cfg(any(test, feature = "parity-oracle"))]
fn resolution_reason(resolved: Option<&gc_pkg::LockedEntry>) -> String {
    match resolved {
        Some(entry) if entry.commit.is_some() && entry.resolved_ref.is_some() => {
            "resolved selector to commit via tracked ref/tag".to_string()
        }
        Some(entry) if entry.commit.is_some() => {
            "resolved selector to pinned commit+snapshot".to_string()
        }
        Some(_) => "resolved selector directly to snapshot artifact".to_string(),
        None => "requirement present but lock entry was not produced".to_string(),
    }
}

#[cfg(any(test, feature = "parity-oracle"))]
fn workflow_object(term: Term, newline: bool) -> PkgWorkflowObject {
    let mut bytes = print_term(&term).into_bytes();
    if newline {
        bytes.push(b'\n');
    }
    PkgWorkflowObject {
        hash: blake3::hash(&bytes).to_hex().to_string(),
        bytes,
    }
}

#[cfg(any(test, feature = "parity-oracle"))]
fn term_map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value))
            .collect(),
    )
}

#[cfg(any(test, feature = "parity-oracle"))]
fn action_term(action: PkgWorkflowAction) -> Term {
    Term::symbol(match action {
        PkgWorkflowAction::Resolve => ":resolve",
        PkgWorkflowAction::Consider => ":consider",
        PkgWorkflowAction::SkipUnselected => ":skip-unselected",
        PkgWorkflowAction::MissingRequirement => ":missing-requirement",
    })
}

#[cfg(any(test, feature = "parity-oracle"))]
fn workflow_term(workflow: PkgResolutionWorkflow) -> Term {
    Term::symbol(match workflow {
        PkgResolutionWorkflow::Lock => ":lock",
        PkgResolutionWorkflow::Update => ":update",
    })
}

#[cfg(any(test, feature = "parity-oracle"))]
fn hash_term_hex(term: &Term) -> String {
    gc_coreform::hash_term(term)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
