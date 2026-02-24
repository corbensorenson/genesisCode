use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use crate::eval::{EvalCtx, prim, type_err};
use crate::value::Value;
use gc_coreform::{Term, TermOrdKey};

#[path = "compiled_blob.rs"]
mod compiled_blob;
#[path = "compiled_compile.rs"]
mod compiled_compile;
#[path = "compiled_coverage.rs"]
mod compiled_coverage;

const COMPILED_MODULE_BLOB_MAGIC: &[u8] = b"GCKM1\0";

#[derive(Clone, Debug)]
pub(crate) enum CExpr {
    Atom(Term),
    Var {
        name: String,
        site_id: String,
    },
    Vector(Vec<Term>),
    Map(Vec<(TermOrdKey, Arc<CExpr>)>),
    Quote(Term),
    If {
        cond: Arc<CExpr>,
        then_expr: Arc<CExpr>,
        else_expr: Arc<CExpr>,
        site_id: String,
    },
    Begin(Vec<Arc<CExpr>>),
    Let(Vec<(String, Arc<CExpr>)>, Arc<CExpr>),
    FnUnary {
        param: String,
        body_term: Term,
        body: Arc<CExpr>,
    },
    Prim {
        op: String,
        args: Vec<Arc<CExpr>>,
    },
    SealNew,
    Seal(Arc<CExpr>, Arc<CExpr>),
    Unseal(Arc<CExpr>, Arc<CExpr>),
    App(Arc<CExpr>, Arc<CExpr>),
}

#[derive(Clone, Debug)]
pub struct CompiledModule {
    forms: Vec<CompiledForm>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoverageSiteManifest {
    pub statement_sites: BTreeSet<String>,
    pub decision_sites: BTreeSet<String>,
    pub decision_conditions: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Clone, Debug)]
enum CompiledForm {
    Def(String, Arc<CExpr>),
    Expr(Arc<CExpr>),
}

pub fn compile_module(forms: &[Term]) -> Result<CompiledModule, KernelError> {
    compile_module_with_site_namespace(forms, "")
}

pub fn compile_module_with_site_namespace(
    forms: &[Term],
    site_namespace: &str,
) -> Result<CompiledModule, KernelError> {
    compiled_compile::compile_module_with_site_namespace_impl(forms, site_namespace)
}

pub fn compiled_module_coverage_manifest(
    forms: &[Term],
    site_namespace: &str,
) -> Result<CoverageSiteManifest, KernelError> {
    let compiled = compile_module_with_site_namespace(forms, site_namespace)?;
    Ok(compiled_module_coverage_manifest_from_compiled(&compiled))
}

pub fn compiled_module_coverage_manifest_from_compiled(
    compiled: &CompiledModule,
) -> CoverageSiteManifest {
    compiled_coverage::compiled_module_coverage_manifest_from_compiled(compiled)
}

pub fn eval_compiled_module(
    ctx: &mut EvalCtx,
    env: &mut Env,
    m: &CompiledModule,
) -> Result<Value, KernelError> {
    let mut last = Value::Data(Term::Nil);
    for f in &m.forms {
        match f {
            CompiledForm::Def(name, e) => {
                let v = eval_cexpr(ctx, env, e)?;
                env.set_local(name.clone(), v);
                last = Value::Data(Term::Nil);
            }
            CompiledForm::Expr(e) => {
                last = eval_cexpr(ctx, env, e)?;
            }
        }
    }
    Ok(last)
}

pub fn eval_module_compiled(
    ctx: &mut EvalCtx,
    env: &mut Env,
    forms: &[Term],
) -> Result<Value, KernelError> {
    let m = compile_module(forms)?;
    eval_compiled_module(ctx, env, &m)
}

pub fn encode_compiled_module_blob(m: &CompiledModule) -> Result<Vec<u8>, KernelError> {
    compiled_blob::encode_compiled_module_blob(m)
}

pub fn decode_compiled_module_blob(bytes: &[u8]) -> Result<CompiledModule, KernelError> {
    compiled_blob::decode_compiled_module_blob(bytes)
}

