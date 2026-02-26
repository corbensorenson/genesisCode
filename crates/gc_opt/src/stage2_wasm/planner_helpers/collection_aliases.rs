use super::*;

pub(in super::super) fn term_const_data_term(t: &Term) -> Option<Term> {
    match t {
        Term::Nil
        | Term::Bool(_)
        | Term::Int(_)
        | Term::Symbol(_)
        | Term::Str(_)
        | Term::Bytes(_) => Some(t.clone()),
        Term::Pair(a, d) => {
            let a2 = term_const_data_term(a)?;
            let d2 = term_const_data_term(d)?;
            Some(Term::Pair(Box::new(a2), Box::new(d2)))
        }
        Term::Vector(xs) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(term_const_data_term(x)?);
            }
            Some(Term::Vector(out))
        }
        Term::Map(m) => {
            let mut out: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
            for (k, v) in m {
                let k2 = term_const_data_term(&k.0)?;
                let v2 = term_const_data_term(v)?;
                out.insert(TermOrdKey(k2), v2);
            }
            Some(Term::Map(out))
        }
    }
}

pub(in super::super) fn term_const_quoted_data_term(t: &Term) -> Option<Term> {
    let xs = t.as_proper_list()?;
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "quote") {
        return Some(xs[1].clone());
    }
    None
}

pub(in super::super) fn term_const_data_expr(t: &Term) -> Option<Term> {
    term_const_data_expr_with_aliases(t, &BTreeMap::new())
}

pub(in super::super) fn term_const_if_condition_expr(t: &Term) -> Option<Term> {
    term_const_if_condition_expr_with_aliases(t, &BTreeMap::new())
}

fn term_const_data_expr_with_aliases(
    t: &Term,
    scalar_aliases: &BTreeMap<String, Term>,
) -> Option<Term> {
    if let Term::Symbol(sym) = t
        && let Some(v) = scalar_aliases.get(sym)
    {
        return Some(v.clone());
    }
    if let Some(quoted) = term_const_quoted_data_term(t) {
        return Some(quoted);
    }
    if let Some(data) = term_const_data_term(t) {
        return Some(data);
    }
    let xs = t.as_proper_list()?;
    if xs.is_empty() {
        return None;
    }
    if matches!(xs[0], Term::Symbol(s) if s == "begin") {
        let mut last = None;
        for expr in xs.iter().skip(1) {
            last = term_const_data_expr_with_aliases(expr, scalar_aliases);
        }
        return last;
    }
    if matches!(xs[0], Term::Symbol(s) if s == "let") {
        if xs.len() < 3 {
            return None;
        }
        let bindings = xs[1].as_proper_list()?;
        let mut scoped = scalar_aliases.clone();
        for binding in bindings {
            let pair = binding.as_proper_list()?;
            if pair.len() != 2 {
                return None;
            }
            let Term::Symbol(name) = pair[0] else {
                return None;
            };
            let rhs = term_const_data_expr_with_aliases(pair[1], &scoped)?;
            scoped.insert(name.clone(), rhs);
        }
        let mut last = None;
        for expr in xs.iter().skip(2) {
            last = term_const_data_expr_with_aliases(expr, &scoped);
        }
        return last;
    }
    if xs.len() == 4 && matches!(xs[0], Term::Symbol(s) if s == "if") {
        let cond = term_const_if_condition_expr_with_aliases(xs[1], scalar_aliases)?;
        let branch = if term_truthy(&cond) { xs[2] } else { xs[3] };
        return term_const_data_expr_with_aliases(branch, scalar_aliases);
    }
    None
}

