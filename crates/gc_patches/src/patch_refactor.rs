use std::collections::BTreeSet;

use super::*;

pub(super) fn rename_symbol_in_forms(
    forms: Vec<Term>,
    from: &str,
    to: &str,
) -> Result<(Vec<Term>, usize), PatchError> {
    if from.is_empty() || to.is_empty() {
        return Err(PatchError::Validate(
            "rename-symbol requires non-empty :from and :to".to_string(),
        ));
    }
    if from == to {
        return Ok((forms, 0));
    }
    let mut rewrites = 0usize;
    let next = forms
        .into_iter()
        .map(|t| rename_symbol_term(t, from, to, &mut rewrites))
        .collect();
    Ok((next, rewrites))
}

pub(super) fn split_module_forms(
    forms: Vec<Term>,
    symbols: &BTreeSet<String>,
) -> Result<(Vec<Term>, Vec<Term>, usize), PatchError> {
    if symbols.is_empty() {
        return Err(PatchError::Validate(
            "split-module :symbols must be non-empty".to_string(),
        ));
    }
    let mut keep = Vec::new();
    let mut extracted = Vec::new();
    let mut moved = 0usize;
    for form in forms {
        if top_level_def_name(&form).is_some_and(|name| symbols.contains(name)) {
            extracted.push(form);
            moved += 1;
        } else {
            keep.push(form);
        }
    }
    if moved == 0 {
        return Err(PatchError::Validate(
            "split-module found no matching top-level definitions".to_string(),
        ));
    }
    Ok((keep, extracted, moved))
}

pub(super) fn rewrite_meta_list(
    forms: Vec<Term>,
    field: MetaListField,
    add: &[String],
    remove: &[String],
    replace: Option<&[String]>,
) -> Result<(Vec<Term>, usize), PatchError> {
    let mut changed = false;
    let mut out = Vec::with_capacity(forms.len());
    for form in forms {
        if top_level_def_name(&form) == Some("::meta") {
            let rhs = top_level_def_rhs(&form).ok_or_else(|| {
                PatchError::Validate("::meta definition must have a value".to_string())
            })?;
            let (mut map, quoted) = extract_meta_map(rhs)?;
            let key = TermOrdKey(Term::symbol(field.key_symbol()));
            let current = map.get(&key).cloned();
            let mut next_set: BTreeSet<String> = if let Some(repl) = replace {
                repl.iter().cloned().collect()
            } else {
                parse_symbol_vec(current.as_ref(), field.key_symbol())?
                    .into_iter()
                    .collect()
            };
            for sym in remove {
                next_set.remove(sym);
            }
            for sym in add {
                next_set.insert(sym.clone());
            }
            let next_vec = next_set
                .iter()
                .cloned()
                .map(Term::symbol)
                .collect::<Vec<_>>();
            let next_term = Term::Vector(next_vec);
            let update = match current {
                Some(cur) => cur != next_term,
                None => !next_set.is_empty() || replace.is_some(),
            };
            if update {
                map.insert(key, next_term);
                changed = true;
            }
            let next_rhs = if quoted {
                Term::list(vec![Term::symbol("quote"), Term::Map(map)])
            } else {
                Term::Map(map)
            };
            out.push(Term::list(vec![
                Term::symbol("def"),
                Term::symbol("::meta"),
                next_rhs,
            ]));
        } else {
            out.push(form);
        }
    }
    if !changed {
        return Err(PatchError::Validate(format!(
            "{} produced no module changes",
            field.op_symbol()
        )));
    }
    Ok((out, 1))
}

