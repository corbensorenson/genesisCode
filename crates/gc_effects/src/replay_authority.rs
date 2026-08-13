use std::collections::BTreeSet;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};

use crate::error::EffectsError;
use crate::log::{Decision, EffectLogEntry};

const REPLAY_AUTHORITY_BINDING: &str = "core/effects::replay-authority";
const REPLAY_AUTHORITY_STEP_LIMIT: u64 = 20_000_000;
const REPLAY_AUTHORITY_ALLOC_LIMIT: u64 = 32_000_000;
const REQUEST_KIND: &str = "genesis/effect-replay-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/effect-replay-authority-result-v0.1";

fn authority_error(message: impl Into<String>) -> EffectsError {
    EffectsError::ReplayAuthority(format!("selfhost replay authority: {}", message.into()))
}

fn key(name: &str) -> TermOrdKey {
    TermOrdKey(Term::symbol(name))
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (key(name), value))
            .collect(),
    )
}

fn int(value: usize) -> Result<Term, EffectsError> {
    let value = u64::try_from(value)
        .map_err(|_| authority_error("host index exceeds the replay protocol integer domain"))?;
    Ok(Term::Int(value.into()))
}

fn optional_string(value: Option<&str>) -> Term {
    value
        .map(|value| Term::Str(value.to_string()))
        .unwrap_or(Term::Nil)
}

fn hash_bytes(value: [u8; 32]) -> Term {
    Term::Bytes(value.to_vec().into())
}