fn term_const_if_condition_expr_with_aliases(
    t: &Term,
    scalar_aliases: &BTreeMap<String, Term>,
) -> Option<Term> {
    if let Term::Symbol(sym) = t
        && let Some(v) = scalar_aliases.get(sym)
    {
        return Some(v.clone());
    }
    if let Some(quoted) = term_const_quoted_data_term(t) {
        return Some(quoted);
    }
    match t {
        Term::Nil | Term::Bool(_) | Term::Int(_) | Term::Str(_) | Term::Bytes(_) => Some(t.clone()),
        _ => {
            let xs = t.as_proper_list()?;
            if xs.is_empty() {
                return None;
            }
            if matches!(xs[0], Term::Symbol(s) if s == "begin") {
                let mut last = None;
                for expr in xs.iter().skip(1) {
                    last = term_const_if_condition_expr_with_aliases(expr, scalar_aliases)
                        .or_else(|| term_const_data_expr_with_aliases(expr, scalar_aliases));
                }
                return last;
            }
            if matches!(xs[0], Term::Symbol(s) if s == "let") {
                if xs.len() < 3 {
                    return None;
                }
                let bindings = xs[1].as_proper_list()?;
                let mut scoped = scalar_aliases.clone();
                for binding in bindings {
                    let pair = binding.as_proper_list()?;
                    if pair.len() != 2 {
                        return None;
                    }
                    let Term::Symbol(name) = pair[0] else {
                        return None;
                    };
                    let rhs = term_const_data_expr_with_aliases(pair[1], &scoped)?;
                    scoped.insert(name.clone(), rhs);
                }
                let mut last = None;
                for expr in xs.iter().skip(2) {
                    last = term_const_if_condition_expr_with_aliases(expr, &scoped)
                        .or_else(|| term_const_data_expr_with_aliases(expr, &scoped));
                }
                return last;
            }
            if xs.len() == 4 && matches!(xs[0], Term::Symbol(s) if s == "if") {
                let cond = term_const_if_condition_expr_with_aliases(xs[1], scalar_aliases)?;
                let branch = if term_truthy(&cond) { xs[2] } else { xs[3] };
                return term_const_if_condition_expr_with_aliases(branch, scalar_aliases)
                    .or_else(|| term_const_data_expr_with_aliases(branch, scalar_aliases));
            }
            if xs.len() == 4
                && matches!(xs[0], Term::Symbol(s) if s == "prim")
                && matches!(xs[1], Term::Symbol(s) if s == "int/lt?")
            {
                let a = term_const_i64_expr_with_aliases(xs[2], scalar_aliases)?;
                let b = term_const_i64_expr_with_aliases(xs[3], scalar_aliases)?;
                return Some(Term::Bool(a < b));
            }
            if xs.len() == 4
                && matches!(xs[0], Term::Symbol(s) if s == "prim")
                && matches!(xs[1], Term::Symbol(s) if s == "int/eq?")
            {
                let a = term_const_i64_expr_with_aliases(xs[2], scalar_aliases)?;
                let b = term_const_i64_expr_with_aliases(xs[3], scalar_aliases)?;
                return Some(Term::Bool(a == b));
            }
            if xs.len() == 4
                && matches!(xs[0], Term::Symbol(s) if s == "prim")
                && matches!(xs[1], Term::Symbol(s) if s == "core/eq?")
            {
                let a = term_const_data_expr_with_aliases(xs[2], scalar_aliases)?;
                let b = term_const_data_expr_with_aliases(xs[3], scalar_aliases)?;
                return Some(Term::Bool(a == b));
            }
            if xs.len() == 3
                && matches!(xs[0], Term::Symbol(s) if s == "prim")
                && matches!(xs[1], Term::Symbol(s) if s == "list/is-nil?")
            {
                let x = term_const_data_expr_with_aliases(xs[2], scalar_aliases)?;
                return Some(Term::Bool(matches!(x, Term::Nil)));
            }
            if xs.len() == 2
                && let Some(inner) = xs[0].as_proper_list()
                && inner.len() == 2
                && matches!(inner[0], Term::Symbol(s) if s == "core/int::lt?")
            {
                let a = term_const_i64_expr_with_aliases(inner[1], scalar_aliases)?;
                let b = term_const_i64_expr_with_aliases(xs[1], scalar_aliases)?;
                return Some(Term::Bool(a < b));
            }
            if xs.len() == 2
                && let Some(inner) = xs[0].as_proper_list()
                && inner.len() == 2
                && matches!(inner[0], Term::Symbol(s) if s == "core/int::eq?")
            {
                let a = term_const_i64_expr_with_aliases(inner[1], scalar_aliases)?;
                let b = term_const_i64_expr_with_aliases(xs[1], scalar_aliases)?;
                return Some(Term::Bool(a == b));
            }
            if xs.len() == 2
                && let Some(inner) = xs[0].as_proper_list()
                && inner.len() == 2
                && matches!(inner[0], Term::Symbol(s) if s == "core/eq?")
            {
                let a = term_const_data_expr_with_aliases(inner[1], scalar_aliases)?;
                let b = term_const_data_expr_with_aliases(xs[1], scalar_aliases)?;
                return Some(Term::Bool(a == b));
            }
            if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/list::is-nil?") {
                let x = term_const_data_expr_with_aliases(xs[1], scalar_aliases)?;
                return Some(Term::Bool(matches!(x, Term::Nil)));
            }
            None
        }
    }
}

