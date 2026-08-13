use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_with_mode};
use num_traits::ToPrimitive;

use crate::EffectsError;
use crate::policy::SelfhostAuthorityConfig;

#[path = "store_authority_read.rs"]
mod read;
pub(crate) use read::{StoreGetDecision, StoreHasDecision};

const BINDING: &str = "core/store::authority";
const REQUEST_KIND: &str = "genesis/store-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/store-authority-result-v0.1";
const STEP_LIMIT: u64 = 20_000_000;
const ALLOC_LIMIT: u64 = 160_000_000;
const PAYLOAD_LIMIT: u64 = 40 * 1024 * 1024;

pub(crate) enum StorePutDecision {
    Write {
        bytes: Vec<u8>,
        hash: String,
        written_bytes: usize,
    },
    Error {
        code: String,
        message: String,
    },
}

pub(crate) struct StoreAuthority {
    context: EvalCtx,
    authority: Value,
}

impl StoreAuthority {
    pub(crate) fn load(config: &SelfhostAuthorityConfig) -> Result<Self, EffectsError> {
        let mut context = EvalCtx::with_step_limit(None);
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(ALLOC_LIMIT),
            max_bytes_len: Some(PAYLOAD_LIMIT),
            max_map_len: Some(32),
            max_string_len: Some(PAYLOAD_LIMIT),
            max_vec_len: Some(16_384),
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
        let authority = environment
            .get(BINDING)
            .ok_or_else(|| authority_error(format!("missing binding {BINDING}")))?;
        context.reset_counters();
        context.step_limit = Some(STEP_LIMIT);
        Ok(Self { context, authority })
    }

    pub(crate) fn put(
        &mut self,
        payload: &Term,
        max_bytes: usize,
        budget_used: usize,
        budget_limit: Option<usize>,
    ) -> Result<StorePutDecision, EffectsError> {
        let request = map([
            (
                ":budget-limit",
                budget_limit
                    .map(|value| Term::Int(value.into()))
                    .unwrap_or(Term::Nil),
            ),
            (":budget-used", Term::Int(budget_used.into())),
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":max-bytes", Term::Int(max_bytes.into())),
            (":payload", payload.clone()),
            (":phase", Term::symbol(":put")),
            (":v", Term::Int(1.into())),
        ]);
        let (term, request_hash) = self.evaluate(request)?;
        decode_put_result(term, request_hash)
    }

    fn evaluate(&mut self, request: Term) -> Result<(Term, [u8; 32]), EffectsError> {
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("apply failed: {error}")))?;
        let term = plain_result(value, &self.context)?;
        Ok((term, request_hash))
    }
}

fn authority_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!("selfhost store authority: {}", message.into()))
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

fn decode_put_result(term: Term, request_hash: [u8; 32]) -> Result<StorePutDecision, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":action",
            ":bytes",
            ":code",
            ":hash",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
            ":written-bytes",
        ],
    )?;
    require_string(fields, ":kind", RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if !required_bool(fields, ":ok")? {
        let code = optional_string(fields, ":code")?.unwrap_or("store/authority-request");
        let message = optional_string(fields, ":message")?.unwrap_or("authority rejected request");
        return Err(authority_error(format!("{code}: {message}")));
    }
    let action = required_symbol(fields, ":action")?;
    match action.as_str() {
        ":write" => {
            require_nil(fields, ":code")?;
            require_nil(fields, ":message")?;
            let bytes = required_bytes(fields, ":bytes")?;
            let hash = required_string(fields, ":hash")?.to_string();
            let written_bytes = required_usize(fields, ":written-bytes")?;
            if written_bytes != bytes.len() {
                return Err(authority_error("write byte count contradiction"));
            }
            let observed_hash = blake3::hash(&bytes).to_hex().to_string();
            if hash != observed_hash {
                return Err(authority_error("write hash/bytes contradiction"));
            }
            Ok(StorePutDecision::Write {
                bytes,
                hash,
                written_bytes,
            })
        }
        ":error" => {
            require_nil(fields, ":bytes")?;
            require_nil(fields, ":hash")?;
            require_nil(fields, ":written-bytes")?;
            Ok(StorePutDecision::Error {
                code: required_string(fields, ":code")?.to_string(),
                message: required_string(fields, ":message")?.to_string(),
            })
        }
        _ => Err(authority_error(format!("unsupported action {action}"))),
    }
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
            "result field set mismatch: expected {wanted:?}, got {actual:?}"
        )));
    }
    Ok(fields)
}

fn field<'a>(fields: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> Result<&'a Term, EffectsError> {
    fields
        .get(&key(name))
        .ok_or_else(|| authority_error(format!("result missing {name}")))
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), EffectsError> {
    if matches!(field(fields, name)?, Term::Nil) {
        Ok(())
    } else {
        Err(authority_error(format!("result {name} must be nil")))
    }
}

fn required_bool(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<bool, EffectsError> {
    match field(fields, name)? {
        Term::Bool(value) => Ok(*value),
        _ => Err(authority_error(format!("result {name} must be bool"))),
    }
}

fn required_symbol<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a String, EffectsError> {
    match field(fields, name)? {
        Term::Symbol(value) => Ok(value),
        _ => Err(authority_error(format!("result {name} must be symbol"))),
    }
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

fn optional_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<&'a str>, EffectsError> {
    match field(fields, name)? {
        Term::Nil => Ok(None),
        Term::Str(value) => Ok(Some(value)),
        _ => Err(authority_error(format!(
            "result {name} must be string or nil"
        ))),
    }
}

fn required_bytes(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Vec<u8>, EffectsError> {
    match field(fields, name)? {
        Term::Bytes(value) => Ok(value.to_vec()),
        _ => Err(authority_error(format!("result {name} must be bytes"))),
    }
}

fn required_usize(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<usize, EffectsError> {
    match field(fields, name)? {
        Term::Int(value) => value
            .to_usize()
            .ok_or_else(|| authority_error(format!("result {name} must fit usize"))),
        _ => Err(authority_error(format!("result {name} must be int"))),
    }
}

fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), EffectsError> {
    let actual = required_string(fields, name)?;
    if actual == expected {
        Ok(())
    } else {
        Err(authority_error(format!(
            "result {name} mismatch: expected {expected}, got {actual}"
        )))
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: u64,
) -> Result<(), EffectsError> {
    match field(fields, name)? {
        Term::Int(value) if value == &expected.into() => Ok(()),
        _ => Err(authority_error(format!("result {name} must be {expected}"))),
    }
}

fn hex32(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}
