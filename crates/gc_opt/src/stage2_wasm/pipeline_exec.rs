use super::*;

pub(super) fn eval_original_data(
    forms: &[Term],
) -> Result<(Stage2ValueKind, Term, [u8; 32]), Stage2CompileError> {
    fn hex32(bytes: [u8; 32]) -> String {
        blake3::Hash::from_bytes(bytes).to_hex().to_string()
    }

    let mut ctx = EvalCtx::with_step_limit(Some(STAGE2_BASELINE_STEP_LIMIT));
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, forms)
        .map_err(|e| Stage2CompileError::Unsupported(format!("kernel eval failed: {e}")))?;
    match v {
        Value::Data(term) => {
            let kind = match &term {
                Term::Int(_) => Stage2ValueKind::Int,
                Term::Bool(_) => Stage2ValueKind::Bool,
                Term::Nil => Stage2ValueKind::Nil,
                Term::Symbol(_) => Stage2ValueKind::Sym,
                Term::Str(_) => Stage2ValueKind::Str,
                Term::Bytes(_) => Stage2ValueKind::Bytes,
                Term::Pair(_, _) | Term::Vector(_) | Term::Map(_) => Stage2ValueKind::Term,
            };
            let h = value_hash(&Value::Data(term.clone()));
            Ok((kind, term, h))
        }
        Value::Vector(items) => {
            let value = Value::Vector(items);
            let term = value.to_term_for_log(None);
            let h = value_hash(&Value::Data(term.clone()));
            Ok((Stage2ValueKind::Term, term, h))
        }
        Value::Map(items) => {
            let value = Value::Map(items);
            let term = value.to_term_for_log(None);
            let h = value_hash(&Value::Data(term.clone()));
            Ok((Stage2ValueKind::Term, term, h))
        }
        Value::EffectProgram(program) => {
            // Stage2 cannot execute effects; for translation validation we compare a deterministic
            // projection that preserves effect-program identity across kernel/wasm boundaries.
            let effect_hash = value_hash(&Value::EffectProgram(program.clone()));
            let mut mm = BTreeMap::new();
            mm.insert(
                TermOrdKey(Term::symbol(":stage2/value-kind")),
                Term::Str("effect-program".to_string()),
            );
            mm.insert(
                TermOrdKey(Term::symbol(":effect-program-h")),
                Term::Str(hex32(effect_hash)),
            );
            let projected = Term::Map(mm);
            let h = value_hash(&Value::Data(projected.clone()));
            Ok((Stage2ValueKind::Term, projected, h))
        }
        other => Err(Stage2CompileError::Unsupported(format!(
            "unsupported result for stage2: {}",
            other.debug_repr()
        ))),
    }
}

