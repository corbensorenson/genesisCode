use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
#[cfg(not(debug_assertions))]
use std::sync::OnceLock;

use crate::Shared;
use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use blake3::Hasher;
use gc_coreform::{HASH_DOMAIN_PREFIX, Term, TermOrdKey, hash_term, print_term};

mod collections;
#[cfg(test)]
mod tests;
mod trace_impl;

pub use collections::{ValueMap, ValueVector};

pub const VALUE_EFFECT_HASH_PROFILE_ID: &str = "genesis/value-effect-hash/v0.2";

pub type Sym = Rc<str>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SealId(pub u64);

/// Opaque compiled expression handle.
///
/// This is intentionally not constructible by user code; it exists so the runtime can carry
/// compiled closures without exposing the compiler IR as part of the public API.
#[derive(Clone, Debug)]
pub struct CompiledExpr {
    expr: Arc<crate::compiled::CExpr>,
    coverage_sites: Arc<crate::compiled::CompiledCoverageSites>,
}

impl CompiledExpr {
    pub(crate) fn new(
        expr: Arc<crate::compiled::CExpr>,
        coverage_sites: Arc<crate::compiled::CompiledCoverageSites>,
    ) -> Self {
        Self {
            expr,
            coverage_sites,
        }
    }

    pub(crate) fn inner(&self) -> &Arc<crate::compiled::CExpr> {
        &self.expr
    }

    pub(crate) fn coverage_sites(&self) -> &Arc<crate::compiled::CompiledCoverageSites> {
        &self.coverage_sites
    }
}

#[derive(Clone, Debug)]
pub enum Value {
    Data(Rc<Term>),
    Int(i64),
    Vector(Shared<ValueVector>),
    Map(Shared<ValueMap>),
    Closure(Shared<ClosureData>),
    /// A compiled closure stores its original `body` term (for stable hashing/logging) and a
    /// compiled expression for faster evaluation.
    CompiledClosure(Shared<CompiledClosureData>),
    SealToken(SealId),
    Sealed {
        token: SealId,
        payload: Box<Value>,
    },
    NativeFn(Shared<NativeFn>),
    Contract(Shared<Contract>),
    EffectProgram(Box<EffectProgram>),
    EffectRequest(Shared<EffectRequest>),
}

#[derive(Clone, Debug)]
pub struct ClosureData {
    pub param: Sym,
    pub body: Term,
    pub env: Env,
}

#[derive(Clone, Debug)]
pub struct CompiledClosureData {
    pub param: Sym,
    pub body: Term,
    pub body_c: CompiledExpr,
    pub env: Env,
    pub compiled_env: Option<crate::compiled::CompiledLexicalEnv>,
    pub module_env: Option<crate::compiled::CompiledModuleCells>,
    pub(crate) primitive_forward_plan:
        Option<std::sync::Arc<crate::compiled::PrimitiveForwardPlan>>,
}

#[derive(Clone)]
pub struct NativeFn {
    pub name: &'static str,
    pub arity: usize,
    pub collected: Vec<Value>,
    pub func: fn(&mut crate::eval::EvalCtx, Vec<Value>) -> Result<Value, KernelError>,
}

impl fmt::Debug for NativeFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeFn")
            .field("name", &self.name)
            .field("arity", &self.arity)
            .field("collected", &self.collected.len())
            .finish()
    }
}

impl NativeFn {
    pub fn new(
        name: &'static str,
        arity: usize,
        func: fn(&mut crate::eval::EvalCtx, Vec<Value>) -> Result<Value, KernelError>,
    ) -> Self {
        Self {
            name,
            arity,
            collected: Vec::new(),
            func,
        }
    }

    pub fn apply(&self, ctx: &mut crate::eval::EvalCtx, arg: Value) -> Result<Value, KernelError> {
        let mut collected = self.collected.clone();
        collected.push(arg);
        ctx.run_panic_guarded("native function application", |ctx| {
            self.apply_collected(ctx, collected)
        })
    }

