use super::*;

// Planner helper ownership: reusable planning/lowering utilities shared across
// stage2 orchestration and focused lowering modules.
pub(super) fn lower_list_is_nil(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let arg_ty = arg.ty();
    let idx = planner.alloc_local(arg_ty)?;
    // list/is-nil? only returns true for literal nil; all other scalar kinds are false.
    let is_nil = matches!(arg_ty, Ty::NilI32);
    Ok(PExpr::Let {
        bindings: vec![LetBinding { idx, expr: arg }],
        body: vec![PExpr::Bool(is_nil)],
        ty: Ty::BoolI32,
    })
}

pub(super) fn lower_data_tag(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let arg_ty = arg.ty();
    let idx = planner.alloc_local(arg_ty)?;
    let tag_sym = match arg_ty {
        Ty::NilI32 => ":nil",
        Ty::BoolI32 => ":bool",
        Ty::I64 => ":int",
        Ty::SymI32 => ":sym",
        Ty::StrI32 => ":str",
        Ty::BytesI32 => ":bytes",
    };
    let tag_id = planner.intern_symbol(tag_sym)?;
    Ok(PExpr::Let {
        bindings: vec![LetBinding { idx, expr: arg }],
        body: vec![PExpr::Sym(tag_id)],
        ty: Ty::SymI32,
    })
}

pub(super) fn planner_string_for_id(
    planner: &Planner,
    id: i32,
) -> Result<String, Stage2CompileError> {
    for (s, sid) in &planner.string_ids {
        if *sid == id {
            return Ok(s.clone());
        }
    }
    Err(Stage2CompileError::Internal(
        "string id missing from planner table".to_string(),
    ))
}

pub(super) fn planner_symbol_for_id(
    planner: &Planner,
    id: i32,
) -> Result<String, Stage2CompileError> {
    for (s, sid) in &planner.symbol_ids {
        if *sid == id {
            return Ok(s.clone());
        }
    }
    Err(Stage2CompileError::Internal(
        "symbol id missing from planner table".to_string(),
    ))
}

pub(super) fn planner_bytes_for_id(
    planner: &Planner,
    id: i32,
) -> Result<Vec<u8>, Stage2CompileError> {
    for (bs, bid) in &planner.bytes_ids {
        if *bid == id {
            return Ok(bs.clone());
        }
    }
    Err(Stage2CompileError::Internal(
        "bytes id missing from planner table".to_string(),
    ))
}

pub(super) fn planner_const_string_id(planner: &Planner, expr: &PExpr) -> Option<i32> {
    const_string_id_with_map(expr, &planner.local_const_string_ids)
}

pub(super) fn planner_const_int_value(planner: &Planner, expr: &PExpr) -> Option<i64> {
    const_int_value_with_map(expr, &planner.local_const_int_values)
}

pub(super) fn planner_const_symbol_id(planner: &Planner, expr: &PExpr) -> Option<i32> {
    const_symbol_id_with_map(expr, &planner.local_const_symbol_ids)
}

pub(super) fn planner_const_bytes_id(planner: &Planner, expr: &PExpr) -> Option<i32> {
    const_bytes_id_with_map(expr, &planner.local_const_bytes_ids)
}

pub(super) fn lower_str_concat(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if lhs.ty() != Ty::StrI32 || rhs.ty() != Ty::StrI32 {
        return Err(Stage2CompileError::Unsupported(
            "str/concat expects string arguments in stage2".to_string(),
        ));
    }
    lower_str_concat_expr(lhs, rhs, planner)
}

pub(super) fn lower_str_repeat(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if lhs.ty() != Ty::StrI32 || rhs.ty() != Ty::I64 {
        return Err(Stage2CompileError::Unsupported(
            "str/repeat expects (string, int) arguments in stage2".to_string(),
        ));
    }
    lower_str_repeat_expr(lhs, rhs, planner)
}

pub(super) fn lower_bytes_concat(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if lhs.ty() != Ty::BytesI32 || rhs.ty() != Ty::BytesI32 {
        return Err(Stage2CompileError::Unsupported(
            "bytes/concat expects bytes arguments in stage2".to_string(),
        ));
    }
    lower_bytes_concat_expr(lhs, rhs, planner)
}

pub(super) fn lower_bytes_get(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if lhs.ty() != Ty::BytesI32 || rhs.ty() != Ty::I64 {
        return Err(Stage2CompileError::Unsupported(
            "bytes/get expects (bytes, int) arguments in stage2".to_string(),
        ));
    }
    lower_bytes_get_expr(lhs, rhs, planner)
}

pub(super) fn ensure_scalar_cond_ty(cond_ty: Ty) -> Result<(), Stage2CompileError> {
    if matches!(
        cond_ty,
        Ty::BoolI32 | Ty::NilI32 | Ty::I64 | Ty::SymI32 | Ty::StrI32 | Ty::BytesI32
    ) {
        Ok(())
    } else {
        Err(Stage2CompileError::Unsupported(
            "if condition must be scalar (bool, nil, int, symbol, string, or bytes)".to_string(),
        ))
    }
}