pub(in super::super) fn term_truthy(t: &Term) -> bool {
    !matches!(t, Term::Nil | Term::Bool(false))
}

fn term_const_i64_expr_with_aliases(
    t: &Term,
    scalar_aliases: &BTreeMap<String, Term>,
) -> Option<i64> {
    let Term::Int(i) = term_const_data_expr_with_aliases(t, scalar_aliases)? else {
        return None;
    };
    i.to_i64()
}

pub(in super::super) fn term_const_map_expr_with_aliases(
    t: &Term,
    local_aliases: &BTreeMap<String, BTreeMap<TermOrdKey, Term>>,
    global_aliases: &BTreeMap<String, BTreeMap<TermOrdKey, Term>>,
) -> Result<Option<BTreeMap<TermOrdKey, Term>>, Stage2CompileError> {
    if let Term::Symbol(sym) = t {
        if let Some(items) = local_aliases.get(sym) {
            return Ok(Some(items.clone()));
        }
        if let Some(items) = global_aliases.get(sym) {
            return Ok(Some(items.clone()));
        }
    }
    if let Term::Map(items) = t {
        return Ok(Some(items.clone()));
    }
    let Some(xs) = t.as_proper_list() else {
        return Ok(None);
    };
    if xs.is_empty() {
        return Ok(None);
    }

    if matches!(xs[0], Term::Symbol(s) if s == "begin") {
        let mut last = None;
        for expr in xs.iter().skip(1) {
            last = term_const_map_expr_with_aliases(expr, local_aliases, global_aliases)?;
        }
        return Ok(last);
    }

    if matches!(xs[0], Term::Symbol(s) if s == "let") {
        if xs.len() < 3 {
            return Ok(None);
        }
        let Some(bindings) = xs[1].as_proper_list() else {
            return Ok(None);
        };
        let mut scoped_aliases = local_aliases.clone();
        for binding in bindings {
            let Some(pair) = binding.as_proper_list() else {
                return Ok(None);
            };
            if pair.len() != 2 {
                return Ok(None);
            }
            let Term::Symbol(name) = pair[0] else {
                return Ok(None);
            };
            if let Some(items) =
                term_const_map_expr_with_aliases(pair[1], &scoped_aliases, global_aliases)?
            {
                scoped_aliases.insert(name.clone(), items);
            } else {
                scoped_aliases.remove(name);
            }
        }
        let mut last = None;
        for expr in xs.iter().skip(2) {
            last = term_const_map_expr_with_aliases(expr, &scoped_aliases, global_aliases)?;
        }
        return Ok(last);
    }

    if xs.len() == 4 && matches!(xs[0], Term::Symbol(s) if s == "if") {
        let Some(cond) = term_const_if_condition_expr(xs[1]) else {
            return Ok(None);
        };
        let branch = if term_truthy(&cond) { xs[2] } else { xs[3] };
        return term_const_map_expr_with_aliases(branch, local_aliases, global_aliases);
    }

    if xs.len() == 5
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && matches!(xs[1], Term::Symbol(s) if s == "map/put")
    {
        let Some(mut map) = term_const_map_expr_with_aliases(xs[2], local_aliases, global_aliases)?
        else {
            return Err(Stage2CompileError::Unsupported(
                "map/put currently requires stage2-known map literals".to_string(),
            ));
        };
        let Some(k) = term_const_data_expr(xs[3]) else {
            return Err(Stage2CompileError::Unsupported(
                "map/put currently requires stage2-known data keys".to_string(),
            ));
        };
        let Some(v) = term_const_data_expr(xs[4]) else {
            return Err(Stage2CompileError::Unsupported(
                "map/put currently requires stage2-known data values".to_string(),
            ));
        };
        map.insert(TermOrdKey(k), v);
        return Ok(Some(map));
    }

    if xs.len() == 4
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && matches!(xs[1], Term::Symbol(s) if s == "map/merge")
    {
        let Some(mut left) =
            term_const_map_expr_with_aliases(xs[2], local_aliases, global_aliases)?
        else {
            return Err(Stage2CompileError::Unsupported(
                "map/merge currently requires stage2-known map literals".to_string(),
            ));
        };
        let Some(right) = term_const_map_expr_with_aliases(xs[3], local_aliases, global_aliases)?
        else {
            return Err(Stage2CompileError::Unsupported(
                "map/merge currently requires stage2-known map literals".to_string(),
            ));
        };
        for (k, v) in right {
            left.insert(k, v);
        }
        return Ok(Some(left));
    }

    if xs.len() == 2 {
        if let Some(inner) = xs[0].as_proper_list()
            && inner.len() == 2
            && matches!(inner[0], Term::Symbol(s) if s == "core/map::merge")
        {
            let Some(mut left) =
                term_const_map_expr_with_aliases(inner[1], local_aliases, global_aliases)?
            else {
                return Err(Stage2CompileError::Unsupported(
                    "core/map::merge currently requires stage2-known map literals".to_string(),
                ));
            };
            let Some(right) =
                term_const_map_expr_with_aliases(xs[1], local_aliases, global_aliases)?
            else {
                return Err(Stage2CompileError::Unsupported(
                    "core/map::merge currently requires stage2-known map literals".to_string(),
                ));
            };
            for (k, v) in right {
                left.insert(k, v);
            }
            return Ok(Some(left));
        }

        if let Some(inner) = xs[0].as_proper_list()
            && inner.len() == 2
            && let Some(inner2) = inner[0].as_proper_list()
            && inner2.len() == 2
            && matches!(inner2[0], Term::Symbol(s) if s == "core/map::put")
        {
            let Some(mut map) =
                term_const_map_expr_with_aliases(inner2[1], local_aliases, global_aliases)?
            else {
                return Err(Stage2CompileError::Unsupported(
                    "core/map::put currently requires stage2-known map literals".to_string(),
                ));
            };
            let Some(k) = term_const_data_expr(inner[1]) else {
                return Err(Stage2CompileError::Unsupported(
                    "core/map::put currently requires stage2-known data keys".to_string(),
                ));
            };
            let Some(v) = term_const_data_expr(xs[1]) else {
                return Err(Stage2CompileError::Unsupported(
                    "core/map::put currently requires stage2-known data values".to_string(),
                ));
            };
            map.insert(TermOrdKey(k), v);
            return Ok(Some(map));
        }
    }

    Ok(None)
}