    pub(crate) fn apply_collected(
        &self,
        ctx: &mut crate::eval::EvalCtx,
        collected: Vec<Value>,
    ) -> Result<Value, KernelError> {
        if collected.len() < self.arity {
            Ok(Value::native_fn(NativeFn {
                name: self.name,
                arity: self.arity,
                collected,
                func: self.func,
            }))
        } else if collected.len() == self.arity {
            if native_per_call_panic_guard_enabled() {
                ctx.run_panic_guarded_always("native function application", |ctx| {
                    (self.func)(ctx, collected)
                })
            } else {
                (self.func)(ctx, collected)
            }
        } else {
            Err(KernelError::new(
                KernelErrorKind::BadForm,
                format!("native fn {} applied to too many args", self.name),
            ))
        }
    }
}

fn native_per_call_panic_guard_enabled() -> bool {
    #[cfg(debug_assertions)]
    {
        std::env::var_os("GENESIS_NATIVE_PANIC_GUARD_EACH").is_some()
    }
    #[cfg(not(debug_assertions))]
    {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("GENESIS_NATIVE_PANIC_GUARD_EACH").is_some())
    }
}

#[derive(Clone, Debug)]
pub struct Contract {
    pub handler: Value,
    pub proto: Option<Shared<Contract>>,
    pub meta: Value,
    pub overrides: BTreeMap<String, Value>,
    pub shape_id: [u8; 32],
    pub contract_id: [u8; 32],
}

#[derive(Clone, Debug)]
pub enum EffectProgram {
    Pure(Box<Value>),
    Perform { request: Box<Value> },
}

#[derive(Clone, Debug)]
pub struct EffectRequest {
    pub op: String,
    pub payload: Term,
    pub k: Box<Value>,
}

pub trait Apply {
    fn apply(self, ctx: &mut crate::eval::EvalCtx, arg: Value) -> Result<Value, KernelError>;
}

impl Apply for Value {
    fn apply(self, ctx: &mut crate::eval::EvalCtx, arg: Value) -> Result<Value, KernelError> {
        let result =
            ctx.run_panic_guarded("value application", |ctx| self.apply_inner(ctx, arg))?;
        ctx.finish_with_live_roots(result, &[])
    }
}

impl Value {
    pub fn data(term: Term) -> Self {
        crate::logical_heap::charge_active_with(|| {
            crate::logical_heap::data_allocation_units(&term)
        });
        Self::Data(Rc::new(term))
    }

    pub fn int(n: i64) -> Self {
        crate::logical_heap::charge_active(1);
        Self::Int(n)
    }

    pub fn vector(xs: ValueVector) -> Self {
        crate::logical_heap::charge_active_with(|| {
            crate::logical_heap::vector_allocation_units(xs.len())
        });
        Self::Vector(Shared::new(xs))
    }

    pub fn vector_shared(xs: Shared<ValueVector>) -> Self {
        crate::logical_heap::charge_active_with(|| {
            crate::logical_heap::vector_allocation_units(xs.len())
        });
        Self::Vector(xs)
    }

    pub fn map(m: ValueMap) -> Self {
        crate::logical_heap::charge_active_with(|| {
            crate::logical_heap::map_allocation_units(m.iter().map(|(key, _)| key))
        });
        Self::Map(Shared::new(m))
    }

    pub fn map_shared(m: Shared<ValueMap>) -> Self {
        crate::logical_heap::charge_active_with(|| {
            crate::logical_heap::map_allocation_units(m.iter().map(|(key, _)| key))
        });
        Self::Map(m)
    }

    pub fn closure(param: String, body: Term, env: Env) -> Self {
        crate::logical_heap::charge_active_with(|| {
            crate::logical_heap::closure_allocation_units(&param, &body)
        });
        Self::Closure(Shared::new(ClosureData {
            param: Rc::<str>::from(param),
            body,
            env,
        }))
    }

    pub(crate) fn compiled_closure(
        param: String,
        body: Term,
        body_c: CompiledExpr,
        env: Env,
        compiled_env: Option<crate::compiled::CompiledLexicalEnv>,
        module_env: Option<crate::compiled::CompiledModuleCells>,
        primitive_forward_plan: Option<std::sync::Arc<crate::compiled::PrimitiveForwardPlan>>,
    ) -> Self {
        crate::logical_heap::charge_active_with(|| {
            crate::logical_heap::closure_allocation_units(&param, &body)
        });
        Self::CompiledClosure(Shared::new(CompiledClosureData {
            param: Rc::<str>::from(param),
            body,
            body_c,
            env,
            compiled_env,
            module_env,
            primitive_forward_plan,
        }))
    }

