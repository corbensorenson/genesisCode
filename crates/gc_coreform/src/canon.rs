use anyhow::{Context, anyhow, bail};

use crate::term::Term;

pub fn canonicalize_module(forms: Vec<Term>) -> anyhow::Result<Vec<Term>> {
    forms
        .into_iter()
        .map(|t| canonicalize_form(t).context("canonicalize form"))
        .collect()
}

pub fn canonicalize_form(form: Term) -> anyhow::Result<Term> {
    canon_code(form)
}

fn canon_code(t: Term) -> anyhow::Result<Term> {
    match t {
        Term::Pair(_, _) => {
            let Some(items) = t.as_proper_list() else {
                // Source syntax cannot construct improper lists; if we see one, it's a bug upstream.
                return Err(anyhow!("improper list in source form"));
            };

            let items: Vec<Term> = items.into_iter().cloned().collect();
            canon_list_code(items)
        }
        // Vectors and maps are data literals; do not desugar application sugar inside them.
        Term::Vector(xs) => Ok(Term::Vector(
            xs.into_iter()
                .map(canon_data)
                .collect::<anyhow::Result<_>>()?,
        )),
        Term::Map(m) => {
            // Map literal keys are data; values are code expressions.
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in m {
                out.insert(crate::term::TermOrdKey(canon_data(k.0)?), canon_code(v)?);
            }
            Ok(Term::Map(out))
        }
        atom => Ok(atom),
    }
}

fn canon_data(t: Term) -> anyhow::Result<Term> {
    // Under quote: do not desugar application sugar.
    // Still recurse to preserve normalized quoting of nested quotes, etc.
    match t {
        Term::Pair(_, _) => {
            let Some(items) = t.as_proper_list() else {
                return Ok(t);
            };
            let items: Vec<Term> = items.into_iter().cloned().collect();
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(canon_data(it)?);
            }
            Ok(Term::list(out))
        }
        Term::Vector(xs) => Ok(Term::Vector(
            xs.into_iter()
                .map(canon_data)
                .collect::<anyhow::Result<_>>()?,
        )),
        Term::Map(m) => Ok(Term::Map(m)),
        atom => Ok(atom),
    }
}

fn is_sym(t: &Term, name: &str) -> bool {
    matches!(t, Term::Symbol(s) if s == name)
}

