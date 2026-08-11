use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{SpecialForm, Term, TermOrdKey, hash_term};

use crate::ModuleForTypecheck;

pub(super) fn module_metadata(forms: &[Term]) -> Result<Option<Term>, String> {
    let mut metadata = None;
    for form in forms {
        let Some(items) = form.as_proper_list() else {
            continue;
        };
        if items.len() != 3
            || !matches!(items[0], Term::Symbol(head) if SpecialForm::from_symbol(head) == Some(SpecialForm::Def))
            || !matches!(items[1], Term::Symbol(name) if name == "::meta")
        {
            continue;
        }
        if metadata.is_some() {
            return Err("module contains more than one ::meta definition".to_string());
        }
        metadata = Some(metadata_payload(items[2])?);
    }
    Ok(metadata)
}

pub(super) fn rewrite_syntax_heads(forms: &mut [Term], from: &str, to: &str) -> usize {
    let mut count = 0;
    for form in forms {
        *form = stacker::maybe_grow(32 * 1024, 4 * 1024 * 1024, || {
            rewrite_syntax_term(form.clone(), from, to, &mut count)
        });
    }
    count
}

pub(super) fn rename_api_symbol(forms: &mut [Term], from: &str, to: &str) -> usize {
    let mut count = 0;
    for form in forms {
        *form = stacker::maybe_grow(32 * 1024, 4 * 1024 * 1024, || {
            rewrite_top_form(form.clone(), from, to, &mut count)
        });
    }
    count
}

pub(super) fn reject_api_definition_collision(
    modules: &[ModuleForTypecheck],
    from: &str,
    to: &str,
) -> Result<(), String> {
    let mut definitions = BTreeSet::new();
    for module in modules {
        for form in &module.forms {
            if let Some(name) = top_level_def_name(form) {
                definitions.insert(name.to_string());
            }
        }
    }
    if definitions.contains(from) && definitions.contains(to) {
        return Err(format!(
            "API target {to} already has a package definition while renaming {from}"
        ));
    }
    Ok(())
}

pub(super) fn replace_metadata_field(
    forms: &mut [Term],
    field: &str,
    expected: Option<&Term>,
    replacement: Option<&Term>,
) -> Result<(), String> {
    let mut found = false;
    for form in forms.iter_mut() {
        let Some(items) = form.as_proper_list() else {
            continue;
        };
        if items.len() != 3
            || !matches!(items[0], Term::Symbol(head) if SpecialForm::from_symbol(head) == Some(SpecialForm::Def))
            || !matches!(items[1], Term::Symbol(name) if name == "::meta")
        {
            continue;
        }
        if found {
            return Err("module contains more than one ::meta definition".to_string());
        }
        found = true;
        let (mut metadata, quoted) = metadata_map(items[2])?;
        let key = TermOrdKey(Term::symbol(field));
        let actual = metadata.get(&key);
        if actual != expected {
            return Err(format!(
                "format field {field} expected {}, found {}",
                optional_term(expected),
                optional_term(actual)
            ));
        }
        match replacement {
            Some(value) => {
                metadata.insert(key, value.clone());
            }
            None => {
                metadata.remove(&key);
            }
        }
        let payload = Term::Map(metadata);
        let rhs = if quoted {
            Term::list(vec![Term::symbol("quote"), payload])
        } else {
            payload
        };
        *form = Term::list(vec![Term::symbol("def"), Term::symbol("::meta"), rhs]);
    }
    if !found {
        return Err("replace-format-field requires an existing ::meta definition".to_string());
    }
    Ok(())
}

fn rewrite_syntax_term(term: Term, from: &str, to: &str, count: &mut usize) -> Term {
    match term {
        Term::Pair(_, _) => {
            let list = term.as_proper_list();
            let Some(items) = list else {
                let Term::Pair(car, cdr) = term else {
                    return term;
                };
                return Term::Pair(
                    Box::new(rewrite_syntax_term(*car, from, to, count)),
                    Box::new(rewrite_syntax_term(*cdr, from, to, count)),
                );
            };
            if matches!(items.first(), Some(Term::Symbol(head)) if SpecialForm::from_symbol(head) == Some(SpecialForm::Quote))
            {
                return term;
            }
            let mut next = Vec::with_capacity(items.len());
            for (index, item) in items.into_iter().enumerate() {
                if index == 0 && matches!(item, Term::Symbol(head) if head == from) {
                    *count += 1;
                    next.push(Term::symbol(to));
                } else {
                    next.push(rewrite_syntax_term(item.clone(), from, to, count));
                }
            }
            Term::list(next)
        }
        Term::Map(entries) => Term::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key, rewrite_syntax_term(value, from, to, count)))
                .collect(),
        ),
        Term::Vector(_) => term,
        _ => term,
    }
}