    pub fn native_fn(f: NativeFn) -> Self {
        crate::logical_heap::charge_active_with(|| {
            crate::logical_heap::native_allocation_units(f.name, f.collected.len())
        });
        Self::NativeFn(Shared::new(f))
    }

    pub fn effect_request(r: EffectRequest) -> Self {
        crate::logical_heap::charge_active_with(|| {
            crate::logical_heap::effect_request_allocation_units(&r.op, &r.payload)
        });
        Self::EffectRequest(Shared::new(r))
    }

    pub fn seal_token(id: SealId) -> Self {
        crate::logical_heap::charge_active(1);
        Self::SealToken(id)
    }

    pub fn sealed(token: SealId, payload: Value) -> Self {
        crate::logical_heap::charge_active(2);
        Self::Sealed {
            token,
            payload: Box::new(payload),
        }
    }

    pub fn contract(contract: Contract) -> Self {
        crate::logical_heap::charge_active_with(|| {
            crate::logical_heap::contract_allocation_units(&contract)
        });
        Self::Contract(Shared::new(contract))
    }

    pub fn pure_effect(value: Value) -> Self {
        crate::logical_heap::charge_active(2);
        Self::EffectProgram(Box::new(EffectProgram::Pure(Box::new(value))))
    }

    pub fn perform_effect(request: Value) -> Self {
        crate::logical_heap::charge_active(2);
        Self::EffectProgram(Box::new(EffectProgram::Perform {
            request: Box::new(request),
        }))
    }

    fn apply_inner(self, ctx: &mut crate::eval::EvalCtx, arg: Value) -> Result<Value, KernelError> {
        match self {
            Value::Closure(data) => {
                let env2 = Env::with_binding(&data.env, data.param.as_ref(), arg);
                crate::eval::eval_term(ctx, &env2, &data.body)
            }
            Value::CompiledClosure(data) => crate::compiled::apply_compiled_closure(
                ctx,
                crate::compiled::CompiledClosureCall {
                    external_env: data.env.clone(),
                    lexical_env: data.compiled_env.clone(),
                    module_env: data.module_env.clone(),
                    coverage_sites: data.body_c.coverage_sites().clone(),
                    param: data.param.clone(),
                    bind_external_param: false,
                    body: data.body_c.inner().clone(),
                    arg,
                },
            ),
            Value::NativeFn(f) => f.apply(ctx, arg),
            _ => Err(KernelError::new(
                KernelErrorKind::NotCallable,
                "value is not callable",
            )),
        }
    }
}

pub fn value_hash(v: &Value) -> [u8; 32] {
    ValueHasher::new().hash(v)
}

struct ValueHasher {
    env_cache: std::collections::HashMap<*const crate::env::EnvFrame, (u64, [u8; 32])>,
    env_in_progress: std::collections::HashSet<*const crate::env::EnvFrame>,
}

impl ValueHasher {
    fn new() -> Self {
        Self {
            env_cache: std::collections::HashMap::new(),
            env_in_progress: std::collections::HashSet::new(),
        }
    }

    fn hash(&mut self, v: &Value) -> [u8; 32] {
        let mut h = Hasher::new();
        self.hash_into(&mut h, v);
        *h.finalize().as_bytes()
    }

