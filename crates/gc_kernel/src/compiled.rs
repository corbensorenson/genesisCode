use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use crate::eval::{EvalCtx, prim, type_err};
use crate::value::Value;
use gc_coreform::{Term, TermOrdKey};

#[path = "compiled_blob.rs"]
mod compiled_blob;
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
    let mut out = Vec::with_capacity(forms.len());
    for (form_idx, form) in forms.iter().enumerate() {
        let form_idx = u32::try_from(form_idx).map_err(|_| {
            KernelError::new(
                KernelErrorKind::Internal,
                "compiled module form index exceeds u32 range",
            )
        })?;
        if let Some((name, expr)) = parse_def(form) {
            let mut path = vec![form_idx, 2];
            out.push(CompiledForm::Def(
                name,
                compile_term_with_site_path(&expr, &mut path, site_namespace)?,
            ));
        } else {
            let mut path = vec![form_idx];
            out.push(CompiledForm::Expr(compile_term_with_site_path(
                form,
                &mut path,
                site_namespace,
            )?));
        }
    }
    Ok(CompiledModule { forms: out })
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

fn parse_def(t: &Term) -> Option<(String, Term)> {
    let items = t.as_proper_list()?;
    if items.len() != 3 {
        return None;
    }
    if !matches!(items[0], Term::Symbol(s) if s == "def") {
        return None;
    }
    let Term::Symbol(name) = items[1] else {
        return None;
    };
    Some((name.clone(), items[2].clone()))
}

fn site_id(kind: &str, site_namespace: &str, path: &[u32]) -> String {
    let path_str = path
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".");
    if site_namespace.is_empty() {
        format!("{kind}:{path_str}")
    } else {
        format!("{site_namespace}::{kind}:{path_str}")
    }
}

fn child_index(i: usize) -> Result<u32, KernelError> {
    u32::try_from(i).map_err(|_| {
        KernelError::new(
            KernelErrorKind::Internal,
            "compiled expression child index exceeds u32 range",
        )
    })
}

fn with_child_path<T>(
    path: &mut Vec<u32>,
    child: u32,
    f: impl FnOnce(&mut Vec<u32>) -> Result<T, KernelError>,
) -> Result<T, KernelError> {
    path.push(child);
    let out = f(path);
    path.pop();
    out
}

fn compile_term(t: &Term) -> Result<Arc<CExpr>, KernelError> {
    let mut path = vec![0];
    compile_term_with_site_path(t, &mut path, "")
}

fn compile_term_with_site_path(
    t: &Term,
    path: &mut Vec<u32>,
    site_namespace: &str,
) -> Result<Arc<CExpr>, KernelError> {
    Ok(Arc::new(compile_term_inner(t, path, site_namespace)?))
}

fn compile_term_inner(
    t: &Term,
    path: &mut Vec<u32>,
    site_namespace: &str,
) -> Result<CExpr, KernelError> {
    match t {
        Term::Nil | Term::Bool(_) | Term::Int(_) | Term::Str(_) | Term::Bytes(_) => {
            Ok(CExpr::Atom(t.clone()))
        }
        Term::Symbol(s) => Ok(CExpr::Var {
            name: s.clone(),
            site_id: site_id("stmt", site_namespace, path),
        }),
        Term::Vector(xs) => Ok(CExpr::Vector(xs.clone())),
        Term::Map(m) => {
            let mut out = Vec::with_capacity(m.len());
            for (idx, (k, v)) in m.iter().enumerate() {
                let child = child_index(idx)?;
                out.push((
                    k.clone(),
                    with_child_path(path, child, |p| {
                        compile_term_with_site_path(v, p, site_namespace)
                    })?,
                ));
            }
            Ok(CExpr::Map(out))
        }
        Term::Pair(_, _) => {
            let Some(items) = t.as_proper_list() else {
                return Err(KernelError::new(
                    KernelErrorKind::BadForm,
                    "improper list is not a valid form",
                ));
            };
            compile_list(items, path, site_namespace)
        }
    }
}