fn rewrite_top_form(term: Term, from: &str, to: &str, count: &mut usize) -> Term {
    let Some(items) = term.as_proper_list() else {
        return rewrite_api_term(term, from, to, &BTreeSet::new(), count);
    };
    if items.len() == 3
        && matches!(items[0], Term::Symbol(head) if SpecialForm::from_symbol(head) == Some(SpecialForm::Def))
        && let Term::Symbol(name) = items[1]
    {
        if name == "::meta" {
            return rewrite_metadata_symbols(term, from, to, count);
        }
        let next_name = if name == from {
            *count += 1;
            Term::symbol(to)
        } else {
            items[1].clone()
        };
        return Term::list(vec![
            Term::symbol("def"),
            next_name,
            rewrite_api_term(items[2].clone(), from, to, &BTreeSet::new(), count),
        ]);
    }
    rewrite_api_term(term, from, to, &BTreeSet::new(), count)
}

fn rewrite_api_term(
    term: Term,
    from: &str,
    to: &str,
    bound: &BTreeSet<String>,
    count: &mut usize,
) -> Term {
    match term {
        Term::Symbol(symbol) => {
            if symbol == from && !bound.contains(&symbol) {
                *count += 1;
                Term::symbol(to)
            } else {
                Term::Symbol(symbol)
            }
        }
        Term::Pair(_, _) => {
            let Some(items) = term.as_proper_list() else {
                let Term::Pair(car, cdr) = term else {
                    return term;
                };
                return Term::Pair(
                    Box::new(rewrite_api_term(*car, from, to, bound, count)),
                    Box::new(rewrite_api_term(*cdr, from, to, bound, count)),
                );
            };
            if items.is_empty()
                || matches!(items[0], Term::Symbol(head) if SpecialForm::from_symbol(head) == Some(SpecialForm::Quote))
            {
                return term;
            }
            if matches!(items[0], Term::Symbol(head) if SpecialForm::from_symbol(head) == Some(SpecialForm::Fn))
                && items.len() >= 3
            {
                return rewrite_fn(items, from, to, bound, count);
            }
            if matches!(items[0], Term::Symbol(head) if SpecialForm::from_symbol(head) == Some(SpecialForm::Let))
                && items.len() >= 3
            {
                return rewrite_let(items, from, to, bound, count);
            }
            Term::list(
                items
                    .into_iter()
                    .map(|item| rewrite_api_term(item.clone(), from, to, bound, count))
                    .collect(),
            )
        }
        Term::Map(entries) => Term::Map(
            entries
                .into_iter()
                .map(|(key, value)| (key, rewrite_api_term(value, from, to, bound, count)))
                .collect(),
        ),
        Term::Vector(_) => term,
        _ => term,
    }
}

fn rewrite_fn(
    items: Vec<&Term>,
    from: &str,
    to: &str,
    bound: &BTreeSet<String>,
    count: &mut usize,
) -> Term {
    let mut body_bound = bound.clone();
    if let Some(parameters) = items[1].as_proper_list() {
        for parameter in parameters {
            if let Term::Symbol(symbol) = parameter {
                body_bound.insert(symbol.clone());
            }
        }
    }
    let mut next = vec![items[0].clone(), items[1].clone()];
    next.extend(
        items
            .into_iter()
            .skip(2)
            .map(|item| rewrite_api_term(item.clone(), from, to, &body_bound, count)),
    );
    Term::list(next)
}

