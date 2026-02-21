use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use crate::eval::{EvalCtx, prim, type_err};
use crate::value::Value;
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};

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
    let mut manifest = CoverageSiteManifest::default();
    for form in &compiled.forms {
        match form {
            CompiledForm::Def(_, expr) | CompiledForm::Expr(expr) => {
                collect_coverage_manifest_from_expr(expr, &mut manifest);
            }
        }
    }
    manifest
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
    let mut out = Vec::new();
    out.extend_from_slice(COMPILED_MODULE_BLOB_MAGIC);
    push_u32(&mut out, m.forms.len())?;
    for f in &m.forms {
        match f {
            CompiledForm::Def(name, expr) => {
                out.push(0);
                push_str(&mut out, name)?;
                encode_cexpr(&mut out, expr)?;
            }
            CompiledForm::Expr(expr) => {
                out.push(1);
                encode_cexpr(&mut out, expr)?;
            }
        }
    }
    Ok(out)
}

pub fn decode_compiled_module_blob(bytes: &[u8]) -> Result<CompiledModule, KernelError> {
    let mut cur = DecodeCursor { bytes, at: 0 };
    let got_magic = cur.read_exact(COMPILED_MODULE_BLOB_MAGIC.len())?;
    if got_magic != COMPILED_MODULE_BLOB_MAGIC {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "compiled module blob magic mismatch",
        ));
    }
    let forms_len = cur.read_u32()? as usize;
    let mut forms = Vec::with_capacity(forms_len);
    for _ in 0..forms_len {
        let tag = cur.read_u8()?;
        match tag {
            0 => {
                let name = cur.read_str()?;
                let expr = decode_cexpr(&mut cur)?;
                forms.push(CompiledForm::Def(name, expr));
            }
            1 => {
                let expr = decode_cexpr(&mut cur)?;
                forms.push(CompiledForm::Expr(expr));
            }
            _ => {
                return Err(KernelError::new(
                    KernelErrorKind::Internal,
                    format!("invalid compiled form tag: {tag}"),
                ));
            }
        }
    }
    if cur.remaining() != 0 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "compiled module blob has trailing bytes",
        ));
    }
    Ok(CompiledModule { forms })
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