fn compile_list(
    items: Vec<&Term>,
    path: &mut Vec<u32>,
    site_namespace: &str,
) -> Result<CExpr, KernelError> {
    if items.is_empty() {
        return Ok(CExpr::Atom(Term::Nil));
    }

    if let Term::Symbol(h) = items[0] {
        match h.as_str() {
            "quote" => {
                if items.len() != 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(quote datum) expects exactly 1 argument",
                    ));
                }
                return Ok(CExpr::Quote(items[1].clone()));
            }
            "fn" => {
                let (param, body_term) = desugar_fn_to_unary(&items)?;
                let body = with_child_path(path, 0, |p| {
                    compile_term_with_site_path(&body_term, p, site_namespace)
                })?;
                return Ok(CExpr::FnUnary {
                    param,
                    body_term,
                    body,
                });
            }
            "if" => {
                if items.len() != 4 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(if c t e) expects exactly 3 arguments",
                    ));
                }
                return Ok(CExpr::If {
                    site_id: site_id("decision", site_namespace, path),
                    cond: with_child_path(path, 0, |p| {
                        compile_term_with_site_path(items[1], p, site_namespace)
                    })?,
                    then_expr: with_child_path(path, 1, |p| {
                        compile_term_with_site_path(items[2], p, site_namespace)
                    })?,
                    else_expr: with_child_path(path, 2, |p| {
                        compile_term_with_site_path(items[3], p, site_namespace)
                    })?,
                });
            }
            "begin" => {
                if items.len() < 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(begin ...) expects at least 1 argument",
                    ));
                }
                if items.len() == 2 {
                    return with_child_path(path, 0, |p| {
                        compile_term_inner(items[1], p, site_namespace)
                    });
                }
                let mut xs = Vec::with_capacity(items.len() - 1);
                for (idx, it) in items.iter().skip(1).enumerate() {
                    let child = child_index(idx)?;
                    xs.push(with_child_path(path, child, |p| {
                        compile_term_with_site_path(it, p, site_namespace)
                    })?);
                }
                return Ok(CExpr::Begin(xs));
            }
            "let" => {
                if items.len() < 3 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(let ((x e) ...) body...) expects bindings and body",
                    ));
                }
                let bindings = items[1];
                let Some(bs) = bindings.as_proper_list() else {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(let ...) bindings must be a list",
                    ));
                };
                let mut out_bs = Vec::with_capacity(bs.len());
                for (idx, b) in bs.into_iter().enumerate() {
                    let Some(pair) = b.as_proper_list() else {
                        return Err(KernelError::new(
                            KernelErrorKind::BadForm,
                            "(let ...) binding must be a list (name expr)",
                        ));
                    };
                    if pair.len() != 2 {
                        return Err(KernelError::new(
                            KernelErrorKind::BadForm,
                            "(let ...) binding must have exactly 2 forms",
                        ));
                    }
                    let Term::Symbol(name) = pair[0] else {
                        return Err(KernelError::new(
                            KernelErrorKind::BadForm,
                            "(let ...) binding name must be symbol",
                        ));
                    };
                    let child = child_index(idx)?;
                    out_bs.push((
                        name.clone(),
                        with_child_path(path, child, |p| {
                            compile_term_with_site_path(pair[1], p, site_namespace)
                        })?,
                    ));
                }

                // multi-body => (begin ...)
                let body_term = if items.len() == 3 {
                    items[2].clone()
                } else {
                    let mut xs = Vec::with_capacity(items.len() - 1);
                    xs.push(Term::Symbol("begin".to_string()));
                    for b in items.iter().skip(2) {
                        xs.push((*b).clone());
                    }
                    Term::list(xs)
                };
                let body_child = child_index(out_bs.len())?;
                return Ok(CExpr::Let(
                    out_bs,
                    with_child_path(path, body_child, |p| {
                        compile_term_with_site_path(&body_term, p, site_namespace)
                    })?,
                ));
            }
            "prim" => {
                if items.len() < 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(prim op ...) expects at least an op",
                    ));
                }
                let Term::Symbol(op) = items[1] else {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(prim ...) op must be a symbol",
                    ));
                };
                let mut args = Vec::with_capacity(items.len().saturating_sub(2));
                for (idx, a) in items.iter().skip(2).enumerate() {
                    let child = child_index(idx)?;
                    args.push(with_child_path(path, child, |p| {
                        compile_term_with_site_path(a, p, site_namespace)
                    })?);
                }
                return Ok(CExpr::Prim {
                    op: op.clone(),
                    args,
                });
            }
            "seal" => {
                return match items.len() {
                    1 => Ok(CExpr::SealNew),
                    3 => Ok(CExpr::Seal(
                        with_child_path(path, 0, |p| {
                            compile_term_with_site_path(items[1], p, site_namespace)
                        })?,
                        with_child_path(path, 1, |p| {
                            compile_term_with_site_path(items[2], p, site_namespace)
                        })?,
                    )),
                    _ => Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(seal) or (seal v tok)",
                    )),
                };
            }
            "unseal" => {
                if items.len() != 3 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(unseal w tok) expects exactly 2 arguments",
                    ));
                }
                return Ok(CExpr::Unseal(
                    with_child_path(path, 0, |p| {
                        compile_term_with_site_path(items[1], p, site_namespace)
                    })?,
                    with_child_path(path, 1, |p| {
                        compile_term_with_site_path(items[2], p, site_namespace)
                    })?,
                ));
            }
            "def" => {
                return Err(KernelError::new(
                    KernelErrorKind::BadForm,
                    "(def ...) is only allowed at module top-level",
                ));
            }
            _ => {}
        }
    }

    // General application.
    let f = with_child_path(path, 0, |p| {
        compile_term_with_site_path(items[0], p, site_namespace)
    })?;
    if items.len() == 1 {
        return with_child_path(path, 0, |p| compile_term_inner(items[0], p, site_namespace));
    }
    let mut acc = f;
    for (idx, a) in items.iter().skip(1).enumerate() {
        let child = child_index(idx + 1)?;
        let arg = with_child_path(path, child, |p| {
            compile_term_with_site_path(a, p, site_namespace)
        })?;
        acc = Arc::new(CExpr::App(acc, arg));
    }
    Ok((*acc).clone())
}