fn rewrite_let(
    items: Vec<&Term>,
    from: &str,
    to: &str,
    bound: &BTreeSet<String>,
    count: &mut usize,
) -> Term {
    let Some(bindings) = items[1].as_proper_list() else {
        return Term::list(
            items
                .into_iter()
                .map(|item| rewrite_api_term(item.clone(), from, to, bound, count))
                .collect(),
        );
    };
    let mut body_bound = bound.clone();
    let mut next_bindings = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let Some(pair) = binding.as_proper_list() else {
            next_bindings.push(rewrite_api_term(binding.clone(), from, to, bound, count));
            continue;
        };
        if pair.len() != 2 {
            next_bindings.push(rewrite_api_term(binding.clone(), from, to, bound, count));
            continue;
        }
        next_bindings.push(Term::list(vec![
            pair[0].clone(),
            rewrite_api_term(pair[1].clone(), from, to, bound, count),
        ]));
        if let Term::Symbol(symbol) = pair[0] {
            body_bound.insert(symbol.clone());
        }
    }
    let mut next = vec![items[0].clone(), Term::list(next_bindings)];
    next.extend(
        items
            .into_iter()
            .skip(2)
            .map(|item| rewrite_api_term(item.clone(), from, to, &body_bound, count)),
    );
    Term::list(next)
}

fn rewrite_metadata_symbols(term: Term, from: &str, to: &str, count: &mut usize) -> Term {
    let Some(items) = term.as_proper_list() else {
        return term;
    };
    let Ok((metadata, quoted)) = metadata_map(items[2]) else {
        return term;
    };
    let next = rewrite_data_symbols(Term::Map(metadata), from, to, count);
    let rhs = if quoted {
        Term::list(vec![Term::symbol("quote"), next])
    } else {
        next
    };
    Term::list(vec![items[0].clone(), items[1].clone(), rhs])
}

fn rewrite_data_symbols(term: Term, from: &str, to: &str, count: &mut usize) -> Term {
    match term {
        Term::Symbol(symbol) => {
            if symbol == from {
                *count += 1;
                Term::symbol(to)
            } else {
                Term::Symbol(symbol)
            }
        }
        Term::Pair(car, cdr) => Term::Pair(
            Box::new(rewrite_data_symbols(*car, from, to, count)),
            Box::new(rewrite_data_symbols(*cdr, from, to, count)),
        ),
        Term::Vector(values) => Term::Vector(
            values
                .into_iter()
                .map(|value| rewrite_data_symbols(value, from, to, count))
                .collect(),
        ),
        Term::Map(entries) => Term::Map(
            entries
                .into_iter()
                .map(|(key, value)| {
                    (
                        TermOrdKey(rewrite_data_symbols(key.0, from, to, count)),
                        rewrite_data_symbols(value, from, to, count),
                    )
                })
                .collect(),
        ),
        other => other,
    }
}

fn metadata_payload(term: &Term) -> Result<Term, String> {
    let (metadata, _) = metadata_map(term)?;
    Ok(Term::Map(metadata))
}

fn metadata_map(term: &Term) -> Result<(BTreeMap<TermOrdKey, Term>, bool), String> {
    if let Term::Map(metadata) = term {
        return Ok((metadata.clone(), false));
    }
    let items = term
        .as_proper_list()
        .ok_or_else(|| "::meta value must be a map or quoted map".to_string())?;
    if items.len() != 2 || !matches!(items[0], Term::Symbol(head) if head == "quote") {
        return Err("::meta value must be a map or quoted map".to_string());
    }
    let Term::Map(metadata) = items[1] else {
        return Err("::meta quoted value must be a map".to_string());
    };
    Ok((metadata.clone(), true))
}

fn top_level_def_name(term: &Term) -> Option<&str> {
    let items = term.as_proper_list()?;
    if items.len() != 3
        || !matches!(items[0], Term::Symbol(head) if SpecialForm::from_symbol(head) == Some(SpecialForm::Def))
    {
        return None;
    }
    match items[1] {
        Term::Symbol(name) => Some(name),
        _ => None,
    }
}

fn optional_term(term: Option<&Term>) -> String {
    term.map(|value| format!("coreform-term-h:{}", hex32(hash_term(value))))
        .unwrap_or_else(|| "<absent>".to_string())
}

fn hex32(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}
