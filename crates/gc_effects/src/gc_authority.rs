use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_with_mode};
use num_traits::ToPrimitive;

use crate::EffectsError;
use crate::policy::{CapsPolicy, SelfhostAuthorityConfig};

const BINDING: &str = "core/gc::authority";
const REQUEST_KIND: &str = "genesis/gc-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/gc-authority-result-v0.1";
const STEP_LIMIT: u64 = 80_000_000;
const ALLOC_LIMIT: u64 = 320_000_000;
const MAX_ITEMS: u64 = 65_536;

pub(crate) struct GcAuthority {
    context: EvalCtx,
    authority: Value,
}

#[derive(Debug)]
pub(crate) struct GcRootsPlan {
    pub(crate) roots: Vec<String>,
    pub(crate) metadata: Vec<Term>,
}

#[derive(Debug)]
pub(crate) struct GcEdgePlan {
    pub(crate) refs: Vec<String>,
    pub(crate) parents: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct GcDeadPlan {
    pub(crate) dead: Vec<String>,
    pub(crate) reclaim_bytes: u64,
    pub(crate) largest: Vec<(String, u64)>,
}

#[derive(Debug)]
pub(crate) struct GcPinsPlan {
    pub(crate) body: Vec<u8>,
    pub(crate) keep: Vec<String>,
    pub(crate) keep_refs: Vec<String>,
}

impl GcAuthority {
    pub(crate) fn ensure(
        slot: &mut Option<Self>,
        op: &str,
        policy: &CapsPolicy,
    ) -> Result<(), EffectsError> {
        if slot.is_some() || !op.starts_with("core/gc-low::") {
            return Ok(());
        }
        let config = policy.selfhost_authority_config().ok_or_else(|| {
            EffectsError::Log(
                "artifact GC requires the artifact-loaded GenesisCode authority".to_string(),
            )
        })?;
        *slot = Some(Self::load(config)?);
        Ok(())
    }