pub(super) fn term_const_vector_expr_with_aliases(
    t: &Term,
    local_aliases: &BTreeMap<String, Vec<Term>>,
    global_aliases: &BTreeMap<String, Vec<Term>>,
) -> Result<Option<Vec<Term>>, Stage2CompileError> {
    if let Term::Symbol(sym) = t {
        if let Some(items) = local_aliases.get(sym) {
            return Ok(Some(items.clone()));
        }
        if let Some(items) = global_aliases.get(sym) {
            return Ok(Some(items.clone()));
        }
    }
    if let Term::Vector(items) = t {
        return Ok(Some(items.clone()));
    }
    let Some(xs) = t.as_proper_list() else {
        return Ok(None);
    };
    if xs.is_empty() {
        return Ok(None);
    }

    if xs.len() == 4 && matches!(xs[0], Term::Symbol(s) if s == "if") {
        let Some(cond) = term_const_if_condition_expr(xs[1]) else {
            return Ok(None);
        };
        let branch = if term_truthy(&cond) { xs[2] } else { xs[3] };
        return term_const_vector_expr_with_aliases(branch, local_aliases, global_aliases);
    }

    if xs.len() == 4
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && matches!(xs[1], Term::Symbol(s) if s == "vec/push")
    {
        let Some(mut items) =
            term_const_vector_expr_with_aliases(xs[2], local_aliases, global_aliases)?
        else {
            return Err(Stage2CompileError::Unsupported(
                "vec/push currently requires stage2-known vector literals".to_string(),
            ));
        };
        let Some(v) = term_const_data_expr(xs[3]) else {
            return Err(Stage2CompileError::Unsupported(
                "vec/push currently requires stage2-known data values".to_string(),
            ));
        };
        items.push(v);
        return Ok(Some(items));
    }

    if xs.len() == 2
        && let Some(inner) = xs[0].as_proper_list()
        && inner.len() == 2
        && matches!(inner[0], Term::Symbol(s) if s == "core/vec::push")
    {
        let Some(mut items) =
            term_const_vector_expr_with_aliases(inner[1], local_aliases, global_aliases)?
        else {
            return Err(Stage2CompileError::Unsupported(
                "core/vec::push currently requires stage2-known vector literals".to_string(),
            ));
        };
        let Some(v) = term_const_data_expr(xs[1]) else {
            return Err(Stage2CompileError::Unsupported(
                "core/vec::push currently requires stage2-known data values".to_string(),
            ));
        };
        items.push(v);
        return Ok(Some(items));
    }

    Ok(None)
}

pub(super) fn term_const_string_vector_ids_with_aliases(
    t: &Term,
    local_aliases: &BTreeMap<String, Vec<Term>>,
    global_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<Option<Vec<i32>>, Stage2CompileError> {
    let Some(items) = term_const_vector_expr_with_aliases(t, local_aliases, global_aliases)? else {
        return Ok(None);
    };
    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        let Term::Str(s) = &item else {
            return Err(Stage2CompileError::Unsupported(
                "str/join expects a vector of stage2-known string values".to_string(),
            ));
        };
        ids.push(planner.intern_string(s)?);
    }
    Ok(Some(ids))
}

pub(super) fn term_const_bytes_vector_ids_with_aliases(
    t: &Term,
    local_aliases: &BTreeMap<String, Vec<Term>>,
    global_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<Option<Vec<i32>>, Stage2CompileError> {
    let Some(items) = term_const_vector_expr_with_aliases(t, local_aliases, global_aliases)? else {
        return Ok(None);
    };
    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        let Term::Bytes(bs) = &item else {
            return Err(Stage2CompileError::Unsupported(
                "bytes/join expects a vector of stage2-known bytes values".to_string(),
            ));
        };
        ids.push(planner.intern_bytes(bs)?);
    }
    Ok(Some(ids))
}

pub(super) fn scalar_term_to_pexpr(
    t: &Term,
    planner: &mut Planner,
) -> Result<Option<PExpr>, Stage2CompileError> {
    match t {
        Term::Nil => Ok(Some(PExpr::Nil)),
        Term::Bool(b) => Ok(Some(PExpr::Bool(*b))),
        Term::Int(i) => {
            let n = i.to_i64().ok_or_else(|| {
                Stage2CompileError::Unsupported(
                    "vec/get supports only int literals in i64 range".to_string(),
                )
            })?;
            Ok(Some(PExpr::Int(n)))
        }
        Term::Symbol(s) => Ok(Some(PExpr::Sym(planner.intern_symbol(s)?))),
        Term::Str(s) => Ok(Some(PExpr::Str(planner.intern_string(s)?))),
        Term::Bytes(bs) => Ok(Some(PExpr::Bytes(planner.intern_bytes(bs)?))),
        _ => Ok(None),
    }
}