fn push_u32(out: &mut Vec<u8>, n: usize) -> Result<(), KernelError> {
    let n: u32 = u32::try_from(n).map_err(|_| {
        KernelError::new(
            KernelErrorKind::Internal,
            "compiled module blob field exceeds u32 range",
        )
    })?;
    out.extend_from_slice(&n.to_le_bytes());
    Ok(())
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), KernelError> {
    push_u32(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}

fn push_str(out: &mut Vec<u8>, s: &str) -> Result<(), KernelError> {
    push_bytes(out, s.as_bytes())
}

fn push_term(out: &mut Vec<u8>, t: &Term) -> Result<(), KernelError> {
    let rendered = print_term(t);
    push_str(out, &rendered)
}

fn encode_cexpr(out: &mut Vec<u8>, expr: &Arc<CExpr>) -> Result<(), KernelError> {
    match expr.as_ref() {
        CExpr::Atom(t) => {
            out.push(0);
            push_term(out, t)
        }
        CExpr::Var { name, site_id } => {
            out.push(1);
            push_str(out, name)?;
            push_str(out, site_id)
        }
        CExpr::Vector(items) => {
            out.push(2);
            push_u32(out, items.len())?;
            for t in items {
                push_term(out, t)?;
            }
            Ok(())
        }
        CExpr::Map(entries) => {
            out.push(3);
            push_u32(out, entries.len())?;
            for (k, v) in entries {
                push_term(out, &k.0)?;
                encode_cexpr(out, v)?;
            }
            Ok(())
        }
        CExpr::Quote(t) => {
            out.push(4);
            push_term(out, t)
        }
        CExpr::If {
            cond,
            then_expr,
            else_expr,
            site_id,
        } => {
            out.push(5);
            push_str(out, site_id)?;
            encode_cexpr(out, cond)?;
            encode_cexpr(out, then_expr)?;
            encode_cexpr(out, else_expr)
        }
        CExpr::Begin(items) => {
            out.push(6);
            push_u32(out, items.len())?;
            for it in items {
                encode_cexpr(out, it)?;
            }
            Ok(())
        }
        CExpr::Let(bindings, body) => {
            out.push(7);
            push_u32(out, bindings.len())?;
            for (name, rhs) in bindings {
                push_str(out, name)?;
                encode_cexpr(out, rhs)?;
            }
            encode_cexpr(out, body)
        }
        CExpr::FnUnary {
            param,
            body_term,
            body,
        } => {
            out.push(8);
            push_str(out, param)?;
            push_term(out, body_term)?;
            encode_cexpr(out, body)
        }
        CExpr::Prim { op, args } => {
            out.push(9);
            push_str(out, op)?;
            push_u32(out, args.len())?;
            for a in args {
                encode_cexpr(out, a)?;
            }
            Ok(())
        }
        CExpr::SealNew => {
            out.push(10);
            Ok(())
        }
        CExpr::Seal(v, tok) => {
            out.push(11);
            encode_cexpr(out, v)?;
            encode_cexpr(out, tok)
        }
        CExpr::Unseal(w, tok) => {
            out.push(12);
            encode_cexpr(out, w)?;
            encode_cexpr(out, tok)
        }
        CExpr::App(f, x) => {
            out.push(13);
            encode_cexpr(out, f)?;
            encode_cexpr(out, x)
        }
    }
}

struct DecodeCursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> DecodeCursor<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn read_exact(&mut self, n: usize) -> Result<&'a [u8], KernelError> {
        if self.remaining() < n {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "compiled module blob truncated",
            ));
        }
        let start = self.at;
        self.at += n;
        Ok(&self.bytes[start..start + n])
    }

    fn read_u8(&mut self) -> Result<u8, KernelError> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_u32(&mut self) -> Result<u32, KernelError> {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(self.read_exact(4)?);
        Ok(u32::from_le_bytes(buf))
    }

    fn read_bytes(&mut self) -> Result<&'a [u8], KernelError> {
        let n = self.read_u32()? as usize;
        self.read_exact(n)
    }

    fn read_str(&mut self) -> Result<String, KernelError> {
        let b = self.read_bytes()?;
        let s = std::str::from_utf8(b).map_err(|e| {
            KernelError::new(
                KernelErrorKind::Internal,
                format!("compiled module blob invalid utf-8: {e}"),
            )
        })?;
        Ok(s.to_string())
    }

    fn read_term(&mut self) -> Result<Term, KernelError> {
        let s = self.read_str()?;
        parse_term(&s).map_err(|e| {
            KernelError::new(
                KernelErrorKind::Internal,
                format!("compiled module blob term parse failed: {e}"),
            )
        })
    }
}

fn decode_cexpr(cur: &mut DecodeCursor<'_>) -> Result<Arc<CExpr>, KernelError> {
    let tag = cur.read_u8()?;
    let out = match tag {
        0 => CExpr::Atom(cur.read_term()?),
        1 => CExpr::Var {
            name: cur.read_str()?,
            site_id: cur.read_str()?,
        },
        2 => {
            let n = cur.read_u32()? as usize;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(cur.read_term()?);
            }
            CExpr::Vector(items)
        }
        3 => {
            let n = cur.read_u32()? as usize;
            let mut entries = Vec::with_capacity(n);
            for _ in 0..n {
                let key = TermOrdKey(cur.read_term()?);
                let val = decode_cexpr(cur)?;
                entries.push((key, val));
            }
            CExpr::Map(entries)
        }
        4 => CExpr::Quote(cur.read_term()?),
        5 => CExpr::If {
            site_id: cur.read_str()?,
            cond: decode_cexpr(cur)?,
            then_expr: decode_cexpr(cur)?,
            else_expr: decode_cexpr(cur)?,
        },
        6 => {
            let n = cur.read_u32()? as usize;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(decode_cexpr(cur)?);
            }
            CExpr::Begin(items)
        }
        7 => {
            let n = cur.read_u32()? as usize;
            let mut bindings = Vec::with_capacity(n);
            for _ in 0..n {
                let name = cur.read_str()?;
                let rhs = decode_cexpr(cur)?;
                bindings.push((name, rhs));
            }
            let body = decode_cexpr(cur)?;
            CExpr::Let(bindings, body)
        }
        8 => {
            let param = cur.read_str()?;
            let body_term = cur.read_term()?;
            let body = decode_cexpr(cur)?;
            CExpr::FnUnary {
                param,
                body_term,
                body,
            }
        }
        9 => {
            let op = cur.read_str()?;
            let n = cur.read_u32()? as usize;
            let mut args = Vec::with_capacity(n);
            for _ in 0..n {
                args.push(decode_cexpr(cur)?);
            }
            CExpr::Prim { op, args }
        }
        10 => CExpr::SealNew,
        11 => CExpr::Seal(decode_cexpr(cur)?, decode_cexpr(cur)?),
        12 => CExpr::Unseal(decode_cexpr(cur)?, decode_cexpr(cur)?),
        13 => CExpr::App(decode_cexpr(cur)?, decode_cexpr(cur)?),
        _ => {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("invalid compiled expr tag: {tag}"),
            ));
        }
    };
    Ok(Arc::new(out))
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