pub(super) fn migrate_contract_signature(
    forms: Vec<Term>,
    contract_symbol: &str,
    from_param: &str,
    to_param: &str,
) -> Result<(Vec<Term>, usize), PatchError> {
    if contract_symbol.is_empty() || from_param.is_empty() || to_param.is_empty() {
        return Err(PatchError::Validate(
            "migrate-contract-signature requires non-empty symbol/param values".to_string(),
        ));
    }
    if from_param == to_param {
        return Err(PatchError::Validate(
            "migrate-contract-signature :from-param and :to-param must differ".to_string(),
        ));
    }

    let mut updated = false;
    let mut out = Vec::with_capacity(forms.len());
    for form in forms {
        if top_level_def_name(&form) == Some(contract_symbol) {
            let rhs = top_level_def_rhs(&form).ok_or_else(|| {
                PatchError::Validate(
                    "migrate-contract-signature target definition is missing rhs".to_string(),
                )
            })?;
            let fn_items = rhs.as_proper_list().ok_or_else(|| {
                PatchError::Validate(
                    "migrate-contract-signature target rhs must be (fn ...)".to_string(),
                )
            })?;
            if fn_items.len() < 3 {
                return Err(PatchError::Validate(
                    "migrate-contract-signature target fn must have params and body".to_string(),
                ));
            }
            match fn_items[0] {
                Term::Symbol(s) if s == "fn" => {}
                _ => {
                    return Err(PatchError::Validate(
                        "migrate-contract-signature target rhs must be (fn ...)".to_string(),
                    ));
                }
            }

            let params = fn_items[1].as_proper_list().ok_or_else(|| {
                PatchError::Validate(
                    "migrate-contract-signature fn params must be a proper list".to_string(),
                )
            })?;
            if params.is_empty() {
                return Err(PatchError::Validate(
                    "migrate-contract-signature fn params must be non-empty".to_string(),
                ));
            }
            let mut next_params = Vec::with_capacity(params.len());
            for (i, p) in params.iter().enumerate() {
                let Term::Symbol(sym) = p else {
                    return Err(PatchError::Validate(
                        "migrate-contract-signature params must be symbols".to_string(),
                    ));
                };
                if i == 0 {
                    if sym != from_param {
                        return Err(PatchError::Validate(format!(
                            "migrate-contract-signature expected first param `{from_param}`, got `{sym}`"
                        )));
                    }
                    next_params.push(Term::symbol(to_param));
                } else {
                    if sym == to_param {
                        return Err(PatchError::Validate(format!(
                            "migrate-contract-signature :to-param `{to_param}` already exists in param list"
                        )));
                    }
                    next_params.push((*p).clone());
                }
            }

            let mut next_fn_items = Vec::with_capacity(fn_items.len());
            next_fn_items.push(Term::symbol("fn"));
            next_fn_items.push(Term::list(next_params));
            for body in fn_items.into_iter().skip(2) {
                next_fn_items.push(rename_bound_symbol(
                    body.clone(),
                    from_param,
                    to_param,
                    true,
                ));
            }
            let next_rhs = Term::list(next_fn_items);
            out.push(Term::list(vec![
                Term::symbol("def"),
                Term::symbol(contract_symbol),
                next_rhs,
            ]));
            updated = true;
        } else {
            out.push(form);
        }
    }

    if !updated {
        return Err(PatchError::Validate(format!(
            "migrate-contract-signature target `{contract_symbol}` not found"
        )));
    }
    Ok((out, 1))
}

fn parse_symbol_vec(t: Option<&Term>, field_name: &str) -> Result<Vec<String>, PatchError> {
    let Some(t) = t else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(PatchError::Validate(format!(
            "{field_name} in ::meta must be a vector"
        )));
    };
    let mut out = Vec::with_capacity(xs.len());
    for x in xs {
        match x {
            Term::Symbol(s) => out.push(s.clone()),
            other => {
                return Err(PatchError::Validate(format!(
                    "{field_name} entries must be symbols, got {}",
                    print_term(other)
                )));
            }
        }
    }
    Ok(out)
}

fn extract_meta_map(
    rhs: Term,
) -> Result<(std::collections::BTreeMap<TermOrdKey, Term>, bool), PatchError> {
    match rhs {
        Term::Map(m) => Ok((m, false)),
        quoted => {
            let items = quoted.as_proper_list().ok_or_else(|| {
                PatchError::Validate("::meta definition must be a map or (quote <map>)".to_string())
            })?;
            if items.len() != 2 {
                return Err(PatchError::Validate(
                    "::meta quote form must have exactly one payload".to_string(),
                ));
            }
            match items[0] {
                Term::Symbol(s) if s == "quote" => {}
                _ => {
                    return Err(PatchError::Validate(
                        "::meta definition must be a map or (quote <map>)".to_string(),
                    ));
                }
            }
            let Term::Map(m) = items[1].clone() else {
                return Err(PatchError::Validate(
                    "::meta quoted payload must be a map".to_string(),
                ));
            };
            Ok((m, true))
        }
    }
}

fn rename_symbol_term(t: Term, from: &str, to: &str, rewrites: &mut usize) -> Term {
    match t {
        Term::Symbol(s) => {
            if s == from {
                *rewrites += 1;
                Term::symbol(to)
            } else {
                Term::Symbol(s)
            }
        }
        Term::Pair(a, d) => Term::Pair(
            Box::new(rename_symbol_term(*a, from, to, rewrites)),
            Box::new(rename_symbol_term(*d, from, to, rewrites)),
        ),
        Term::Vector(xs) => Term::Vector(
            xs.into_iter()
                .map(|x| rename_symbol_term(x, from, to, rewrites))
                .collect(),
        ),
        Term::Map(m) => Term::Map(
            m.into_iter()
                .map(|(k, v)| {
                    let next_key = rename_symbol_term(k.0, from, to, rewrites);
                    let next_val = rename_symbol_term(v, from, to, rewrites);
                    (TermOrdKey(next_key), next_val)
                })
                .collect(),
        ),
        other => other,
    }
}