pub(crate) fn eval_cexpr(
    ctx: &mut EvalCtx,
    env: &Env,
    expr: &Arc<CExpr>,
) -> Result<Value, KernelError> {
    // Like eval_term, implement tail-call optimization for:
    // - (if ...) branches
    // - (begin ...) last form
    // - application where the callee is a closure
    let mut cur_env = env.clone();
    let mut cur = expr.clone();
    loop {
        ctx.tick()?;
        match cur.as_ref() {
            CExpr::Atom(t) => {
                // Mirror eval_term's memory observations for strings/bytes.
                match t {
                    Term::Str(s) => ctx.mem_observe_string_len(s.len())?,
                    Term::Bytes(b) => ctx.mem_observe_bytes_len(b.len())?,
                    _ => {}
                }
                return Ok(Value::Data(t.clone()));
            }
            CExpr::Var { name, site_id } => {
                let value = cur_env.get(name).ok_or_else(|| {
                    KernelError::new(KernelErrorKind::Unbound, format!("unbound symbol: {name}"))
                })?;
                ctx.coverage_statement_site(site_id);
                ctx.coverage_hit(name, &value);
                return Ok(value);
            }
            CExpr::Vector(xs) => {
                ctx.mem_observe_vec_len(xs.len())?;
                for x in xs {
                    ctx.mem_observe_data_term(x)?;
                }
                return Ok(Value::Vector(xs.iter().cloned().map(Value::Data).collect()));
            }
            CExpr::Map(entries) => {
                ctx.mem_observe_map_len(entries.len())?;
                for (k, _v) in entries {
                    ctx.mem_observe_data_term(&k.0)?;
                }
                let mut out = std::collections::BTreeMap::new();
                for (k, v) in entries {
                    let vv = eval_cexpr(ctx, &cur_env, v)?;
                    out.insert(k.clone(), vv);
                }
                return Ok(Value::Map(out));
            }
            CExpr::Quote(d) => {
                ctx.mem_observe_data_term(d)?;
                return Ok(Value::Data(d.clone()));
            }
            CExpr::If {
                cond,
                then_expr,
                else_expr,
                site_id,
            } => {
                ctx.coverage_begin_decision_site(site_id);
                let cv = match eval_cexpr(ctx, &cur_env, cond) {
                    Ok(v) => v,
                    Err(e) => {
                        ctx.coverage_abort_decision_site();
                        return Err(e);
                    }
                };
                let cond_truthy = cv.truthy();
                ctx.coverage_finish_decision_site(cond_truthy);
                cur = if cond_truthy {
                    then_expr.clone()
                } else {
                    else_expr.clone()
                };
                continue;
            }
            CExpr::Begin(xs) => {
                if xs.is_empty() {
                    return Ok(Value::Data(Term::Nil));
                }
                if xs.len() == 1 {
                    cur = xs[0].clone();
                    continue;
                }
                for x in xs.iter().take(xs.len() - 1) {
                    let _ = eval_cexpr(ctx, &cur_env, x)?;
                }
                cur = xs[xs.len() - 1].clone();
                continue;
            }
            CExpr::Let(bs, body) => {
                let mut env2 = cur_env.clone();
                for (name, rhs) in bs {
                    let v = eval_cexpr(ctx, &env2, rhs)?;
                    env2 = Env::with_binding(&env2, name.clone(), v);
                }
                cur_env = env2;
                cur = body.clone();
                continue;
            }
            CExpr::FnUnary {
                param,
                body_term,
                body,
            } => {
                return Ok(Value::CompiledClosure {
                    param: param.clone(),
                    body: body_term.clone(),
                    body_c: crate::value::CompiledExpr::new(body.clone()),
                    env: cur_env.clone(),
                });
            }
            CExpr::Prim { op, args } => {
                let mut vs = Vec::with_capacity(args.len());
                for a in args {
                    vs.push(eval_cexpr(ctx, &cur_env, a)?);
                }
                return prim(ctx, op, vs);
            }
            CExpr::SealNew => {
                let id = ctx.state.next_seal_id;
                ctx.state.next_seal_id = ctx.state.next_seal_id.saturating_add(1);
                return Ok(Value::SealToken(crate::value::SealId(id)));
            }
            CExpr::Seal(v, tok) => {
                let vv = eval_cexpr(ctx, &cur_env, v)?;
                let tv = eval_cexpr(ctx, &cur_env, tok)?;
                let Value::SealToken(id) = tv else {
                    return type_err(ctx, "seal expects a seal token as second argument");
                };
                return Ok(Value::Sealed {
                    token: id,
                    payload: Box::new(vv),
                });
            }
            CExpr::Unseal(w, tok) => {
                let wv = eval_cexpr(ctx, &cur_env, w)?;
                let tv = eval_cexpr(ctx, &cur_env, tok)?;
                let Value::SealToken(id) = tv else {
                    return type_err(ctx, "unseal expects a seal token as second argument");
                };
                if let Value::Sealed { token, payload } = wv
                    && token == id
                {
                    return Ok(*payload);
                }
                return Ok(Value::Data(Term::Nil));
            }
            CExpr::App(f, x) => {
                let fv = eval_cexpr(ctx, &cur_env, f)?;
                let xv = eval_cexpr(ctx, &cur_env, x)?;

                match fv {
                    Value::Closure { param, body, env } => {
                        // Legacy closures can be present in mixed-mode envs (e.g., values created
                        // by tree-walk eval before compiled execution). Compile on-demand so
                        // compiled execution never deopts to the term evaluator.
                        let compiled_body = compiled_compile::compile_term(&body).map_err(|e| {
                            KernelError::new(
                                e.kind.clone(),
                                format!(
                                    "failed to compile legacy closure body in compiled mode: {e}"
                                ),
                            )
                        })?;
                        cur_env = Env::with_binding(&env, param, xv);
                        cur = compiled_body;
                        continue;
                    }
                    Value::CompiledClosure {
                        param,
                        body: _,
                        body_c,
                        env,
                    } => {
                        cur_env = Env::with_binding(&env, param, xv);
                        cur = body_c.inner().clone();
                        continue;
                    }
                    Value::NativeFn(nf) => return nf.apply(ctx, xv),
                    other => {
                        return Err(KernelError::new(
                            KernelErrorKind::NotCallable,
                            format!("value is not callable: {}", other.debug_repr()),
                        ));
                    }
                }
            }
        }
    }
}