pub(super) fn eval_wasm_scalar(
    wasm: &[u8],
    kind: Stage2ValueKind,
    symbol_table: &[String],
    string_table: &[String],
    bytes_table: &[Vec<u8>],
    term_table: &[Term],
) -> Result<Term, Stage2CompileError> {
    let engine = Engine::default();
    let module = WasmiModule::new(&engine, wasm)
        .map_err(|e| Stage2CompileError::Internal(format!("wasmi module decode: {e}")))?;
    let mut store = Store::new(&engine, ());
    let linker: Linker<()> = Linker::new(&engine);
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| Stage2CompileError::Internal(format!("wasmi instantiate/start: {e}")))?;
    let func = instance.get_func(&mut store, "eval").ok_or_else(|| {
        Stage2CompileError::Internal("missing exported eval function".to_string())
    })?;

    let mut results = [match kind {
        Stage2ValueKind::Int => Val::I64(0),
        Stage2ValueKind::Bool => Val::I32(0),
        Stage2ValueKind::Nil => Val::I32(0),
        Stage2ValueKind::Sym => Val::I32(0),
        Stage2ValueKind::Str => Val::I32(0),
        Stage2ValueKind::Bytes => Val::I32(0),
        Stage2ValueKind::Term => Val::I32(0),
    }];
    func.call(&mut store, &[], &mut results)
        .map_err(|e| Stage2CompileError::Internal(format!("wasmi call eval: {e}")))?;

    match (kind, results[0].clone()) {
        (Stage2ValueKind::Int, Val::I64(v)) => Ok(Term::Int(v.into())),
        (Stage2ValueKind::Bool, Val::I32(v)) => Ok(Term::Bool(v != 0)),
        (Stage2ValueKind::Nil, Val::I32(_)) => Ok(Term::Nil),
        (Stage2ValueKind::Sym, Val::I32(v)) => {
            let idx = usize::try_from(v).map_err(|_| {
                Stage2CompileError::Internal("negative symbol id result".to_string())
            })?;
            let sym = symbol_table.get(idx).ok_or_else(|| {
                Stage2CompileError::Internal("symbol id result out of range".to_string())
            })?;
            Ok(Term::Symbol(sym.clone()))
        }
        (Stage2ValueKind::Str, Val::I32(v)) => {
            let idx = usize::try_from(v).map_err(|_| {
                Stage2CompileError::Internal("negative string id result".to_string())
            })?;
            let s = string_table.get(idx).ok_or_else(|| {
                Stage2CompileError::Internal("string id result out of range".to_string())
            })?;
            Ok(Term::Str(s.clone()))
        }
        (Stage2ValueKind::Bytes, Val::I32(v)) => {
            let idx = usize::try_from(v).map_err(|_| {
                Stage2CompileError::Internal("negative bytes id result".to_string())
            })?;
            let bs = bytes_table.get(idx).ok_or_else(|| {
                Stage2CompileError::Internal("bytes id result out of range".to_string())
            })?;
            Ok(Term::Bytes(bs.clone().into()))
        }
        (Stage2ValueKind::Term, Val::I32(v)) => {
            let idx = usize::try_from(v)
                .map_err(|_| Stage2CompileError::Internal("negative term id result".to_string()))?;
            let term = term_table.get(idx).ok_or_else(|| {
                Stage2CompileError::Internal("term id result out of range".to_string())
            })?;
            Ok(term.clone())
        }
        (k, got) => Err(Stage2CompileError::Internal(format!(
            "unexpected wasm result type for {:?}: {:?}",
            k, got
        ))),
    }
}

pub(super) fn parse_statements(forms: &[Term]) -> Result<Vec<Stmt>, Stage2CompileError> {
    if forms.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "empty module is not supported by stage2".to_string(),
        ));
    }
    let mut out = Vec::with_capacity(forms.len());
    for t in forms {
        if let Some(xs) = t.as_proper_list()
            && xs.len() == 3
            && matches!(xs[0], Term::Symbol(s) if s == "def")
        {
            let name = match &xs[1] {
                Term::Symbol(s) => s.clone(),
                _ => {
                    return Err(Stage2CompileError::Unsupported(
                        "def name must be a symbol".to_string(),
                    ));
                }
            };
            out.push(Stmt::Def(name, xs[2].clone()));
            continue;
        }
        out.push(Stmt::Expr(t.clone()));
    }
    Ok(out)
}