fn desugar_fn_to_unary(items: &[&Term]) -> Result<(String, Term), KernelError> {
    if items.len() < 3 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn (x) body...) expects params and body",
        ));
    }
    let params = items[1];
    let Some(ps) = params.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn ...) params must be a list",
        ));
    };
    if ps.is_empty() {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn ...) requires at least 1 parameter",
        ));
    }
    for p in &ps {
        if !matches!(p, Term::Symbol(_)) {
            return Err(KernelError::new(
                KernelErrorKind::BadForm,
                "(fn ...) params must be symbols",
            ));
        }
    }

    let body_term = if items.len() == 3 {
        items[2].clone()
    } else {
        // multi-body => (begin ...)
        let mut xs = Vec::with_capacity(items.len() - 1);
        xs.push(Term::Symbol("begin".to_string()));
        for b in items.iter().skip(2) {
            xs.push((*b).clone());
        }
        Term::list(xs)
    };

    // Desugar multi-arg lambda into nested unary functions.
    let mut out = body_term;
    for p in ps.into_iter().rev() {
        let Term::Symbol(name) = p else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "internal fn desugaring expected symbol parameter",
            ));
        };
        out = Term::list(vec![
            Term::Symbol("fn".to_string()),
            Term::list(vec![Term::Symbol(name.clone())]),
            out,
        ]);
    }

    let Some(items2) = out.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring failed",
        ));
    };
    if items2.len() != 3 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced unexpected shape",
        ));
    }
    let params2 = &items2[1];
    let Some(ps2) = params2.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced bad params",
        ));
    };
    if ps2.len() != 1 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced non-unary params",
        ));
    }
    let Term::Symbol(param) = ps2[0] else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced non-symbol param",
        ));
    };
    Ok((param.clone(), items2[2].clone()))
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
                        let compiled_body = compile_term(&body).map_err(|e| {
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
