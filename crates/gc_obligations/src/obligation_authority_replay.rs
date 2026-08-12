#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ReplayEntryObservation {
    pub position: u64,
    pub op: String,
    pub task_id: Option<String>,
    pub schedule_step: Option<u64>,
    pub await_edge: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ReplayObservation {
    pub suite: String,
    pub name: String,
    pub log_artifact: String,
    pub program: bool,
    pub actual_hash: [u8; 32],
    pub replay_hash: Option<[u8; 32]>,
    pub entries: Vec<ReplayEntryObservation>,
}

fn replay_entry_term(entry: &ReplayEntryObservation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":await-edge")),
                entry
                    .await_edge
                    .clone()
                    .map(Term::Str)
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":op")),
                Term::symbol(entry.op.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":position")),
                Term::Int(BigInt::from(entry.position)),
            ),
            (
                TermOrdKey(Term::symbol(":schedule-step")),
                optional_u64_term(entry.schedule_step),
            ),
            (
                TermOrdKey(Term::symbol(":task-id")),
                entry
                    .task_id
                    .clone()
                    .map(Term::Str)
                    .unwrap_or(Term::Nil),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn replay_observation_term(observation: &ReplayObservation) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":actual-h")),
                Term::Bytes(observation.actual_hash.to_vec().into()),
            ),
            (
                TermOrdKey(Term::symbol(":entries")),
                Term::Vector(
                    observation
                        .entries
                        .iter()
                        .map(replay_entry_term)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":log-artifact")),
                Term::Str(observation.log_artifact.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str(observation.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":program")),
                Term::Bool(observation.program),
            ),
            (
                TermOrdKey(Term::symbol(":replay-h")),
                optional_hash_term(observation.replay_hash),
            ),
            (
                TermOrdKey(Term::symbol(":suite")),
                Term::symbol(observation.suite.clone()),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn replay_inputs(observations: &[ReplayObservation]) -> Term {
    Term::Map(
        [(
            TermOrdKey(Term::symbol(":tests")),
            Term::Vector(
                observations
                    .iter()
                    .map(replay_observation_term)
                    .collect(),
            ),
        )]
        .into_iter()
        .collect(),
    )
}

fn task_like_op(op: &str) -> bool {
    op.starts_with("core/task::") || op.starts_with("editor/task::")
}

fn task_id_required(op: &str) -> bool {
    matches!(
        op,
        "core/task::await"
            | "core/task::cancel"
            | "core/task::status"
            | "editor/task::poll"
            | "editor/task::cancel"
    )
}

fn replay_expected_errors(
    operation: ObligationAuthorityOperation,
    observations: &[ReplayObservation],
) -> (Vec<String>, u64) {
    let mut errors = Vec::new();
    let mut concurrent_tests = 0_u64;
    for observation in observations {
        let concurrent = observation
            .entries
            .iter()
            .any(|entry| task_like_op(&entry.op));
        if operation == ObligationAuthorityOperation::ConcurrencyReplay && !concurrent {
            continue;
        }
        if operation == ObligationAuthorityOperation::ConcurrencyReplay {
            concurrent_tests = concurrent_tests.saturating_add(1);
            for entry in &observation.entries {
                if !task_like_op(&entry.op) {
                    continue;
                }
                if entry.schedule_step != Some(entry.position) {
                    errors.push(format!(
                        "concurrency log mismatch for {}::{} at entry {}: expected :schedule-step {}, got {:?}",
                        observation.suite,
                        observation.name,
                        entry.position,
                        entry.position,
                        entry.schedule_step
                    ));
                }
                if entry.op == "core/task::await" && entry.await_edge.is_none() {
                    errors.push(format!(
                        "concurrency log missing :await-edge for {}::{} at entry {}",
                        observation.suite, observation.name, entry.position
                    ));
                }
                if task_id_required(&entry.op) && entry.task_id.is_none() {
                    errors.push(format!(
                        "concurrency log missing :task-id for {}::{} at entry {} ({})",
                        observation.suite, observation.name, entry.position, entry.op
                    ));
                }
            }
        }
        if !observation.program {
            let suffix = if operation == ObligationAuthorityOperation::ConcurrencyReplay {
                "concurrency replay"
            } else {
                "replayability"
            };
            errors.push(format!(
                "test {} expected effect program for {suffix}",
                observation.name
            ));
        } else if observation.replay_hash != Some(observation.actual_hash) {
            let replay_hash = observation.replay_hash.unwrap_or([0; 32]);
            if operation == ObligationAuthorityOperation::ConcurrencyReplay {
                errors.push(format!(
                    "concurrency replay mismatch for {}::{}: {}",
                    observation.suite,
                    observation.name,
                    hex32(replay_hash)
                ));
            } else {
                errors.push(format!(
                    "replay mismatch for {}: {}",
                    observation.name,
                    hex32(replay_hash)
                ));
            }
        }
    }
    (errors, concurrent_tests)
}

fn expected_replay_report(
    operation: ObligationAuthorityOperation,
    manifest: &PackageManifest,
    errors: &[String],
    concurrent_tests: u64,
) -> Term {
    let mut fields = BTreeMap::from([
        (
            TermOrdKey(Term::symbol(":errors")),
            Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
        ),
        (
            TermOrdKey(Term::symbol(":kind")),
            Term::Str(
                if operation == ObligationAuthorityOperation::ConcurrencyReplay {
                    "genesis/concurrency-replay-v0.1"
                } else {
                    "genesis/replayable-tests-v0.2"
                }
                .to_string(),
            ),
        ),
        (
            TermOrdKey(Term::symbol(":ok")),
            Term::Bool(errors.is_empty()),
        ),
        (
            TermOrdKey(Term::symbol(":package")),
            Term::Str(manifest.name.clone()),
        ),
    ]);
    if operation == ObligationAuthorityOperation::ConcurrencyReplay {
        fields.insert(
            TermOrdKey(Term::symbol(":concurrent-tests")),
            Term::Int(BigInt::from(concurrent_tests)),
        );
    }
    Term::Map(fields)
}

fn validate_replay_report(
    operation: ObligationAuthorityOperation,
    report: &Term,
    manifest: &PackageManifest,
    observations: &[ReplayObservation],
    outer_ok: bool,
    outer_errors: &[String],
) -> Result<(), ObligationError> {
    let (expected_errors, concurrent_tests) = replay_expected_errors(operation, observations);
    let expected = expected_replay_report(operation, manifest, &expected_errors, concurrent_tests);
    if outer_errors != expected_errors
        || outer_ok != expected_errors.is_empty()
        || report != &expected
    {
        return Err(authority_error("replay authority result contradicts host observations"));
    }
    Ok(())
}

pub(super) fn evaluate_replay_obligation_with_authority(
    operation: ObligationAuthorityOperation,
    store: &EvidenceStore,
    manifest: &PackageManifest,
    observations: &[ReplayObservation],
    frontend: &CoreformFrontend,
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    if !matches!(
        operation,
        ObligationAuthorityOperation::ReplayableTests
            | ObligationAuthorityOperation::ConcurrencyReplay
    ) {
        return Err(authority_error("non-replay operation used replay evaluator"));
    }
    let request = authority_request_term(operation, &manifest.name, replay_inputs(observations));
    let request_hash = hash_term(&request);
    let term = invoke_authority(request, frontend, limits)?;
    decode_authority_result(
        operation,
        store,
        manifest,
        &[],
        &[],
        observations,
        request_hash,
        term,
    )
}