pub(in super::super) fn resolve_map_alias_term(
    t: &Term,
    map_aliases: &BTreeMap<String, BTreeMap<TermOrdKey, Term>>,
) -> Term {
    match t {
        Term::Symbol(sym) => map_aliases
            .get(sym)
            .cloned()
            .map(Term::Map)
            .unwrap_or_else(|| t.clone()),
        Term::Vector(items) => Term::Vector(
            items
                .iter()
                .map(|item| resolve_map_alias_term(item, map_aliases))
                .collect(),
        ),
        Term::Map(items) => {
            let mut out = BTreeMap::new();
            for (k, v) in items {
                let key = resolve_map_alias_term(&k.0, map_aliases);
                let val = resolve_map_alias_term(v, map_aliases);
                out.insert(TermOrdKey(key), val);
            }
            Term::Map(out)
        }
        _ => {
            if let Some(xs) = t.as_proper_list() {
                let resolved: Vec<Term> = xs
                    .iter()
                    .map(|item| resolve_map_alias_term(item, map_aliases))
                    .collect();
                Term::list(resolved)
            } else {
                t.clone()
            }
        }
    }
}

pub(in super::super) fn resolve_collection_aliases_term(
    t: &Term,
    vec_aliases: &BTreeMap<String, Vec<Term>>,
    map_aliases: &BTreeMap<String, BTreeMap<TermOrdKey, Term>>,
) -> Term {
    match t {
        Term::Symbol(sym) => {
            if let Some(items) = map_aliases.get(sym) {
                return Term::Map(items.clone());
            }
            if let Some(items) = vec_aliases.get(sym) {
                return Term::Vector(items.clone());
            }
            t.clone()
        }
        Term::Vector(items) => Term::Vector(
            items
                .iter()
                .map(|item| resolve_collection_aliases_term(item, vec_aliases, map_aliases))
                .collect(),
        ),
        Term::Map(items) => {
            let mut out = BTreeMap::new();
            for (k, v) in items {
                let key = resolve_collection_aliases_term(&k.0, vec_aliases, map_aliases);
                let val = resolve_collection_aliases_term(v, vec_aliases, map_aliases);
                out.insert(TermOrdKey(key), val);
            }
            Term::Map(out)
        }
        _ => {
            let Some(xs) = t.as_proper_list() else {
                return t.clone();
            };
            if !xs.is_empty() {
                if matches!(xs[0], Term::Symbol(s) if s == "quote") {
                    return t.clone();
                }
                // Avoid alias substitution under binders in generic planning.
                if matches!(xs[0], Term::Symbol(s) if s == "fn" || s == "let") {
                    return t.clone();
                }
            }
            Term::list(
                xs.iter()
                    .map(|item| resolve_collection_aliases_term(item, vec_aliases, map_aliases))
                    .collect(),
            )
        }
    }
}