fn canon_list_code(mut items: Vec<Term>) -> anyhow::Result<Term> {
    if items.is_empty() {
        return Ok(Term::Nil);
    }

    // Special forms.
    let head = items[0].clone();
    if is_sym(&head, "quote") {
        if items.len() != 2 {
            bail!("(quote ...) expects exactly 1 argument");
        }
        let datum = canon_data(items.remove(1))?;
        return Ok(Term::list(vec![Term::symbol("quote"), datum]));
    }

    if is_sym(&head, "def") {
        if items.len() != 3 {
            bail!("(def name expr) expects exactly 2 arguments");
        }
        let name = items.remove(1);
        if !matches!(name, Term::Symbol(_)) {
            bail!("(def ...) name must be a symbol");
        }
        let expr = canon_code(items.remove(1))?;
        return Ok(Term::list(vec![Term::symbol("def"), name, expr]));
    }

    if is_sym(&head, "fn") {
        // (fn (x) body) where (x) can have multiple params in input; canonicalize to unary.
        if items.len() < 3 {
            bail!("(fn (x) body) expects at least 2 arguments");
        }
        let params = items.remove(1);
        let body_terms: Vec<Term> = items.drain(1..).collect();
        let body = if body_terms.len() == 1 {
            canon_code(body_terms.into_iter().next().unwrap())?
        } else {
            // Canonicalize multi-body as (begin ...).
            let mut canon = Vec::with_capacity(body_terms.len() + 1);
            canon.push(Term::symbol("begin"));
            for t in body_terms {
                canon.push(canon_code(t)?);
            }
            Term::list(canon)
        };

        let Some(params_list) = params.as_proper_list() else {
            bail!("(fn ...) parameter list must be a list");
        };
        let params_vec: Vec<Term> = params_list.into_iter().cloned().collect();
        if params_vec.is_empty() {
            bail!("(fn ...) requires at least 1 parameter");
        }
        for p in &params_vec {
            if !matches!(p, Term::Symbol(_)) {
                bail!("(fn ...) parameters must be symbols");
            }
        }

        // Desugar multi-arg fn into nested unary fns.
        let mut out = body;
        for p in params_vec.into_iter().rev() {
            out = Term::list(vec![Term::symbol("fn"), Term::list(vec![p]), out]);
        }
        return Ok(out);
    }

    if is_sym(&head, "if") {
        if items.len() != 4 {
            bail!("(if cond then else) expects exactly 3 arguments");
        }
        let c = canon_code(items.remove(1))?;
        let t = canon_code(items.remove(1))?;
        let e = canon_code(items.remove(1))?;
        return Ok(Term::list(vec![Term::symbol("if"), c, t, e]));
    }

    if is_sym(&head, "begin") {
        if items.len() < 2 {
            bail!("(begin ...) expects at least 1 argument");
        }
        let mut out = Vec::with_capacity(items.len());
        out.push(Term::symbol("begin"));
        for t in items.into_iter().skip(1) {
            out.push(canon_code(t)?);
        }
        return Ok(Term::list(out));
    }

    if is_sym(&head, "let") {
        if items.len() < 3 {
            bail!("(let ((x e) ...) body...) expects bindings and body");
        }
        let bindings = items.remove(1);
        let Some(bind_list) = bindings.as_proper_list() else {
            bail!("(let ...) bindings must be a list");
        };
        let mut canon_bindings = Vec::new();
        for b in bind_list {
            let Some(pair) = b.as_proper_list() else {
                bail!("(let ...) binding must be a list (name expr)");
            };
            let pair: Vec<Term> = pair.into_iter().cloned().collect();
            if pair.len() != 2 {
                bail!("(let ...) binding must have exactly 2 forms");
            }
            if !matches!(pair[0], Term::Symbol(_)) {
                bail!("(let ...) binding name must be symbol");
            }
            canon_bindings.push(Term::list(vec![
                pair[0].clone(),
                canon_code(pair[1].clone())?,
            ]));
        }

        let body_terms: Vec<Term> = items.into_iter().skip(1).collect();
        let body = if body_terms.len() == 1 {
            canon_code(body_terms.into_iter().next().unwrap())?
        } else {
            let mut b = Vec::with_capacity(body_terms.len() + 1);
            b.push(Term::symbol("begin"));
            for t in body_terms {
                b.push(canon_code(t)?);
            }
            Term::list(b)
        };

        return Ok(Term::list(vec![
            Term::symbol("let"),
            Term::list(canon_bindings),
            body,
        ]));
    }

    if is_sym(&head, "prim") {
        if items.len() < 2 {
            bail!("(prim op ...) expects op");
        }
        let op = items.remove(1);
        if !matches!(op, Term::Symbol(_)) {
            bail!("(prim ...) op must be a symbol");
        }
        let mut out = Vec::with_capacity(items.len());
        out.push(Term::symbol("prim"));
        out.push(op);
        for a in items.into_iter().skip(1) {
            out.push(canon_code(a)?);
        }
        return Ok(Term::list(out));
    }

    if is_sym(&head, "seal") {
        if items.len() == 1 {
            return Ok(Term::list(vec![Term::symbol("seal")]));
        }
        if items.len() != 3 {
            bail!("(seal) or (seal v tok)");
        }
        let v = canon_code(items.remove(1))?;
        let tok = canon_code(items.remove(1))?;
        return Ok(Term::list(vec![Term::symbol("seal"), v, tok]));
    }

    if is_sym(&head, "unseal") {
        if items.len() != 3 {
            bail!("(unseal w tok) expects exactly 2 arguments");
        }
        let w = canon_code(items.remove(1))?;
        let tok = canon_code(items.remove(1))?;
        return Ok(Term::list(vec![Term::symbol("unseal"), w, tok]));
    }

    // General application desugaring: (f a b c) => (((f a) b) c).
    let mut canon_items = Vec::with_capacity(items.len());
    for it in items {
        canon_items.push(canon_code(it)?);
    }

    if canon_items.len() <= 2 {
        return Ok(Term::list(canon_items));
    }

    let mut acc = Term::list(vec![canon_items[0].clone(), canon_items[1].clone()]);
    for arg in canon_items.into_iter().skip(2) {
        acc = Term::list(vec![acc, arg]);
    }
    Ok(acc)
}