    fn hash_into(&mut self, h: &mut Hasher, v: &Value) {
        h.update(HASH_DOMAIN_PREFIX);
        h.update(b"value\0");
        match v {
            Value::Data(t) => {
                h.update(b"data\0");
                h.update(&hash_term(t.as_ref()));
            }
            Value::Int(n) => {
                h.update(b"data\0");
                h.update(&hash_term(&Term::Int(num_bigint::BigInt::from(*n))));
            }
            Value::Vector(xs) => {
                h.update(b"vec\0");
                h.update(&(xs.len() as u64).to_le_bytes());
                for x in xs.iter() {
                    let hx = self.hash(x);
                    h.update(&hx);
                }
            }
            Value::Map(m) => {
                h.update(b"map\0");
                h.update(&(m.size() as u64).to_le_bytes());
                for (k, v) in m.iter() {
                    h.update(&hash_term(&k.0));
                    let hv = self.hash(v);
                    h.update(&hv);
                }
            }
            Value::Closure(data) => {
                h.update(b"closure\0");
                h.update(data.param.as_bytes());
                h.update(b"\0");
                h.update(&hash_term(&data.body));
                let he = self.hash_env(&data.env);
                h.update(&he);
            }
            Value::CompiledClosure(data) => {
                h.update(b"closure\0");
                h.update(data.param.as_bytes());
                h.update(b"\0");
                h.update(&hash_term(&data.body));
                let he = self.hash_env(&data.env);
                h.update(&he);
            }
            Value::SealToken(SealId(id)) => {
                h.update(b"seal-token\0");
                h.update(&id.to_le_bytes());
            }
            Value::Sealed { token, payload } => {
                h.update(b"sealed\0");
                h.update(&token.0.to_le_bytes());
                let hp = self.hash(payload);
                h.update(&hp);
            }
            Value::NativeFn(f) => {
                h.update(b"native\0");
                h.update(f.name.as_bytes());
                h.update(b"\0");
                h.update(&(f.arity as u64).to_le_bytes());
                h.update(&(f.collected.len() as u64).to_le_bytes());
                for a in &f.collected {
                    let ha = self.hash(a);
                    h.update(&ha);
                }
            }
            Value::Contract(c) => {
                h.update(b"contract\0");
                h.update(&c.contract_id);
            }
            Value::EffectProgram(p) => {
                h.update(b"effect-program\0");
                match p.as_ref() {
                    EffectProgram::Pure(v) => {
                        h.update(b"pure\0");
                        let hv = self.hash(v);
                        h.update(&hv);
                    }
                    EffectProgram::Perform { request } => {
                        h.update(b"perform\0");
                        let hr = self.hash(request);
                        h.update(&hr);
                    }
                }
            }
            Value::EffectRequest(r) => {
                h.update(b"effect-req\0");
                h.update(r.op.as_bytes());
                h.update(b"\0");
                h.update(&hash_term(&r.payload));
                let hk = self.hash(&r.k);
                h.update(&hk);
            }
        }
    }

    fn hash_env(&mut self, env: &Env) -> [u8; 32] {
        let ptr = env.0.as_ref() as *const crate::env::EnvFrame;
        let rev = env.0.rev.get();

        // Recursive module scopes can create cyclic env graphs (e.g., a function bound in the env
        // closes over the env that binds the function). We define a total hash by breaking cycles
        // with a stable marker.
        if !self.env_in_progress.insert(ptr) {
            let mut h = Hasher::new();
            h.update(HASH_DOMAIN_PREFIX);
            h.update(b"env-cycle\0");
            return *h.finalize().as_bytes();
        }
        if let Some((cached_rev, h)) = self.env_cache.get(&ptr)
            && *cached_rev == rev
        {
            self.env_in_progress.remove(&ptr);
            return *h;
        }

        let mut h = Hasher::new();
        h.update(HASH_DOMAIN_PREFIX);
        h.update(b"env\0");

        // Parent hash first to preserve a stable chain structure.
        if let Some(parent) = &env.0.parent {
            let hp = self.hash_env(parent);
            h.update(b"parent\0");
            h.update(&hp);
        } else {
            h.update(b"parent\0nil\0");
        }

        h.update(b"binds\0");
        let binds = env.0.binds.borrow();
        h.update(&(binds.len() as u64).to_le_bytes());
        for (k, v) in binds.iter() {
            h.update(k.as_bytes());
            h.update(b"\0");
            let hv = self.hash(v);
            h.update(&hv);
        }

        let out = *h.finalize().as_bytes();
        self.env_cache.insert(ptr, (rev, out));
        self.env_in_progress.remove(&ptr);
        out
    }
}

