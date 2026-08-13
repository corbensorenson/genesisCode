use std::path::Path;

use gc_coreform::hash_term;
use gc_kernel::{Apply, EffectProgram, EvalCtx, Value, value_hash};
use gc_prelude::SelfhostBootstrapMode;

use super::runner_response_budget::{hash_request, resp_from_log, unseal_effect_request};
use crate::error::EffectsError;
use crate::log::EffectLog;
use crate::replay_authority::{ExpectedReplayObservation, ReplayAuthority, ReplayDecision};
use crate::store::ArtifactStore;

/// Replay through the artifact-loaded GenesisCode authority.
///
/// Rust supplies canonical observations and performs sealed runtime mechanics. It does not
/// duplicate or override the authority's replay verdicts on logged facts.
pub fn replay_with_selfhost_authority(
    ctx: &mut EvalCtx,
    program: Value,
    log: &EffectLog,
    store: Option<&ArtifactStore>,
    expected_program_hash: [u8; 32],
    bootstrap_mode: SelfhostBootstrapMode,
    artifact: Option<&Path>,
) -> Result<Value, EffectsError> {
    let proto = ctx.protocol.ok_or(EffectsError::MissingProtocol)?;
    let mut authority = ReplayAuthority::load(bootstrap_mode, artifact)?;
    require_accept(authority.header(expected_program_hash, log.program_hash)?)?;

    let mut cur = program;
    let mut idx: usize = 0;
    loop {
        let Value::EffectProgram(effect_program) = cur else {
            return Err(EffectsError::NotAnEffectProgram);
        };
        match effect_program.as_ref() {
            EffectProgram::Pure(value) => {
                require_accept(authority.pure(idx, log.entries.len())?)?;
                return Ok((*value.as_ref()).clone());
            }
            EffectProgram::Perform { request } => {
                let (effect_request, sealed_token) =
                    unseal_effect_request(request.as_ref(), proto.effect)?;
                if sealed_token != proto.effect {
                    return Err(EffectsError::BadEffectSeal);
                }

                let payload_hash = hash_term(&effect_request.payload);
                let continuation_hash = value_hash(&effect_request.k);
                let request_hash =
                    hash_request(&effect_request.op, payload_hash, continuation_hash);
                let expected = ExpectedReplayObservation {
                    op: &effect_request.op,
                    payload: &effect_request.payload,
                    payload_hash,
                    continuation_hash,
                    request_hash,
                };

                let Some(entry) = log.entries.get(idx) else {
                    require_accept(authority.missing_entry(idx, log.version, &expected)?)?;
                    return Err(EffectsError::ReplayAuthority(
                        "authority accepted a replay step without a logged entry".to_string(),
                    ));
                };

                let response = match resp_from_log(&entry.resp, store, proto.error) {
                    Ok(response) => response,
                    Err(load_error) => {
                        const MESSAGE: &str =
                            "logged response artifact could not be loaded or decoded";
                        require_accept(authority.response_load_error(
                            idx,
                            log.version,
                            &expected,
                            entry,
                            MESSAGE,
                        )?)?;
                        return Err(EffectsError::ReplayAuthority(format!(
                            "authority accepted an unusable logged response: {load_error}"
                        )));
                    }
                };
                let response_hash = value_hash(&response);
                require_accept(authority.entry(
                    idx,
                    log.version,
                    &expected,
                    entry,
                    &response,
                    response_hash,
                )?)?;

                let continuation = (*effect_request.k).clone();
                let next = continuation.apply(ctx, response)?;
                cur = match next {
                    Value::EffectProgram(_) => next,
                    other => Value::EffectProgram(Box::new(EffectProgram::Pure(Box::new(other)))),
                };
                idx = idx.saturating_add(1);
            }
        }
    }
}

fn require_accept(decision: ReplayDecision) -> Result<(), EffectsError> {
    match decision {
        ReplayDecision::Accept => Ok(()),
        ReplayDecision::Reject { code, message } => {
            Err(EffectsError::ReplayRejected { code, message })
        }
    }
}