pub(in super::super) fn resolve_scalar_aliases_term(
    t: &Term,
    scalar_aliases: &BTreeMap<String, Term>,
) -> Term {
    match t {
        Term::Symbol(sym) => scalar_aliases
            .get(sym)
            .cloned()
            .unwrap_or_else(|| t.clone()),
        Term::Vector(items) => Term::Vector(
            items
                .iter()
                .map(|item| resolve_scalar_aliases_term(item, scalar_aliases))
                .collect(),
        ),
        Term::Map(items) => {
            let mut out = BTreeMap::new();
            for (k, v) in items {
                let key = resolve_scalar_aliases_term(&k.0, scalar_aliases);
                let val = resolve_scalar_aliases_term(v, scalar_aliases);
                out.insert(TermOrdKey(key), val);
            }
            Term::Map(out)
        }
        _ => {
            let Some(xs) = t.as_proper_list() else {
                return t.clone();
            };
            if !xs.is_empty() {
                if matches!(xs[0], Term::Symbol(s) if s == "quote") {
                    return t.clone();
                }
                if matches!(xs[0], Term::Symbol(s) if s == "fn") {
                    return t.clone();
                }
            }
            Term::list(
                xs.iter()
                    .map(|item| resolve_scalar_aliases_term(item, scalar_aliases))
                    .collect(),
            )
        }
    }
}

pub(in super::super) fn scalar_term_from_pexpr_const(
    planner: &Planner,
    expr: &PExpr,
) -> Option<Term> {
    match expr {
        PExpr::Nil => Some(Term::Nil),
        PExpr::Bool(b) => Some(Term::Bool(*b)),
        PExpr::Int(n) => Some(Term::Int((*n).into())),
        _ => {
            if let Some(id) = planner_const_symbol_id(planner, expr)
                && let Ok(sym) = planner_symbol_for_id(planner, id)
            {
                return Some(Term::Symbol(sym));
            }
            if let Some(id) = planner_const_string_id(planner, expr)
                && let Ok(s) = planner_string_for_id(planner, id)
            {
                return Some(Term::Str(s));
            }
            if let Some(id) = planner_const_bytes_id(planner, expr)
                && let Ok(bs) = planner_bytes_for_id(planner, id)
            {
                return Some(Term::Bytes(bs.into()));
            }
            None
        }
    }
}
