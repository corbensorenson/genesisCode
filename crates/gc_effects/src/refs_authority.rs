use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_with_mode};

use crate::EffectsError;
use crate::policy::SelfhostAuthorityConfig;
use crate::refs::{RefEntry, RefsDb, SetResult};

#[path = "refs_authority_bulk.rs"]
mod bulk;
pub(crate) use bulk::{BulkSetInput, BulkSetMode, BulkSetResult};

const BINDING: &str = "core/refs::authority";
const REQUEST_KIND: &str = "genesis/refs-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/refs-authority-result-v0.1";
const STEP_LIMIT: u64 = 20_000_000;
const ALLOC_LIMIT: u64 = 80_000_000;
const MAX_RETRIES: usize = 16;

pub(crate) struct RefsAuthority {
    context: EvalCtx,
    authority: Value,
}

impl RefsAuthority {
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
        let authority = environment
            .get(BINDING)
            .ok_or_else(|| authority_error(format!("missing binding {BINDING}")))?;
        context.reset_counters();
        context.step_limit = Some(STEP_LIMIT);
        Ok(Self { context, authority })
    }

    pub(crate) fn get(
        &mut self,
        refs: &RefsDb,
        name: &str,
    ) -> Result<Option<String>, EffectsError> {
        let snapshot = refs.snapshot()?;
        let payload = map([
            (":name", Term::Str(name.to_string())),
            (":refs", refs_term(&snapshot)),
        ]);
        let term = self.evaluate(":get", payload)?;
        decode_get(term, &snapshot, name)
    }

    pub(crate) fn list(
        &mut self,
        refs: &RefsDb,
        prefix: Option<&str>,
    ) -> Result<Vec<RefEntry>, EffectsError> {
        let snapshot = refs.snapshot()?;
        let payload = map([
            (
                ":prefix",
                prefix
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (":refs", refs_term(&snapshot)),
        ]);
        let term = self.evaluate(":list", payload)?;
        decode_list(term, &snapshot, prefix)
    }

    pub(crate) fn set(
        &mut self,
        refs: &RefsDb,
        name: &str,
        new_hash: Option<&str>,
        expected_old: Option<Option<&str>>,
    ) -> Result<SetResult, EffectsError> {
        for _ in 0..MAX_RETRIES {
            let snapshot = refs.snapshot()?;
            let payload = map([
                (
                    ":expected-old",
                    expected_old
                        .flatten()
                        .map(|value| Term::Str(value.to_string()))
                        .unwrap_or(Term::Nil),
                ),
                (":expected-old-present", Term::Bool(expected_old.is_some())),
                (":name", Term::Str(name.to_string())),
                (
                    ":new-hash",
                    new_hash
                        .map(|value| Term::Str(value.to_string()))
                        .unwrap_or(Term::Nil),
                ),
                (":refs", refs_term(&snapshot)),
            ]);
            let term = self.evaluate(":set", payload)?;
            match decode_set(term, &snapshot, name, new_hash, expected_old)? {
                SetAuthorityDecision::Conflict(current) => {
                    return Ok(SetResult::Conflict { current });
                }
                SetAuthorityDecision::Write(replacement) => {
                    if refs.replace_if_unchanged(&snapshot, &replacement)? {
                        return Ok(SetResult::Updated);
                    }
                }
            }
        }
        Err(authority_error(format!(
            "reference snapshot changed during all {MAX_RETRIES} authorized write attempts"
        )))
    }

    fn evaluate(&mut self, op: &str, payload: Term) -> Result<DecodedResult, EffectsError> {
        let request = map([
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":op", Term::symbol(op)),
            (":payload", payload),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("apply failed: {error}")))?;
        let term = plain_result(value, &self.context)?;
        decode_result(term, request_hash)
    }
}

struct DecodedResult {
    action: String,
    current: Option<String>,
    entries: Vec<RefEntry>,
    refs: Option<BTreeMap<String, String>>,
    value: Option<String>,
}

enum SetAuthorityDecision {
    Conflict(Option<String>),
    Write(BTreeMap<String, String>),
}