fn hash_hex(value: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in value {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn response_data(value: &Value) -> Term {
    match value {
        Value::Data(term) => term.as_ref().clone(),
        // Scheduler metadata is derived only from unsealed response data. Sealed ERROR
        // payloads remain opaque to that derivation, exactly as in the runtime boundary.
        _ => Term::Nil,
    }
}

fn logged_observation(entry: &EffectLogEntry) -> Term {
    map([
        (":await-edge", optional_string(entry.await_edge.as_deref())),
        (":cap", entry.cap.clone()),
        (":cont-h", hash_bytes(entry.cont_h)),
        (
            ":decision",
            Term::symbol(match entry.decision {
                Decision::Allow => ":allow",
                Decision::Deny => ":deny",
            }),
        ),
        (":i", Term::Int(entry.i.into())),
        (":op", Term::symbol(entry.op.clone())),
        (
            ":parent-task",
            optional_string(entry.parent_task.as_deref()),
        ),
        (":payload-h", hash_bytes(entry.payload_h)),
        (":req-h", hash_bytes(entry.req_h)),
        (":resp-h", hash_bytes(entry.resp_h)),
        (
            ":schedule-step",
            entry
                .schedule_step
                .map(|value| Term::Int(value.into()))
                .unwrap_or(Term::Nil),
        ),
        (":task-id", optional_string(entry.task_id.as_deref())),
    ])
}

pub(crate) struct ExpectedReplayObservation<'a> {
    pub(crate) op: &'a str,
    pub(crate) payload: &'a Term,
    pub(crate) payload_hash: [u8; 32],
    pub(crate) continuation_hash: [u8; 32],
    pub(crate) request_hash: [u8; 32],
}

enum ResponseObservation<'a> {
    Loaded { value: &'a Value, hash: [u8; 32] },
    LoadError(&'a str),
    Unavailable,
}

fn expected_observation(
    expected: &ExpectedReplayObservation<'_>,
    response: ResponseObservation<'_>,
) -> Term {
    let (status, response_term, response_hash, response_error) = match response {
        ResponseObservation::Loaded { value, hash } => {
            (":loaded", response_data(value), hash_bytes(hash), Term::Nil)
        }
        ResponseObservation::LoadError(message) => (
            ":load-error",
            Term::Nil,
            Term::Nil,
            Term::Str(message.to_string()),
        ),
        ResponseObservation::Unavailable => (":unavailable", Term::Nil, Term::Nil, Term::Nil),
    };
    map([
        (":cont-h", hash_bytes(expected.continuation_hash)),
        (":op", Term::symbol(expected.op.to_string())),
        (":payload", expected.payload.clone()),
        (":payload-h", hash_bytes(expected.payload_hash)),
        (":req-h", hash_bytes(expected.request_hash)),
        (":resp-h", response_hash),
        (":response", response_term),
        (":response-error", response_error),
        (":response-status", Term::symbol(status)),
    ])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReplayDecision {
    Accept,
    Reject { code: String, message: String },
}

pub(crate) struct ReplayAuthority {
    context: EvalCtx,
    authority: Value,
}

impl ReplayAuthority {
    pub(crate) fn load(
        bootstrap_mode: SelfhostBootstrapMode,
        artifact: Option<&Path>,
    ) -> Result<Self, EffectsError> {
        let mut context = EvalCtx::with_step_limit(None);
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(REPLAY_AUTHORITY_ALLOC_LIMIT),
            ..MemLimits::default()
        });
        let prelude = build_prelude(&mut context);
        let mut environment = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut context,
            &mut environment,
            bootstrap_mode,
            artifact,
        )
        .map_err(|error| authority_error(format!("selfhost/init: {error:#}")))?;
        let authority = environment.get(REPLAY_AUTHORITY_BINDING).ok_or_else(|| {
            authority_error(format!("missing binding {REPLAY_AUTHORITY_BINDING}"))
        })?;
        // Bootstrap has its own bounded profile. Replay receives the complete
        // declared request-processing budget rather than bootstrap's residue.
        context.reset_counters();
        context.step_limit = Some(REPLAY_AUTHORITY_STEP_LIMIT);
        Ok(Self { context, authority })
    }

    pub(crate) fn header(
        &mut self,
        expected_program_hash: [u8; 32],
        logged_program_hash: [u8; 32],
    ) -> Result<ReplayDecision, EffectsError> {
        self.decide(map([
            (":expected-program-h", hash_bytes(expected_program_hash)),
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":logged-program-h", hash_bytes(logged_program_hash)),
            (":phase", Term::symbol(":header")),
            (":v", Term::Int(1.into())),
        ]))
    }

    pub(crate) fn pure(
        &mut self,
        index: usize,
        entry_count: usize,
    ) -> Result<ReplayDecision, EffectsError> {
        self.decide(map([
            (":entry-count", int(entry_count)?),
            (":index", int(index)?),
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":phase", Term::symbol(":pure")),
            (":v", Term::Int(1.into())),
        ]))
    }

    pub(crate) fn missing_entry(
        &mut self,
        index: usize,
        log_version: u64,
        expected: &ExpectedReplayObservation<'_>,
    ) -> Result<ReplayDecision, EffectsError> {
        self.perform(
            index,
            log_version,
            expected,
            None,
            ResponseObservation::Unavailable,
        )
    }

    pub(crate) fn response_load_error(
        &mut self,
        index: usize,
        log_version: u64,
        expected: &ExpectedReplayObservation<'_>,
        entry: &EffectLogEntry,
        message: &str,
    ) -> Result<ReplayDecision, EffectsError> {
        self.perform(
            index,
            log_version,
            expected,
            Some(entry),
            ResponseObservation::LoadError(message),
        )
    }

    pub(crate) fn entry(
        &mut self,
        index: usize,
        log_version: u64,
        expected: &ExpectedReplayObservation<'_>,
        entry: &EffectLogEntry,
        response: &Value,
        response_hash: [u8; 32],
    ) -> Result<ReplayDecision, EffectsError> {
        self.perform(
            index,
            log_version,
            expected,
            Some(entry),
            ResponseObservation::Loaded {
                value: response,
                hash: response_hash,
            },
        )
    }

    fn perform(
        &mut self,
        index: usize,
        log_version: u64,
        expected: &ExpectedReplayObservation<'_>,
        entry: Option<&EffectLogEntry>,
        response: ResponseObservation<'_>,
    ) -> Result<ReplayDecision, EffectsError> {
        self.decide(map([
            (":expected", expected_observation(expected, response)),
            (":index", int(index)?),
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":log-version", Term::Int(log_version.into())),
            (
                ":logged",
                entry.map(logged_observation).unwrap_or(Term::Nil),
            ),
            (":phase", Term::symbol(":perform")),
            (":v", Term::Int(1.into())),
        ]))
    }

    fn decide(&mut self, request: Term) -> Result<ReplayDecision, EffectsError> {
        let request_hash = hash_term(&request);
        let value = self
            .authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("apply failed: {error}")))?;
        let term = match &value {
            Value::Sealed { token, payload }
                if self
                    .context
                    .protocol
                    .is_some_and(|protocol| *token == protocol.error) =>
            {
                let detail = payload
                    .to_plain_term()
                    .map(|term| print_term(&term))
                    .unwrap_or_else(|| "<opaque-error-payload>".to_string());
                return Err(authority_error(format!("returned sealed ERROR {detail}")));
            }
            _ => value
                .to_plain_term()
                .ok_or_else(|| authority_error("returned an opaque value"))?,
        };
        decode_result(term, request_hash)
    }
}