fn collect_coverage_manifest_from_expr(expr: &Arc<CExpr>, manifest: &mut CoverageSiteManifest) {
    match expr.as_ref() {
        CExpr::Atom(_) | CExpr::Vector(_) | CExpr::Quote(_) | CExpr::SealNew => {}
        CExpr::Var { site_id, .. } => {
            manifest.statement_sites.insert(site_id.clone());
        }
        CExpr::Map(entries) => {
            for (_, v) in entries {
                collect_coverage_manifest_from_expr(v, manifest);
            }
        }
        CExpr::If {
            cond,
            then_expr,
            else_expr,
            site_id,
        } => {
            manifest.decision_sites.insert(site_id.clone());
            let mut vars = BTreeSet::new();
            collect_condition_symbols_from_expr(cond, &mut vars);
            manifest
                .decision_conditions
                .entry(site_id.clone())
                .or_default()
                .extend(vars);
            collect_coverage_manifest_from_expr(cond, manifest);
            collect_coverage_manifest_from_expr(then_expr, manifest);
            collect_coverage_manifest_from_expr(else_expr, manifest);
        }
        CExpr::Begin(xs) => {
            for x in xs {
                collect_coverage_manifest_from_expr(x, manifest);
            }
        }
        CExpr::Let(bindings, body) => {
            for (_, rhs) in bindings {
                collect_coverage_manifest_from_expr(rhs, manifest);
            }
            collect_coverage_manifest_from_expr(body, manifest);
        }
        CExpr::FnUnary { body, .. } => {
            collect_coverage_manifest_from_expr(body, manifest);
        }
        CExpr::Prim { args, .. } => {
            for a in args {
                collect_coverage_manifest_from_expr(a, manifest);
            }
        }
        CExpr::Seal(v, tok) | CExpr::Unseal(v, tok) | CExpr::App(v, tok) => {
            collect_coverage_manifest_from_expr(v, manifest);
            collect_coverage_manifest_from_expr(tok, manifest);
        }
    }
}

fn collect_condition_symbols_from_expr(expr: &Arc<CExpr>, out: &mut BTreeSet<String>) {
    match expr.as_ref() {
        CExpr::Var { name, .. } => {
            out.insert(name.clone());
        }
        CExpr::Atom(_) | CExpr::Vector(_) | CExpr::Quote(_) | CExpr::SealNew => {}
        CExpr::Map(entries) => {
            for (_, v) in entries {
                collect_condition_symbols_from_expr(v, out);
            }
        }
        CExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_condition_symbols_from_expr(cond, out);
            collect_condition_symbols_from_expr(then_expr, out);
            collect_condition_symbols_from_expr(else_expr, out);
        }
        CExpr::Begin(xs) => {
            for x in xs {
                collect_condition_symbols_from_expr(x, out);
            }
        }
        CExpr::Let(bindings, body) => {
            for (_, rhs) in bindings {
                collect_condition_symbols_from_expr(rhs, out);
            }
            collect_condition_symbols_from_expr(body, out);
        }
        CExpr::FnUnary { body, .. } => {
            collect_condition_symbols_from_expr(body, out);
        }
        CExpr::Prim { args, .. } => {
            for a in args {
                collect_condition_symbols_from_expr(a, out);
            }
        }
        CExpr::Seal(v, tok) | CExpr::Unseal(v, tok) | CExpr::App(v, tok) => {
            collect_condition_symbols_from_expr(v, out);
            collect_condition_symbols_from_expr(tok, out);
        }
    }
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