pub(super) fn term_const_scalar_vector_exprs_with_aliases(
    t: &Term,
    local_aliases: &BTreeMap<String, Vec<Term>>,
    global_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<Option<Vec<PExpr>>, Stage2CompileError> {
    let Some(items) = term_const_vector_expr_with_aliases(t, local_aliases, global_aliases)? else {
        return Ok(None);
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(e) = scalar_term_to_pexpr(&item, planner)? else {
            return Err(Stage2CompileError::Unsupported(
                "vec/get expects a vector of stage2-known scalar values".to_string(),
            ));
        };
        out.push(e);
    }
    Ok(Some(out))
}

pub(super) fn term_const_data_term(t: &Term) -> Option<Term> {
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

pub(super) fn term_const_quoted_data_term(t: &Term) -> Option<Term> {
    let xs = t.as_proper_list()?;
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "quote") {
        return Some(xs[1].clone());
    }
    None
}

pub(super) fn term_const_data_expr(t: &Term) -> Option<Term> {
    term_const_quoted_data_term(t).or_else(|| term_const_data_term(t))
}

pub(super) fn term_const_if_condition_expr(t: &Term) -> Option<Term> {
    if let Some(quoted) = term_const_quoted_data_term(t) {
        return Some(quoted);
    }
    match t {
        Term::Nil | Term::Bool(_) | Term::Int(_) | Term::Str(_) | Term::Bytes(_) => Some(t.clone()),
        _ => {
            let xs = t.as_proper_list()?;
            if xs.len() == 4 && matches!(xs[0], Term::Symbol(s) if s == "if") {
                let cond = term_const_if_condition_expr(xs[1])?;
                let branch = if term_truthy(&cond) { xs[2] } else { xs[3] };
                return term_const_if_condition_expr(branch)
                    .or_else(|| term_const_data_expr(branch));
            }
            if xs.len() == 4
                && matches!(xs[0], Term::Symbol(s) if s == "prim")
                && matches!(xs[1], Term::Symbol(s) if s == "int/lt?")
            {
                let a = term_const_i64_expr(xs[2])?;
                let b = term_const_i64_expr(xs[3])?;
                return Some(Term::Bool(a < b));
            }
            if xs.len() == 4
                && matches!(xs[0], Term::Symbol(s) if s == "prim")
                && matches!(xs[1], Term::Symbol(s) if s == "int/eq?")
            {
                let a = term_const_i64_expr(xs[2])?;
                let b = term_const_i64_expr(xs[3])?;
                return Some(Term::Bool(a == b));
            }
            if xs.len() == 4
                && matches!(xs[0], Term::Symbol(s) if s == "prim")
                && matches!(xs[1], Term::Symbol(s) if s == "core/eq?")
            {
                let a = term_const_data_expr(xs[2])?;
                let b = term_const_data_expr(xs[3])?;
                return Some(Term::Bool(a == b));
            }
            if xs.len() == 3
                && matches!(xs[0], Term::Symbol(s) if s == "prim")
                && matches!(xs[1], Term::Symbol(s) if s == "list/is-nil?")
            {
                let x = term_const_data_expr(xs[2])?;
                return Some(Term::Bool(matches!(x, Term::Nil)));
            }
            if xs.len() == 2
                && let Some(inner) = xs[0].as_proper_list()
                && inner.len() == 2
                && matches!(inner[0], Term::Symbol(s) if s == "core/int::lt?")
            {
                let a = term_const_i64_expr(inner[1])?;
                let b = term_const_i64_expr(xs[1])?;
                return Some(Term::Bool(a < b));
            }
            if xs.len() == 2
                && let Some(inner) = xs[0].as_proper_list()
                && inner.len() == 2
                && matches!(inner[0], Term::Symbol(s) if s == "core/int::eq?")
            {
                let a = term_const_i64_expr(inner[1])?;
                let b = term_const_i64_expr(xs[1])?;
                return Some(Term::Bool(a == b));
            }
            if xs.len() == 2
                && let Some(inner) = xs[0].as_proper_list()
                && inner.len() == 2
                && matches!(inner[0], Term::Symbol(s) if s == "core/eq?")
            {
                let a = term_const_data_expr(inner[1])?;
                let b = term_const_data_expr(xs[1])?;
                return Some(Term::Bool(a == b));
            }
            if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/list::is-nil?") {
                let x = term_const_data_expr(xs[1])?;
                return Some(Term::Bool(matches!(x, Term::Nil)));
            }
            None
        }
    }
}

pub(super) fn term_truthy(t: &Term) -> bool {
    !matches!(t, Term::Nil | Term::Bool(false))
}

pub(super) fn term_const_i64_expr(t: &Term) -> Option<i64> {
    let Term::Int(i) = term_const_data_expr(t)? else {
        return None;
    };
    i.to_i64()
}

pub(super) fn term_const_map_expr_with_aliases(
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

pub(super) fn resolve_map_alias_term(
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

pub(super) fn resolve_collection_aliases_term(
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

pub(super) fn resolve_scalar_aliases_term(
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

pub(super) fn scalar_term_from_pexpr_const(planner: &Planner, expr: &PExpr) -> Option<Term> {
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

pub(super) struct VecGetScope<'a> {
    pub(super) env: &'a BTreeMap<String, Local>,
    pub(super) global_env: &'a BTreeMap<String, Local>,
    pub(super) fn_defs: &'a BTreeMap<String, InlinableFnDef>,
    pub(super) local_fn_defs: &'a BTreeMap<String, InlinableFnDef>,
}