    pub(crate) fn load(config: &SelfhostAuthorityConfig) -> Result<Self, EffectsError> {
        let mut context = EvalCtx::with_step_limit(None);
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(ALLOC_LIMIT),
            max_bytes_len: Some(8 * 1024 * 1024),
            max_map_len: Some(MAX_ITEMS),
            max_string_len: Some(8 * 1024 * 1024),
            max_vec_len: Some(MAX_ITEMS),
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

    pub(crate) fn roots(
        &mut self,
        refs: Vec<Term>,
        lock: Term,
        pins: Term,
        include_lock: bool,
        include_refs: bool,
    ) -> Result<GcRootsPlan, EffectsError> {
        let value = self.evaluate(
            ":roots",
            map([
                (":include-lock", Term::Bool(include_lock)),
                (":include-refs", Term::Bool(include_refs)),
                (":lock", lock),
                (":pins", pins),
                (":refs", Term::Vector(refs)),
            ]),
        )?;
        let fields = exact_map(&value, &[":metadata", ":roots"])?;
        let roots = hash_vector(field(fields, ":roots")?, ":roots")?;
        require_sorted_unique(&roots, ":roots")?;
        let Term::Vector(metadata) = field(fields, ":metadata")? else {
            return Err(authority_error("roots :metadata must be a vector"));
        };
        if metadata.len() != roots.len() {
            return Err(authority_error(
                "roots :metadata must bind one entry to every canonical root",
            ));
        }
        Ok(GcRootsPlan {
            roots,
            metadata: metadata.clone(),
        })
    }

    pub(crate) fn artifact_edges(
        &mut self,
        artifact: Term,
        include_deps: bool,
        include_evidence: bool,
        include_parents: bool,
    ) -> Result<GcEdgePlan, EffectsError> {
        let value = self.evaluate(
            ":artifact-edges",
            map([
                (":artifact", artifact),
                (":include-deps", Term::Bool(include_deps)),
                (":include-evidence", Term::Bool(include_evidence)),
                (":include-parents", Term::Bool(include_parents)),
            ]),
        )?;
        let fields = exact_map(&value, &[":parents", ":refs"])?;
        let refs = hash_vector(field(fields, ":refs")?, ":refs")?;
        let parents = hash_vector(field(fields, ":parents")?, ":parents")?;
        require_sorted_unique(&refs, ":refs")?;
        require_sorted_unique(&parents, ":parents")?;
        Ok(GcEdgePlan { refs, parents })
    }

    pub(crate) fn dead_plan(
        &mut self,
        live: Vec<String>,
        inventory: Vec<(String, u64)>,
    ) -> Result<GcDeadPlan, EffectsError> {
        let value = self.evaluate(
            ":dead-plan",
            map([
                (":inventory", inventory_term(&inventory)),
                (":live", strings_term(&live)),
            ]),
        )?;
        let fields = exact_map(&value, &[":dead", ":largest", ":reclaim-bytes"])?;
        let dead = hash_vector(field(fields, ":dead")?, ":dead")?;
        require_sorted_unique(&dead, ":dead")?;
        let reclaim_bytes = required_u64(fields, ":reclaim-bytes")?;
        let largest = inventory_vector(field(fields, ":largest")?, ":largest")?;
        if largest.len() > 25 {
            return Err(authority_error("dead plan :largest exceeds 25 entries"));
        }
        Ok(GcDeadPlan {
            dead,
            reclaim_bytes,
            largest,
        })
    }

    pub(crate) fn update_pins(
        &mut self,
        action: &str,
        target: &str,
        document: Term,
    ) -> Result<GcPinsPlan, EffectsError> {
        let value = self.evaluate(
            ":pins-update",
            map([
                (":action", Term::symbol(action)),
                (":document", document),
                (":target", Term::Str(target.to_string())),
            ]),
        )?;
        let fields = exact_map(&value, &[":body", ":keep", ":keep-refs"])?;
        let Term::Bytes(body) = field(fields, ":body")? else {
            return Err(authority_error("pins update :body must be bytes"));
        };
        let keep = hash_vector(field(fields, ":keep")?, ":keep")?;
        let keep_refs = string_vector(field(fields, ":keep-refs")?, ":keep-refs")?;
        require_sorted_unique(&keep, ":keep")?;
        require_sorted_unique(&keep_refs, ":keep-refs")?;
        if keep_refs.iter().any(|name| !name.starts_with("refs/")) {
            return Err(authority_error(
                "pins update :keep-refs contains a non-reference name",
            ));
        }
        Ok(GcPinsPlan {
            body: body.to_vec(),
            keep,
            keep_refs,
        })
    }

    pub(crate) fn purge_plan(
        &mut self,
        ttl_seconds: u64,
        inventory: Vec<(String, u64)>,
    ) -> Result<Vec<String>, EffectsError> {
        let value = self.evaluate(
            ":purge-plan",
            map([
                (":inventory", inventory_term(&inventory)),
                (":ttl-seconds", Term::Int(ttl_seconds.into())),
            ]),
        )?;
        let fields = exact_map(&value, &[":purge"])?;
        let purge = hash_vector(field(fields, ":purge")?, ":purge")?;
        require_sorted_unique(&purge, ":purge")?;
        Ok(purge)
    }

    fn evaluate(&mut self, op: &str, payload: Term) -> Result<Term, EffectsError> {
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
        require_string(fields, ":kind", RESULT_KIND)?;
        require_int(fields, ":v", 1)?;
        require_string(fields, ":request-h", &hex32(request_hash))?;
        if !required_bool(fields, ":ok")? {
            require_nil(fields, ":value")?;
            let code = required_string(fields, ":code")?;
            if !matches!(code, "core/gc/bad-authority-request" | "core/gc/bad-pins") {
                return Err(authority_error(
                    "result rejection code is outside closed inventory",
                ));
            }
            return Err(authority_error(format!(
                "{code}: {}",
                required_string(fields, ":message")?
            )));
        }
        require_nil(fields, ":code")?;
        require_nil(fields, ":message")?;
        Ok(field(fields, ":value")?.clone())
    }
}

fn authority_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!("selfhost gc authority: {}", message.into()))
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn strings_term(values: &[String]) -> Term {
    Term::Vector(values.iter().cloned().map(Term::Str).collect())
}

fn inventory_term(values: &[(String, u64)]) -> Term {
    Term::Vector(
        values
            .iter()
            .map(|(hash, value)| {
                map([
                    (":hash", Term::Str(hash.clone())),
                    (":value", Term::Int((*value).into())),
                ])
            })
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
            "result field set mismatch: expected {wanted:?}, got {actual:?}"
        )));
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

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, EffectsError> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(authority_error(format!("result {name} must be string"))),
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

fn required_u64(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<u64, EffectsError> {
    match field(fields, name)? {
        Term::Int(value) => value
            .to_u64()
            .ok_or_else(|| authority_error(format!("result {name} must fit u64"))),
        _ => Err(authority_error(format!("result {name} must be int"))),
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

fn string_vector(term: &Term, name: &str) -> Result<Vec<String>, EffectsError> {
    let Term::Vector(values) = term else {
        return Err(authority_error(format!("result {name} must be vector")));
    };
    values
        .iter()
        .map(|value| match value {
            Term::Str(value) => Ok(value.clone()),
            _ => Err(authority_error(format!(
                "result {name} entries must be strings"
            ))),
        })
        .collect()
}

fn hash_vector(term: &Term, name: &str) -> Result<Vec<String>, EffectsError> {
    let values = string_vector(term, name)?;
    if values.iter().any(|value| !lowercase_hash(value)) {
        return Err(authority_error(format!(
            "result {name} contains a non-canonical artifact identity"
        )));
    }
    Ok(values)
}

fn inventory_vector(term: &Term, name: &str) -> Result<Vec<(String, u64)>, EffectsError> {
    let Term::Vector(values) = term else {
        return Err(authority_error(format!("result {name} must be vector")));
    };
    values
        .iter()
        .map(|value| {
            let fields = exact_map(value, &[":hash", ":value"])?;
            let hash = required_string(fields, ":hash")?.to_string();
            if !lowercase_hash(&hash) {
                return Err(authority_error(format!(
                    "result {name} contains a non-canonical artifact identity"
                )));
            }
            Ok((hash, required_u64(fields, ":value")?))
        })
        .collect()
}

fn require_sorted_unique(values: &[String], name: &str) -> Result<(), EffectsError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(authority_error(format!(
            "result {name} must be strictly sorted and unique"
        )));
    }
    Ok(())
}

fn lowercase_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn hex32(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}