impl Value {
    pub fn as_symbol(&self) -> Option<&str> {
        match self {
            Value::Data(t) => match t.as_ref() {
                Term::Symbol(s) => Some(s.as_str()),
                _ => None,
            },
            Value::Int(_) => None,
            _ => None,
        }
    }

    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Data(t) if matches!(t.as_ref(), Term::Nil | Term::Bool(false)))
    }

    pub fn as_data(&self) -> Option<&Term> {
        match self {
            Value::Data(t) => Some(t.as_ref()),
            Value::Int(_) => None,
            _ => None,
        }
    }

    pub fn to_plain_term(&self) -> Option<Term> {
        match self {
            Value::Data(t) => Some(t.as_ref().clone()),
            Value::Int(n) => Some(Term::Int(num_bigint::BigInt::from(*n))),
            Value::Vector(xs) => {
                let mut out = Vec::with_capacity(xs.len());
                for x in xs.iter() {
                    out.push(x.to_plain_term()?);
                }
                Some(Term::Vector(out))
            }
            Value::Map(m) => {
                let mut out = std::collections::BTreeMap::new();
                for (k, v) in m.iter() {
                    out.insert(TermOrdKey(k.0.clone()), v.to_plain_term()?);
                }
                Some(Term::Map(out))
            }
            _ => None,
        }
    }

    pub fn debug_repr(&self) -> String {
        match self {
            Value::Data(t) => print_term(t.as_ref()),
            Value::Int(n) => n.to_string(),
            Value::Vector(xs) => {
                let inner: Vec<String> = xs.iter().map(|x| x.debug_repr()).collect();
                format!("[{}]", inner.join(" "))
            }
            Value::Map(m) => {
                let mut inner = String::new();
                inner.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i != 0 {
                        inner.push(' ');
                    }
                    inner.push_str(&print_term(&k.0));
                    inner.push(' ');
                    inner.push_str(&v.debug_repr());
                }
                inner.push('}');
                inner
            }
            Value::Closure(data) => {
                format!("(closure {} {})", data.param, print_term(&data.body))
            }
            Value::CompiledClosure(data) => {
                format!("(closure {} {})", data.param, print_term(&data.body))
            }
            Value::SealToken(SealId(id)) => format!("#<seal-token {}>", id),
            Value::Sealed {
                token: SealId(id),
                payload,
            } => {
                format!("#<sealed {} {}>", id, payload.debug_repr())
            }
            Value::NativeFn(f) => format!("#<native {} {}/{}>", f.name, f.collected.len(), f.arity),
            Value::Contract(c) => format!("#<contract {}>", hex_prefix(&c.contract_id, 8)),
            Value::EffectProgram(_) => "#<effect-program>".to_string(),
            Value::EffectRequest(r) => format!("#<effect-req {}>", r.op),
        }
    }

    pub fn to_term_for_log(&self, protocol_error: Option<SealId>) -> Term {
        match self {
            Value::Data(t) => t.as_ref().clone(),
            Value::Int(n) => Term::Int(num_bigint::BigInt::from(*n)),
            Value::Vector(xs) => Term::Vector(
                xs.iter()
                    .map(|x| x.to_term_for_log(protocol_error))
                    .collect(),
            ),
            Value::Map(m) => Term::Map(
                m.iter()
                    .map(|(k, v)| (TermOrdKey(k.0.clone()), v.to_term_for_log(protocol_error)))
                    .collect(),
            ),
            Value::Sealed { token, payload } => {
                if protocol_error.is_some_and(|e| e == *token) {
                    // For logs we record the *payload* and let replay re-seal it.
                    payload.to_term_for_log(protocol_error)
                } else {
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::Symbol(":opaque".to_string())),
                                Term::Str(self.debug_repr()),
                            ),
                            (
                                TermOrdKey(Term::Symbol(":note".to_string())),
                                Term::Str("sealed value not serializable in v0.2 log".to_string()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    )
                }
            }
            _ => Term::Map(
                [(
                    TermOrdKey(Term::Symbol(":opaque".to_string())),
                    Term::Str(self.debug_repr()),
                )]
                .into_iter()
                .collect(),
            ),
        }
    }
}

fn hex_prefix(bytes: &[u8], n: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    let end = n.min(bytes.len());
    for &b in &bytes[..end] {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}