fn top_level_def_name(form: &Term) -> Option<&str> {
    let items = form.as_proper_list()?;
    if items.len() != 3 {
        return None;
    }
    match (items[0], items[1]) {
        (Term::Symbol(head), Term::Symbol(name)) if head == "def" => Some(name.as_str()),
        _ => None,
    }
}

fn top_level_def_rhs(form: &Term) -> Option<Term> {
    let items = form.as_proper_list()?;
    if items.len() != 3 {
        return None;
    }
    match items[0] {
        Term::Symbol(head) if head == "def" => Some(items[2].clone()),
        _ => None,
    }
}

fn rename_bound_symbol(t: Term, from: &str, to: &str, in_scope: bool) -> Term {
    match t {
        Term::Symbol(s) => {
            if in_scope && s == from {
                Term::symbol(to)
            } else {
                Term::Symbol(s)
            }
        }
        Term::Pair(a, d) => {
            let list_view = Term::Pair(a.clone(), d.clone());
            if let Some(items) = list_view.as_proper_list() {
                if !items.is_empty() {
                    if let Term::Symbol(head) = items[0] {
                        if head == "fn" && items.len() >= 3 {
                            if let Some(params) = items[1].as_proper_list() {
                                let shadows = params
                                    .iter()
                                    .any(|p| matches!(p, Term::Symbol(s) if s == from));
                                let body_scope = in_scope && !shadows;
                                let mut next = Vec::with_capacity(items.len());
                                next.push(Term::symbol("fn"));
                                next.push(items[1].clone());
                                for body in items.iter().skip(2) {
                                    next.push(rename_bound_symbol(
                                        (*body).clone(),
                                        from,
                                        to,
                                        body_scope,
                                    ));
                                }
                                return Term::list(next);
                            }
                        }
                        if head == "let" && items.len() >= 3 {
                            if let Some(bindings) = items[1].as_proper_list() {
                                let mut shadows = false;
                                let mut next_bindings = Vec::with_capacity(bindings.len());
                                for b in bindings {
                                    let Some(binding_items) = b.as_proper_list() else {
                                        return Term::Pair(
                                            Box::new(rename_bound_symbol(*a, from, to, in_scope)),
                                            Box::new(rename_bound_symbol(*d, from, to, in_scope)),
                                        );
                                    };
                                    if binding_items.len() != 2 {
                                        return Term::Pair(
                                            Box::new(rename_bound_symbol(*a, from, to, in_scope)),
                                            Box::new(rename_bound_symbol(*d, from, to, in_scope)),
                                        );
                                    }
                                    let binding_name = binding_items[0].clone();
                                    let binding_rhs = rename_bound_symbol(
                                        binding_items[1].clone(),
                                        from,
                                        to,
                                        in_scope,
                                    );
                                    if matches!(binding_name, Term::Symbol(ref s) if s == from) {
                                        shadows = true;
                                    }
                                    next_bindings.push(Term::list(vec![binding_name, binding_rhs]));
                                }
                                let body_scope = in_scope && !shadows;
                                let mut next = Vec::with_capacity(items.len());
                                next.push(Term::symbol("let"));
                                next.push(Term::list(next_bindings));
                                for body in items.iter().skip(2) {
                                    next.push(rename_bound_symbol(
                                        (*body).clone(),
                                        from,
                                        to,
                                        body_scope,
                                    ));
                                }
                                return Term::list(next);
                            }
                        }
                    }
                }
            }
            Term::Pair(
                Box::new(rename_bound_symbol(*a, from, to, in_scope)),
                Box::new(rename_bound_symbol(*d, from, to, in_scope)),
            )
        }
        Term::Vector(xs) => Term::Vector(
            xs.into_iter()
                .map(|x| rename_bound_symbol(x, from, to, in_scope))
                .collect(),
        ),
        Term::Map(m) => Term::Map(
            m.into_iter()
                .map(|(k, v)| {
                    (
                        TermOrdKey(rename_bound_symbol(k.0, from, to, in_scope)),
                        rename_bound_symbol(v, from, to, in_scope),
                    )
                })
                .collect(),
        ),
        other => other,
    }
}
