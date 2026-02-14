use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use blake3::Hasher;

use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use gc_coreform::{Term, TermOrdKey, hash_term, print_term};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SealId(pub u64);

#[derive(Clone, Debug)]
pub enum Value {
    Data(Term),
    Vector(Vec<Value>),
    Map(BTreeMap<TermOrdKey, Value>),
    Closure { param: String, body: Term, env: Env },
    SealToken(SealId),
    Sealed { token: SealId, payload: Box<Value> },
    NativeFn(NativeFn),
    Contract(Rc<Contract>),
    EffectProgram(Box<EffectProgram>),
    EffectRequest(EffectRequest),
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
        if collected.len() < self.arity {
            Ok(Value::NativeFn(NativeFn {
                name: self.name,
                arity: self.arity,
                collected,
                func: self.func,
            }))
        } else if collected.len() == self.arity {
            // Native functions are required to be total and panic-free; this is a
            // hardening boundary so a bug can't crash the whole evaluator.
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (self.func)(ctx, collected)
            }));
            match r {
                Ok(v) => v,
                Err(_) => Err(KernelError::new(
                    KernelErrorKind::Internal,
                    format!("native fn {} panicked", self.name),
                )),
            }
        } else {
            Err(KernelError::new(
                KernelErrorKind::BadForm,
                format!("native fn {} applied to too many args", self.name),
            ))
        }
    }
}

#[derive(Clone, Debug)]
pub struct Contract {
    pub handler: Value,
    pub proto: Option<Rc<Contract>>,
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
        match self {
            Value::Closure { param, body, env } => {
                let env2 = Env::with_binding(&env, param, arg);
                crate::eval::eval_term(ctx, &env2, &body)
            }
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
    env_cache: std::collections::HashMap<*const crate::env::EnvFrame, [u8; 32]>,
}

impl ValueHasher {
    fn new() -> Self {
        Self {
            env_cache: std::collections::HashMap::new(),
        }
    }

    fn hash(&mut self, v: &Value) -> [u8; 32] {
        let mut h = Hasher::new();
        self.hash_into(&mut h, v);
        *h.finalize().as_bytes()
    }

    fn hash_into(&mut self, h: &mut Hasher, v: &Value) {
        h.update(b"GCv0.2\0value\0");
        match v {
            Value::Data(t) => {
                h.update(b"data\0");
                h.update(&hash_term(t));
            }
            Value::Vector(xs) => {
                h.update(b"vec\0");
                h.update(&(xs.len() as u64).to_le_bytes());
                for x in xs {
                    let hx = self.hash(x);
                    h.update(&hx);
                }
            }
            Value::Map(m) => {
                h.update(b"map\0");
                h.update(&(m.len() as u64).to_le_bytes());
                for (k, v) in m {
                    h.update(&hash_term(&k.0));
                    let hv = self.hash(v);
                    h.update(&hv);
                }
            }
            Value::Closure { param, body, env } => {
                h.update(b"closure\0");
                h.update(param.as_bytes());
                h.update(b"\0");
                h.update(&hash_term(body));
                let he = self.hash_env(env);
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
        if let Some(h) = self.env_cache.get(&ptr) {
            return *h;
        }

        let mut h = Hasher::new();
        h.update(b"GCv0.2\0env\0");

        // Parent hash first to preserve a stable chain structure.
        if let Some(parent) = &env.0.parent {
            let hp = self.hash_env(parent);
            h.update(b"parent\0");
            h.update(&hp);
        } else {
            h.update(b"parent\0nil\0");
        }

        h.update(b"binds\0");
        h.update(&(env.0.binds.len() as u64).to_le_bytes());
        for (k, v) in &env.0.binds {
            h.update(k.as_bytes());
            h.update(b"\0");
            let hv = self.hash(v);
            h.update(&hv);
        }

        let out = *h.finalize().as_bytes();
        self.env_cache.insert(ptr, out);
        out
    }
}

impl Value {
    pub fn as_symbol(&self) -> Option<&str> {
        match self {
            Value::Data(Term::Symbol(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn truthy(&self) -> bool {
        !matches!(
            self,
            Value::Data(Term::Nil) | Value::Data(Term::Bool(false))
        )
    }

    pub fn as_data(&self) -> Option<&Term> {
        match self {
            Value::Data(t) => Some(t),
            _ => None,
        }
    }

    pub fn debug_repr(&self) -> String {
        match self {
            Value::Data(t) => print_term(t),
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
            Value::Closure { param, body, .. } => {
                format!("(closure {} {})", param, print_term(body))
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
            Value::Data(t) => t.clone(),
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