pub(super) fn try_plan_defs_only_scalar(
    statements: &[Stmt],
) -> Result<Option<(Planner, Vec<PStmt>)>, Stage2CompileError> {
    let mut planner = Planner::default();
    let mut env: BTreeMap<String, Local> = BTreeMap::new();
    let mut fn_defs: BTreeMap<String, InlinableFnDef> = BTreeMap::new();
    let empty_local_fns: BTreeMap<String, InlinableFnDef> = BTreeMap::new();
    let mut planned = Vec::with_capacity(statements.len());

    for stmt in statements {
        let Stmt::Def(name, expr) = stmt else {
            return Err(Stage2CompileError::Internal(
                "defs-only planning received non-def statement".to_string(),
            ));
        };

        if let Some((param, body)) = desugar_fn_literal_to_unary(expr)? {
            env.remove(name);
            fn_defs.insert(
                name.clone(),
                InlinableFnDef {
                    param,
                    body,
                    capture: FnCapture::GlobalFrame,
                },
            );
            continue;
        }
        if let Term::Symbol(sym) = expr
            && let Some(alias_fn) = resolve_global_inlinable_symbol(sym, &fn_defs)
        {
            env.remove(name);
            fn_defs.insert(name.clone(), alias_fn);
            continue;
        }

        let pexpr = match plan_expr(expr, &env, &env, &fn_defs, &empty_local_fns, &mut planner) {
            Ok(v) => v,
            Err(Stage2CompileError::Unsupported(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let ty = pexpr.ty();
        let idx = planner.alloc_local(ty)?;
        record_local_const_ids(&mut planner, idx, &pexpr);
        fn_defs.remove(name);
        env.insert(name.clone(), Local { idx, ty });
        planned.push(PStmt::Def {
            name: name.clone(),
            idx,
            expr: pexpr,
        });
    }

    Ok(Some((planner, planned)))
}

pub(super) fn emit_wasm_module(
    planned: &[PStmt],
    locals: &[Ty],
    result_ty: Ty,
    append_nil: bool,
) -> Result<Vec<u8>, Stage2CompileError> {
    let locals_decl: Vec<(u32, ValType)> = locals.iter().map(|ty| (1u32, val_ty(*ty))).collect();
    let mut func = Function::new(locals_decl);
    let expr_count = planned
        .iter()
        .filter(|s| matches!(s, PStmt::Expr(_)))
        .count();
    let mut seen_expr = 0usize;
    for stmt in planned {
        match stmt {
            PStmt::Def { name, idx, expr } => {
                let got = emit_expr(&mut func, expr)?;
                if got != expr.ty() {
                    return Err(Stage2CompileError::Internal(format!(
                        "local type mismatch for {name}: expected {:?}, got {:?}",
                        expr.ty(),
                        got
                    )));
                }
                func.instruction(&Instruction::LocalSet(*idx));
            }
            PStmt::Expr(expr) => {
                seen_expr = seen_expr.saturating_add(1);
                let _ = emit_expr(&mut func, expr)?;
                if seen_expr < expr_count {
                    func.instruction(&Instruction::Drop);
                }
            }
        }
    }

    if append_nil {
        if result_ty != Ty::NilI32 {
            return Err(Stage2CompileError::Internal(
                "append_nil requires nil result type".to_string(),
            ));
        }
        func.instruction(&Instruction::I32Const(0));
    }
    func.instruction(&Instruction::End);

    let mut types = TypeSection::new();
    types.ty().function([], [val_ty(result_ty)]);

    let mut funcs = FunctionSection::new();
    funcs.function(0);

    let mut exports = ExportSection::new();
    exports.export("eval", ExportKind::Func, 0);

    let mut code = CodeSection::new();
    code.function(&func);

    let mut module = Module::new();
    module.section(&types);
    module.section(&funcs);
    module.section(&exports);
    module.section(&code);
    Ok(module.finish())
}

#[expect(
    clippy::type_complexity,
    reason = "constant fallback returns parallel tables for deterministic wasm replay envelopes"
)]
fn emit_wasm_constant_scalar(
    kind: Stage2ValueKind,
    term: &Term,
) -> Result<(Vec<u8>, Vec<String>, Vec<String>, Vec<Vec<u8>>, Vec<Term>), Stage2CompileError> {
    let mut symbol_table = Vec::new();
    let mut string_table = Vec::new();
    let mut bytes_table = Vec::new();
    let mut term_table = Vec::new();
    let mut func = Function::new(Vec::new());

    let result_val_ty = match (kind, term) {
        (Stage2ValueKind::Int, Term::Int(i)) => {
            let v = i.to_i64().ok_or_else(|| {
                Stage2CompileError::Unsupported(
                    "constant-fallback int result is out of i64 range".to_string(),
                )
            })?;
            func.instruction(&Instruction::I64Const(v));
            ValType::I64
        }
        (Stage2ValueKind::Bool, Term::Bool(v)) => {
            func.instruction(&Instruction::I32Const(if *v { 1 } else { 0 }));
            ValType::I32
        }
        (Stage2ValueKind::Nil, Term::Nil) => {
            func.instruction(&Instruction::I32Const(0));
            ValType::I32
        }
        (Stage2ValueKind::Sym, Term::Symbol(s)) => {
            symbol_table.push(s.clone());
            func.instruction(&Instruction::I32Const(0));
            ValType::I32
        }
        (Stage2ValueKind::Str, Term::Str(s)) => {
            string_table.push(s.clone());
            func.instruction(&Instruction::I32Const(0));
            ValType::I32
        }
        (Stage2ValueKind::Bytes, Term::Bytes(bs)) => {
            bytes_table.push(bs.to_vec());
            func.instruction(&Instruction::I32Const(0));
            ValType::I32
        }
        (Stage2ValueKind::Term, _) => {
            term_table.push(term.clone());
            func.instruction(&Instruction::I32Const(0));
            ValType::I32
        }
        (k, t) => {
            return Err(Stage2CompileError::Internal(format!(
                "constant-fallback kind/term mismatch: kind={k:?}, term={t:?}"
            )));
        }
    };
    func.instruction(&Instruction::End);

    let mut types = TypeSection::new();
    types.ty().function([], [result_val_ty]);

    let mut funcs = FunctionSection::new();
    funcs.function(0);

    let mut exports = ExportSection::new();
    exports.export("eval", ExportKind::Func, 0);

    let mut code = CodeSection::new();
    code.function(&func);

    let mut module = Module::new();
    module.section(&types);
    module.section(&funcs);
    module.section(&exports);
    module.section(&code);

    Ok((
        module.finish(),
        symbol_table,
        string_table,
        bytes_table,
        term_table,
    ))
}

fn stage2_compile_module_fallback_constant(
    forms: &[Term],
) -> Result<Stage2CompileArtifact, Stage2CompileError> {
    let module_hash = hash_module(forms);
    let (value_kind, term, _value_hash) = eval_original_data(forms)?;
    let (wasm_bytes, symbol_table, string_table, bytes_table, term_table) =
        emit_wasm_constant_scalar(value_kind, &term)?;
    let wasm_hash = *blake3::hash(&wasm_bytes).as_bytes();

    Ok(Stage2CompileArtifact {
        wasm_bytes,
        wasm_hash,
        module_hash,
        lowering_mode: Stage2LoweringMode::ConstantFallback,
        value_kind,
        symbol_table,
        string_table,
        bytes_table,
        term_table,
    })
}

pub(super) fn stage2_compile_module_pipeline(
    forms: &[Term],
) -> Result<Stage2CompileArtifact, Stage2CompileError> {
    match stage2_compile_module_pipeline_strict(forms) {
        Ok(artifact) => Ok(artifact),
        Err(Stage2CompileError::Internal(msg)) => Err(Stage2CompileError::Internal(msg)),
        Err(strict_unsupported @ Stage2CompileError::Unsupported(_)) => {
            stage2_compile_module_fallback_constant(forms).map_err(|fallback_err| {
                let strict_reason = strict_unsupported.to_string();
                match fallback_err {
                    Stage2CompileError::Unsupported(msg) => Stage2CompileError::Unsupported(
                        format!("{strict_reason}; constant-fallback failed: {msg}"),
                    ),
                    Stage2CompileError::Internal(msg) => Stage2CompileError::Internal(format!(
                        "{strict_reason}; constant-fallback failed: {msg}"
                    )),
                }
            })
        }
    }
}

fn stage2_compile_module_pipeline_strict(
    forms: &[Term],
) -> Result<Stage2CompileArtifact, Stage2CompileError> {
    let module_hash = hash_module(forms);
    let statements = parse_statements(forms)?;
    let has_user_expr = statements.iter().any(|s| matches!(s, Stmt::Expr(_)));

    if !has_user_expr {
        if let Some((planner, planned)) = try_plan_defs_only_scalar(&statements)? {
            let wasm_bytes = emit_wasm_module(&planned, &planner.locals, Ty::NilI32, true)?;
            let wasm_hash = *blake3::hash(&wasm_bytes).as_bytes();
            return Ok(Stage2CompileArtifact {
                wasm_bytes,
                wasm_hash,
                module_hash,
                lowering_mode: Stage2LoweringMode::Strict,
                value_kind: Stage2ValueKind::Nil,
                symbol_table: Vec::new(),
                string_table: Vec::new(),
                bytes_table: Vec::new(),
                term_table: Vec::new(),
            });
        }

        let mut vec_aliases: BTreeMap<String, Vec<Term>> = BTreeMap::new();
        let mut map_aliases: BTreeMap<String, BTreeMap<TermOrdKey, Term>> = BTreeMap::new();
        let mut scalar_aliases: BTreeMap<String, Term> = BTreeMap::new();
        let empty_vec_aliases: BTreeMap<String, Vec<Term>> = BTreeMap::new();
        let empty_map_aliases: BTreeMap<String, BTreeMap<TermOrdKey, Term>> = BTreeMap::new();
        for stmt in &statements {
            let Stmt::Def(name, rhs) = stmt else {
                continue;
            };
            let rhs_resolved = resolve_scalar_aliases_term(rhs, &scalar_aliases);
            if let Some(items) = term_const_vector_expr_with_aliases(
                &rhs_resolved,
                &vec_aliases,
                &empty_vec_aliases,
            )? {
                vec_aliases.insert(name.clone(), items);
                map_aliases.remove(name);
                scalar_aliases.remove(name);
                continue;
            }
            if let Some(items) =
                term_const_map_expr_with_aliases(&rhs_resolved, &map_aliases, &empty_map_aliases)?
            {
                map_aliases.insert(name.clone(), items);
                vec_aliases.remove(name);
                scalar_aliases.remove(name);
                continue;
            }
            if let Some(v) = term_const_if_condition_expr(&rhs_resolved)
                .or_else(|| term_const_data_expr(&rhs_resolved))
            {
                scalar_aliases.insert(name.clone(), v);
                vec_aliases.remove(name);
                map_aliases.remove(name);
                continue;
            }
            if !is_safe_defs_only_rhs(&rhs_resolved) {
                return Err(Stage2CompileError::Unsupported(format!(
                    "defs-only module contains non-trivial def rhs: {name}"
                )));
            }
            scalar_aliases.remove(name);
            vec_aliases.remove(name);
            map_aliases.remove(name);
        }

        let mut func = Function::new(Vec::new());
        // `nil` is represented as i32 sentinel 0 at the wasm boundary.
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::End);

        let mut types = TypeSection::new();
        types.ty().function([], [val_ty(Ty::NilI32)]);

        let mut funcs = FunctionSection::new();
        funcs.function(0);

        let mut exports = ExportSection::new();
        exports.export("eval", ExportKind::Func, 0);

        let mut code = CodeSection::new();
        code.function(&func);

        let mut module = Module::new();
        module.section(&types);
        module.section(&funcs);
        module.section(&exports);
        module.section(&code);
        let wasm_bytes = module.finish();
        let wasm_hash = *blake3::hash(&wasm_bytes).as_bytes();

        return Ok(Stage2CompileArtifact {
            wasm_bytes,
            wasm_hash,
            module_hash,
            lowering_mode: Stage2LoweringMode::Strict,
            value_kind: Stage2ValueKind::Nil,
            symbol_table: Vec::new(),
            string_table: Vec::new(),
            bytes_table: Vec::new(),
            term_table: Vec::new(),
        });
    }

    let mut planner = Planner::default();
    let mut env: BTreeMap<String, Local> = BTreeMap::new();
    let mut fn_defs: BTreeMap<String, InlinableFnDef> = BTreeMap::new();
    let empty_local_fns: BTreeMap<String, InlinableFnDef> = BTreeMap::new();
    let mut planned = Vec::with_capacity(statements.len());
    let mut last_expr_ty = None;

    for stmt in statements {
        match stmt {
            Stmt::Def(name, expr) => {
                if let Some((param, body)) = desugar_fn_literal_to_unary(&expr)? {
                    planner.global_const_vector_aliases.remove(&name);
                    planner.global_const_map_aliases.remove(&name);
                    env.remove(&name);
                    fn_defs.insert(
                        name,
                        InlinableFnDef {
                            param,
                            body,
                            capture: FnCapture::GlobalFrame,
                        },
                    );
                    continue;
                }
                if let Term::Symbol(sym) = &expr
                    && let Some(alias_fn) = resolve_global_inlinable_symbol(sym, &fn_defs)
                {
                    planner.global_const_vector_aliases.remove(&name);
                    planner.global_const_map_aliases.remove(&name);
                    env.remove(&name);
                    fn_defs.insert(name, alias_fn);
                    continue;
                }
                if let Term::Symbol(sym) = &expr
                    && !env.contains_key(sym)
                    && let Some(items) = planner.global_const_vector_aliases.get(sym).cloned()
                {
                    planner
                        .global_const_vector_aliases
                        .insert(name.clone(), items);
                    planner.global_const_map_aliases.remove(&name);
                    env.remove(&name);
                    fn_defs.remove(&name);
                    continue;
                }
                if let Term::Symbol(sym) = &expr
                    && !env.contains_key(sym)
                    && let Some(items) = planner.global_const_map_aliases.get(sym).cloned()
                {
                    planner.global_const_map_aliases.insert(name.clone(), items);
                    planner.global_const_vector_aliases.remove(&name);
                    env.remove(&name);
                    fn_defs.remove(&name);
                    continue;
                }
                let local_vec_aliases: BTreeMap<String, Vec<Term>> = BTreeMap::new();
                if let Some(items) = term_const_vector_expr_with_aliases(
                    &expr,
                    &local_vec_aliases,
                    &planner.global_const_vector_aliases,
                )? {
                    planner
                        .global_const_vector_aliases
                        .insert(name.clone(), items);
                    planner.global_const_map_aliases.remove(&name);
                    env.remove(&name);
                    fn_defs.remove(&name);
                    continue;
                }
                let local_map_aliases: BTreeMap<String, BTreeMap<TermOrdKey, Term>> =
                    BTreeMap::new();
                if let Some(items) = term_const_map_expr_with_aliases(
                    &expr,
                    &local_map_aliases,
                    &planner.global_const_map_aliases,
                )? {
                    planner.global_const_map_aliases.insert(name.clone(), items);
                    planner.global_const_vector_aliases.remove(&name);
                    env.remove(&name);
                    fn_defs.remove(&name);
                    continue;
                }

                let pexpr = plan_expr(&expr, &env, &env, &fn_defs, &empty_local_fns, &mut planner)?;
                let ty = pexpr.ty();
                let idx = planner.alloc_local(ty)?;
                record_local_const_ids(&mut planner, idx, &pexpr);
                planner.global_const_vector_aliases.remove(&name);
                planner.global_const_map_aliases.remove(&name);
                fn_defs.remove(&name);
                env.insert(name.clone(), Local { idx, ty });
                planned.push(PStmt::Def {
                    name,
                    idx,
                    expr: pexpr,
                });
            }
            Stmt::Expr(expr) => {
                let pexpr = plan_expr(&expr, &env, &env, &fn_defs, &empty_local_fns, &mut planner)?;
                last_expr_ty = Some(pexpr.ty());
                planned.push(PStmt::Expr(pexpr));
            }
        }
    }

    let result_ty = last_expr_ty.ok_or_else(|| {
        Stage2CompileError::Unsupported(
            "stage2 requires at least one top-level expression (non-def form)".to_string(),
        )
    })?;

    let value_kind = match result_ty {
        Ty::I64 => Stage2ValueKind::Int,
        Ty::BoolI32 => Stage2ValueKind::Bool,
        Ty::NilI32 => Stage2ValueKind::Nil,
        Ty::SymI32 => Stage2ValueKind::Sym,
        Ty::StrI32 => Stage2ValueKind::Str,
        Ty::BytesI32 => Stage2ValueKind::Bytes,
    };
    let symbol_table = planner_symbol_table(&planner)?;
    let string_table = planner_string_table(&planner)?;
    let bytes_table = planner_bytes_table(&planner)?;

    let locals_decl: Vec<(u32, ValType)> = planner
        .locals
        .iter()
        .map(|ty| (1u32, val_ty(*ty)))
        .collect();
    let mut func = Function::new(locals_decl);
    let expr_count = planned
        .iter()
        .filter(|s| matches!(s, PStmt::Expr(_)))
        .count();
    let mut seen_expr = 0usize;
    for stmt in &planned {
        match stmt {
            PStmt::Def { name, idx, expr } => {
                let got = emit_expr(&mut func, expr)?;
                if got != expr.ty() {
                    return Err(Stage2CompileError::Internal(format!(
                        "local type mismatch for {name}: expected {:?}, got {:?}",
                        expr.ty(),
                        got
                    )));
                }
                func.instruction(&Instruction::LocalSet(*idx));
            }
            PStmt::Expr(expr) => {
                seen_expr = seen_expr.saturating_add(1);
                let _ = emit_expr(&mut func, expr)?;
                if seen_expr < expr_count {
                    func.instruction(&Instruction::Drop);
                }
            }
        }
    }
    func.instruction(&Instruction::End);

    let mut types = TypeSection::new();
    types.ty().function([], [val_ty(result_ty)]);

    let mut funcs = FunctionSection::new();
    funcs.function(0);

    let mut exports = ExportSection::new();
    exports.export("eval", ExportKind::Func, 0);

    let mut code = CodeSection::new();
    code.function(&func);

    let mut module = Module::new();
    module.section(&types);
    module.section(&funcs);
    module.section(&exports);
    module.section(&code);
    let wasm_bytes = module.finish();
    let wasm_hash = *blake3::hash(&wasm_bytes).as_bytes();

    Ok(Stage2CompileArtifact {
        wasm_bytes,
        wasm_hash,
        module_hash,
        lowering_mode: Stage2LoweringMode::Strict,
        value_kind,
        symbol_table,
        string_table,
        bytes_table,
        term_table: Vec::new(),
    })
}