fn decode_result(term: Term, request_hash: [u8; 32]) -> Result<DecodedResult, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":action",
            ":code",
            ":current",
            ":entries",
            ":kind",
            ":message",
            ":ok",
            ":refs",
            ":request-h",
            ":v",
            ":value",
        ],
    )?;
    require_string(fields, ":kind", RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if !required_bool(fields, ":ok")? {
        let code = optional_string(fields, ":code")?.unwrap_or("core/refs/bad-authority-request");
        let message = optional_string(fields, ":message")?.unwrap_or("authority rejected request");
        return Err(authority_error(format!("{code}: {message}")));
    }
    require_nil(fields, ":code")?;
    require_nil(fields, ":message")?;
    Ok(DecodedResult {
        action: required_symbol(fields, ":action")?,
        current: optional_hash(fields, ":current")?,
        entries: required_entries(fields, ":entries")?,
        refs: optional_refs(fields, ":refs")?,
        value: optional_hash(fields, ":value")?,
    })
}

fn decode_get(
    result: DecodedResult,
    snapshot: &BTreeMap<String, String>,
    name: &str,
) -> Result<Option<String>, EffectsError> {
    if result.action != ":read"
        || result.current.is_some()
        || !result.entries.is_empty()
        || result.refs.is_some()
    {
        return Err(authority_error("get result shape contradiction"));
    }
    let expected = snapshot.get(name).cloned();
    if result.value != expected {
        return Err(authority_error("get result value contradiction"));
    }
    Ok(result.value)
}

fn decode_list(
    result: DecodedResult,
    snapshot: &BTreeMap<String, String>,
    prefix: Option<&str>,
) -> Result<Vec<RefEntry>, EffectsError> {
    if result.action != ":list"
        || result.current.is_some()
        || result.refs.is_some()
        || result.value.is_some()
    {
        return Err(authority_error("list result shape contradiction"));
    }
    let expected: Vec<RefEntry> = snapshot
        .iter()
        .filter(|(name, _)| prefix.is_none_or(|prefix| name.starts_with(prefix)))
        .map(|(name, hash)| RefEntry {
            name: name.clone(),
            hash: Some(hash.clone()),
        })
        .collect();
    if result.entries.len() != expected.len()
        || result
            .entries
            .iter()
            .zip(&expected)
            .any(|(left, right)| left.name != right.name || left.hash != right.hash)
    {
        return Err(authority_error("list result entries contradiction"));
    }
    Ok(result.entries)
}

fn decode_set(
    result: DecodedResult,
    snapshot: &BTreeMap<String, String>,
    name: &str,
    new_hash: Option<&str>,
    expected_old: Option<Option<&str>>,
) -> Result<SetAuthorityDecision, EffectsError> {
    if !result.entries.is_empty() || result.value.is_some() {
        return Err(authority_error("set result shape contradiction"));
    }
    let current = snapshot.get(name).cloned();
    if result.current != current {
        return Err(authority_error("set current value contradiction"));
    }
    let expected_matches = expected_old.is_none_or(|expected| expected == current.as_deref());
    match result.action.as_str() {
        ":conflict" => {
            if result.refs.is_some() || expected_matches {
                return Err(authority_error("set conflict decision contradiction"));
            }
            Ok(SetAuthorityDecision::Conflict(result.current))
        }
        ":write" => {
            if !expected_matches {
                return Err(authority_error("set write decision contradiction"));
            }
            let replacement = result
                .refs
                .ok_or_else(|| authority_error("set write result missing refs snapshot"))?;
            let mut expected = snapshot.clone();
            match new_hash {
                Some(hash) => {
                    expected.insert(name.to_string(), hash.to_string());
                }
                None => {
                    expected.remove(name);
                }
            }
            if replacement != expected {
                return Err(authority_error("set replacement snapshot contradiction"));
            }
            Ok(SetAuthorityDecision::Write(replacement))
        }
        _ => Err(authority_error(format!(
            "unsupported set result action {}",
            result.action
        ))),
    }
}

fn authority_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!("selfhost refs authority: {}", message.into()))
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn refs_term(refs: &BTreeMap<String, String>) -> Term {
    Term::Map(
        refs.iter()
            .map(|(name, hash)| (TermOrdKey(Term::Str(name.clone())), Term::Str(hash.clone())))
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
        return Err(authority_error(format!(
            "result must be a map, got {}",
            print_term(term)
        )));
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
        return Err(authority_error("result field set mismatch"));
    }
    Ok(fields)
}

fn field<'a>(fields: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> Result<&'a Term, EffectsError> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| authority_error(format!("result missing {name}")))
}

fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), EffectsError> {
    match field(fields, name)? {
        Term::Str(value) if value == expected => Ok(()),
        _ => Err(authority_error(format!("result {name} mismatch"))),
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

fn required_symbol(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<String, EffectsError> {
    match field(fields, name)? {
        Term::Symbol(value) => Ok(value.clone()),
        _ => Err(authority_error(format!("result {name} must be symbol"))),
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

fn optional_hash(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, EffectsError> {
    let value = optional_string(fields, name)?.map(str::to_string);
    if value.as_deref().is_some_and(|value| !is_hash(value)) {
        return Err(authority_error(format!(
            "result {name} must be lowercase hex64 or nil"
        )));
    }
    Ok(value)
}

fn optional_refs(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<BTreeMap<String, String>>, EffectsError> {
    match field(fields, name)? {
        Term::Nil => Ok(None),
        Term::Map(entries) => {
            let mut out = BTreeMap::new();
            for (key, value) in entries {
                let (Term::Str(name), Term::Str(hash)) = (&key.0, value) else {
                    return Err(authority_error("result refs must map strings to strings"));
                };
                if !is_hash(hash) {
                    return Err(authority_error("result refs contains a non-hash value"));
                }
                out.insert(name.clone(), hash.clone());
            }
            Ok(Some(out))
        }
        _ => Err(authority_error(format!("result {name} must be map or nil"))),
    }
}

fn required_entries(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Vec<RefEntry>, EffectsError> {
    let Term::Vector(values) = field(fields, name)? else {
        return Err(authority_error(format!("result {name} must be vector")));
    };
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let entry = exact_map(value, &[":hash", ":name"])?;
        let hash = optional_hash(entry, ":hash")?;
        let name = optional_string(entry, ":name")?
            .ok_or_else(|| authority_error("result entry name must not be nil"))?
            .to_string();
        out.push(RefEntry { name, hash });
    }
    Ok(out)
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

    fn hash(byte: char) -> String {
        std::iter::repeat_n(byte, 64).collect()
    }

    fn decoded(action: &str) -> DecodedResult {
        DecodedResult {
            action: action.to_string(),
            current: None,
            entries: Vec::new(),
            refs: None,
            value: None,
        }
    }

    #[test]
    fn decoder_rejects_lookup_and_list_substitution() {
        let snapshot = BTreeMap::from([
            ("refs/heads/dev".to_string(), hash('a')),
            ("refs/heads/main".to_string(), hash('b')),
        ]);
        let mut get = decoded(":read");
        get.value = Some(hash('b'));
        assert!(decode_get(get, &snapshot, "refs/heads/dev").is_err());

        let mut list = decoded(":list");
        list.entries.push(RefEntry {
            name: "refs/heads/main".to_string(),
            hash: Some(hash('b')),
        });
        assert!(decode_list(list, &snapshot, Some("refs/heads/")).is_err());
    }

    #[test]
    fn decoder_rejects_cas_action_and_replacement_substitution() {
        let name = "refs/heads/main";
        let old = hash('a');
        let new = hash('b');
        let snapshot = BTreeMap::from([(name.to_string(), old.clone())]);

        let mut false_conflict = decoded(":conflict");
        false_conflict.current = Some(old.clone());
        assert!(
            decode_set(
                false_conflict,
                &snapshot,
                name,
                Some(&new),
                Some(Some(&old))
            )
            .is_err()
        );

        let mut smuggled = decoded(":write");
        smuggled.current = Some(old.clone());
        smuggled.refs = Some(BTreeMap::from([
            (name.to_string(), new.clone()),
            ("refs/heads/smuggled".to_string(), hash('c')),
        ]));
        assert!(decode_set(smuggled, &snapshot, name, Some(&new), None).is_err());
    }

    #[test]
    fn decoder_rejects_unbound_and_open_results() {
        let request_hash = [7_u8; 32];
        let valid = map([
            (":action", Term::symbol(":read")),
            (":code", Term::Nil),
            (":current", Term::Nil),
            (":entries", Term::Vector(Vec::new())),
            (":kind", Term::Str(RESULT_KIND.to_string())),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":refs", Term::Nil),
            (":request-h", Term::Str(hex32(request_hash))),
            (":v", Term::Int(1.into())),
            (":value", Term::Nil),
        ]);
        assert!(decode_result(valid.clone(), request_hash).is_ok());
        assert!(decode_result(valid.clone(), [8_u8; 32]).is_err());

        let mut open = match valid {
            Term::Map(fields) => fields,
            _ => BTreeMap::new(),
        };
        open.insert(TermOrdKey(Term::symbol(":extra")), Term::Nil);
        assert!(decode_result(Term::Map(open), request_hash).is_err());
    }
}