fn decode_result(term: Term, request_hash: [u8; 32]) -> Result<ReplayDecision, EffectsError> {
    let Term::Map(fields) = term else {
        return Err(authority_error("result must be a data map"));
    };
    let expected_keys: BTreeSet<_> = [":code", ":kind", ":message", ":ok", ":request-h", ":v"]
        .into_iter()
        .map(key)
        .collect();
    if fields.keys().cloned().collect::<BTreeSet<_>>() != expected_keys {
        return Err(authority_error("result field set mismatch"));
    }
    if !matches!(fields.get(&key(":kind")), Some(Term::Str(kind)) if kind == RESULT_KIND)
        || !matches!(fields.get(&key(":v")), Some(Term::Int(version)) if version == &1.into())
        || !matches!(fields.get(&key(":request-h")), Some(Term::Str(actual)) if actual == &hash_hex(request_hash))
    {
        return Err(authority_error("result identity mismatch"));
    }
    match fields.get(&key(":ok")) {
        Some(Term::Bool(true)) => {
            if fields.get(&key(":code")) != Some(&Term::Nil)
                || fields.get(&key(":message")) != Some(&Term::Nil)
            {
                return Err(authority_error(
                    "accepted result must carry nil :code and :message",
                ));
            }
            Ok(ReplayDecision::Accept)
        }
        Some(Term::Bool(false)) => {
            let code = match fields.get(&key(":code")) {
                Some(Term::Str(code)) if !code.is_empty() => code.clone(),
                _ => return Err(authority_error("rejected result must carry nonempty :code")),
            };
            let message = match fields.get(&key(":message")) {
                Some(Term::Str(message)) if !message.is_empty() => message.clone(),
                _ => {
                    return Err(authority_error(
                        "rejected result must carry nonempty :message",
                    ));
                }
            };
            Ok(ReplayDecision::Reject { code, message })
        }
        _ => Err(authority_error("result :ok must be a bool")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn result(request_hash: [u8; 32], extra: Option<(&'static str, Term)>) -> Term {
        let mut fields: BTreeMap<_, _> = [
            (key(":code"), Term::Nil),
            (key(":kind"), Term::Str(RESULT_KIND.to_string())),
            (key(":message"), Term::Nil),
            (key(":ok"), Term::Bool(true)),
            (key(":request-h"), Term::Str(hash_hex(request_hash))),
            (key(":v"), Term::Int(1.into())),
        ]
        .into_iter()
        .collect();
        if let Some((name, value)) = extra {
            fields.insert(key(name), value);
        }
        Term::Map(fields)
    }

    #[test]
    fn result_decoder_rejects_open_results() {
        let hash = [7; 32];
        let error = decode_result(result(hash, Some((":invented", Term::Bool(true)))), hash)
            .expect_err("authority result field set must be closed");
        assert!(error.to_string().contains("field set mismatch"));
    }

    #[test]
    fn result_decoder_rejects_unbound_results() {
        let error = decode_result(result([8; 32], None), [9; 32])
            .expect_err("authority result must bind the exact request");
        assert!(error.to_string().contains("identity mismatch"));
    }
}