pub(super) fn stage2_validation_report_pipeline(forms: &[Term]) -> Stage2ValidationReport {
    let obligation = "core/obligation::translation-validation".to_string();
    let module_hash = hash_module(forms);
    let mut errors = Vec::new();

    let artifact = match stage2_compile_module_pipeline(forms) {
        Ok(a) => a,
        Err(e) => {
            let supported = !matches!(e, Stage2CompileError::Unsupported(_));
            errors.push(e.to_string());
            return Stage2ValidationReport {
                obligation,
                supported,
                ok: false,
                module_hash,
                lowering_mode: None,
                wasm_hash: None,
                value_kind: None,
                original_value_hash: None,
                wasm_value_hash: None,
                wasm_bytes_len: None,
                errors,
            };
        }
    };

    let (orig_kind, original_term, original_value_hash) = match eval_original_data(forms) {
        Ok(v) => v,
        Err(e) => {
            let supported = !matches!(e, Stage2CompileError::Unsupported(_));
            errors.push(e.to_string());
            return Stage2ValidationReport {
                obligation,
                supported,
                ok: false,
                module_hash,
                lowering_mode: Some(artifact.lowering_mode),
                wasm_hash: Some(artifact.wasm_hash),
                value_kind: None,
                original_value_hash: None,
                wasm_value_hash: None,
                wasm_bytes_len: Some(artifact.wasm_bytes.len()),
                errors,
            };
        }
    };

    let wasm_term = match eval_wasm_scalar(
        &artifact.wasm_bytes,
        artifact.value_kind,
        &artifact.symbol_table,
        &artifact.string_table,
        &artifact.bytes_table,
        &artifact.term_table,
    ) {
        Ok(t) => t,
        Err(e) => {
            errors.push(e.to_string());
            return Stage2ValidationReport {
                obligation,
                supported: true,
                ok: false,
                module_hash,
                lowering_mode: Some(artifact.lowering_mode),
                wasm_hash: Some(artifact.wasm_hash),
                value_kind: Some(orig_kind),
                original_value_hash: Some(original_value_hash),
                wasm_value_hash: None,
                wasm_bytes_len: Some(artifact.wasm_bytes.len()),
                errors,
            };
        }
    };
    let wasm_value_hash = value_hash(&Value::Data(wasm_term.clone()));

    if original_term != wasm_term {
        errors.push("stage2 wasm result differs from kernel result".to_string());
    }
    if original_value_hash != wasm_value_hash {
        errors.push("stage2 wasm value hash mismatch".to_string());
    }

    Stage2ValidationReport {
        obligation,
        supported: true,
        ok: errors.is_empty(),
        module_hash,
        lowering_mode: Some(artifact.lowering_mode),
        wasm_hash: Some(artifact.wasm_hash),
        value_kind: Some(orig_kind),
        original_value_hash: Some(original_value_hash),
        wasm_value_hash: Some(wasm_value_hash),
        wasm_bytes_len: Some(artifact.wasm_bytes.len()),
        errors,
    }
}
