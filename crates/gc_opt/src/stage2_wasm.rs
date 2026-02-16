use std::collections::BTreeMap;

use gc_coreform::{Term, hash_module};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;
use num_traits::ToPrimitive;
use thiserror::Error;
use wasm_encoder::{
    BlockType, CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction,
    Module, TypeSection, ValType,
};
use wasmi::{Engine, Linker, Module as WasmiModule, Store, Val};

const STAGE2_BASELINE_STEP_LIMIT: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage2ValueKind {
    Int,
    Bool,
    Nil,
}

#[derive(Debug, Clone)]
pub struct Stage2CompileArtifact {
    pub wasm_bytes: Vec<u8>,
    pub wasm_hash: [u8; 32],
    pub module_hash: [u8; 32],
    pub value_kind: Stage2ValueKind,
}

#[derive(Debug, Error, Clone)]
pub enum Stage2CompileError {
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, Clone)]
pub struct Stage2ValidationReport {
    pub obligation: String,
    pub supported: bool,
    pub ok: bool,
    pub module_hash: [u8; 32],
    pub wasm_hash: Option<[u8; 32]>,
    pub value_kind: Option<Stage2ValueKind>,
    pub original_value_hash: Option<[u8; 32]>,
    pub wasm_value_hash: Option<[u8; 32]>,
    pub wasm_bytes_len: Option<usize>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
enum Stmt {
    Def(String, Term),
    Expr(Term),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    I64,
    BoolI32,
    NilI32,
}

#[derive(Debug, Clone, Copy)]
struct Local {
    idx: u32,
    ty: Ty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimOp {
    Add,
    Sub,
    Mul,
    EqI64,
    EqI32,
    EqAlwaysFalse,
    Lt,
}

#[derive(Debug, Clone)]
struct InlinableFnDef {
    param: String,
    body: Term,
    capture: FnCapture,
}

#[derive(Debug, Clone)]
enum FnCapture {
    GlobalFrame,
    Lexical(BTreeMap<String, Local>),
}

#[derive(Debug, Clone)]
struct LetBinding {
    idx: u32,
    expr: PExpr,
}

#[derive(Debug, Clone)]
struct CallableHead {
    param: String,
    body: Term,
    base_env: BTreeMap<String, Local>,
    def_name: Option<String>,
}

#[derive(Debug, Clone)]
enum PExpr {
    Nil,
    Int(i64),
    Bool(bool),
    Local(Local),
    Prim {
        op: PrimOp,
        lhs: Box<PExpr>,
        rhs: Box<PExpr>,
        ty: Ty,
    },
    If {
        cond: Box<PExpr>,
        then_expr: Box<PExpr>,
        else_expr: Box<PExpr>,
        cond_ty: Ty,
        ty: Ty,
    },
    Begin {
        exprs: Vec<PExpr>,
        ty: Ty,
    },
    Let {
        bindings: Vec<LetBinding>,
        body: Vec<PExpr>,
        ty: Ty,
    },
}

impl PExpr {
    fn ty(&self) -> Ty {
        match self {
            PExpr::Nil => Ty::NilI32,
            PExpr::Int(_) => Ty::I64,
            PExpr::Bool(_) => Ty::BoolI32,
            PExpr::Local(l) => l.ty,
            PExpr::Prim { ty, .. } => *ty,
            PExpr::If { ty, .. } => *ty,
            PExpr::Begin { ty, .. } => *ty,
            PExpr::Let { ty, .. } => *ty,
        }
    }
}

#[derive(Debug, Clone)]
enum PStmt {
    Def { name: String, idx: u32, expr: PExpr },
    Expr(PExpr),
}

#[derive(Debug, Default)]
struct Planner {
    locals: Vec<Ty>,
    expanding_fn_defs: Vec<String>,
}

impl Planner {
    fn alloc_local(&mut self, ty: Ty) -> Result<u32, Stage2CompileError> {
        let idx = u32::try_from(self.locals.len())
            .map_err(|_| Stage2CompileError::Internal("too many wasm locals".to_string()))?;
        self.locals.push(ty);
        Ok(idx)
    }
}

pub fn stage2_compile_module(forms: &[Term]) -> Result<Stage2CompileArtifact, Stage2CompileError> {
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
                value_kind: Stage2ValueKind::Nil,
            });
        }

        for stmt in &statements {
            let Stmt::Def(name, rhs) = stmt else {
                continue;
            };
            if !is_safe_defs_only_rhs(rhs) {
                return Err(Stage2CompileError::Unsupported(format!(
                    "defs-only module contains non-trivial def rhs: {name}"
                )));
            }
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
            value_kind: Stage2ValueKind::Nil,
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
                    env.remove(&name);
                    fn_defs.insert(name, alias_fn);
                    continue;
                }

                let pexpr = plan_expr(&expr, &env, &env, &fn_defs, &empty_local_fns, &mut planner)?;
                let ty = pexpr.ty();
                let idx = planner.alloc_local(ty)?;
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
    };

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
        value_kind,
    })
}

pub fn stage2_validation_report(forms: &[Term]) -> Stage2ValidationReport {
    let obligation = "core/obligation::translation-validation".to_string();
    let module_hash = hash_module(forms);
    let mut errors = Vec::new();

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
                wasm_hash: None,
                value_kind: None,
                original_value_hash: None,
                wasm_value_hash: None,
                wasm_bytes_len: None,
                errors,
            };
        }
    };

    let artifact = match stage2_compile_module(forms) {
        Ok(a) => a,
        Err(e) => {
            let supported = !matches!(e, Stage2CompileError::Unsupported(_));
            errors.push(e.to_string());
            return Stage2ValidationReport {
                obligation,
                supported,
                ok: false,
                module_hash,
                wasm_hash: None,
                value_kind: Some(orig_kind),
                original_value_hash: Some(original_value_hash),
                wasm_value_hash: None,
                wasm_bytes_len: None,
                errors,
            };
        }
    };

    let wasm_term = match eval_wasm_scalar(&artifact.wasm_bytes, artifact.value_kind) {
        Ok(t) => t,
        Err(e) => {
            errors.push(e.to_string());
            return Stage2ValidationReport {
                obligation,
                supported: true,
                ok: false,
                module_hash,
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
        wasm_hash: Some(artifact.wasm_hash),
        value_kind: Some(orig_kind),
        original_value_hash: Some(original_value_hash),
        wasm_value_hash: Some(wasm_value_hash),
        wasm_bytes_len: Some(artifact.wasm_bytes.len()),
        errors,
    }
}

fn eval_original_data(
    forms: &[Term],
) -> Result<(Stage2ValueKind, Term, [u8; 32]), Stage2CompileError> {
    let mut ctx = EvalCtx::with_step_limit(Some(STAGE2_BASELINE_STEP_LIMIT));
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, forms)
        .map_err(|e| Stage2CompileError::Unsupported(format!("kernel eval failed: {e}")))?;
    match v {
        Value::Data(Term::Int(i)) => {
            let term = Term::Int(i);
            let h = value_hash(&Value::Data(term.clone()));
            Ok((Stage2ValueKind::Int, term, h))
        }
        Value::Data(Term::Bool(b)) => {
            let term = Term::Bool(b);
            let h = value_hash(&Value::Data(term.clone()));
            Ok((Stage2ValueKind::Bool, term, h))
        }
        Value::Data(Term::Nil) => {
            let term = Term::Nil;
            let h = value_hash(&Value::Data(term.clone()));
            Ok((Stage2ValueKind::Nil, term, h))
        }
        Value::EffectProgram(_) => Err(Stage2CompileError::Unsupported(
            "effect program produced (stage2 supports pure scalar results only)".to_string(),
        )),
        other => Err(Stage2CompileError::Unsupported(format!(
            "unsupported result for stage2: {}",
            other.debug_repr()
        ))),
    }
}

fn eval_wasm_scalar(wasm: &[u8], kind: Stage2ValueKind) -> Result<Term, Stage2CompileError> {
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
    }];
    func.call(&mut store, &[], &mut results)
        .map_err(|e| Stage2CompileError::Internal(format!("wasmi call eval: {e}")))?;

    match (kind, results[0].clone()) {
        (Stage2ValueKind::Int, Val::I64(v)) => Ok(Term::Int(v.into())),
        (Stage2ValueKind::Bool, Val::I32(v)) => Ok(Term::Bool(v != 0)),
        (Stage2ValueKind::Nil, Val::I32(_)) => Ok(Term::Nil),
        (k, got) => Err(Stage2CompileError::Internal(format!(
            "unexpected wasm result type for {:?}: {:?}",
            k, got
        ))),
    }
}

fn parse_statements(forms: &[Term]) -> Result<Vec<Stmt>, Stage2CompileError> {
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

fn try_plan_defs_only_scalar(
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

fn emit_wasm_module(
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

fn plan_expr(
    t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    match t {
        Term::Nil => Ok(PExpr::Nil),
        Term::Int(i) => {
            let n = i.to_i64().ok_or_else(|| {
                Stage2CompileError::Unsupported(
                    "int literal out of i64 range for stage2".to_string(),
                )
            })?;
            Ok(PExpr::Int(n))
        }
        Term::Bool(b) => Ok(PExpr::Bool(*b)),
        Term::Symbol(s) => env.get(s).copied().map(PExpr::Local).ok_or_else(|| {
            Stage2CompileError::Unsupported(format!("unknown symbol in stage2: {s}"))
        }),
        _ => plan_list_expr(t, env, global_env, fn_defs, local_fn_defs, planner),
    }
}

fn plan_list_expr(
    t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let xs = t.as_proper_list().ok_or_else(|| {
        Stage2CompileError::Unsupported(
            "improper list expression is unsupported in stage2".to_string(),
        )
    })?;
    if xs.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "empty list expression is unsupported in stage2".to_string(),
        ));
    }
    if matches!(xs[0], Term::Symbol(s) if s == "if") {
        if xs.len() != 4 {
            return Err(Stage2CompileError::Unsupported(
                "if must have exactly 3 arguments".to_string(),
            ));
        }
        let cond = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        let cond_ty = cond.ty();
        if !matches!(cond_ty, Ty::BoolI32 | Ty::NilI32 | Ty::I64) {
            return Err(Stage2CompileError::Unsupported(
                "if condition must be scalar (bool, nil, or int)".to_string(),
            ));
        }
        let then_expr = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        let else_expr = plan_expr(xs[3], env, global_env, fn_defs, local_fn_defs, planner)?;
        if then_expr.ty() != else_expr.ty() {
            return Err(Stage2CompileError::Unsupported(
                "if branches must have matching types".to_string(),
            ));
        }
        let ty = then_expr.ty();
        return Ok(PExpr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
            cond_ty,
            ty,
        });
    }
    if matches!(xs[0], Term::Symbol(s) if s == "begin") {
        if xs.len() < 2 {
            return Err(Stage2CompileError::Unsupported(
                "begin must have at least one expression".to_string(),
            ));
        }
        let mut exprs = Vec::with_capacity(xs.len() - 1);
        for x in xs.iter().skip(1) {
            exprs.push(plan_expr(
                x,
                env,
                global_env,
                fn_defs,
                local_fn_defs,
                planner,
            )?);
        }
        let ty = exprs
            .last()
            .map(PExpr::ty)
            .ok_or_else(|| Stage2CompileError::Internal("begin planning failed".to_string()))?;
        return Ok(PExpr::Begin { exprs, ty });
    }
    if matches!(xs[0], Term::Symbol(s) if s == "quote") {
        if xs.len() != 2 {
            return Err(Stage2CompileError::Unsupported(
                "quote must have exactly 1 argument".to_string(),
            ));
        }
        return match xs[1] {
            Term::Nil => Ok(PExpr::Nil),
            Term::Bool(b) => Ok(PExpr::Bool(*b)),
            Term::Int(i) => {
                let n = i.to_i64().ok_or_else(|| {
                    Stage2CompileError::Unsupported(
                        "quoted int literal out of i64 range for stage2".to_string(),
                    )
                })?;
                Ok(PExpr::Int(n))
            }
            _ => Err(Stage2CompileError::Unsupported(
                "quote is stage2-supported only for scalar nil/bool/int".to_string(),
            )),
        };
    }
    if matches!(xs[0], Term::Symbol(s) if s == "let") {
        if xs.len() < 3 {
            return Err(Stage2CompileError::Unsupported(
                "(let ((x e) ...) body...) expects bindings and body".to_string(),
            ));
        }
        let Some(bs) = xs[1].as_proper_list() else {
            return Err(Stage2CompileError::Unsupported(
                "(let ...) bindings must be a list".to_string(),
            ));
        };
        let mut env2 = env.clone();
        let mut local_fn_defs2 = local_fn_defs.clone();
        let mut bindings = Vec::with_capacity(bs.len());
        for b in bs {
            let Some(pair) = b.as_proper_list() else {
                return Err(Stage2CompileError::Unsupported(
                    "(let ...) binding must be a list (name expr)".to_string(),
                ));
            };
            if pair.len() != 2 {
                return Err(Stage2CompileError::Unsupported(
                    "(let ...) binding must have exactly 2 forms".to_string(),
                ));
            }
            let Term::Symbol(name) = pair[0] else {
                return Err(Stage2CompileError::Unsupported(
                    "(let ...) binding name must be symbol".to_string(),
                ));
            };
            if let Some((param, body)) = desugar_fn_literal_to_unary(pair[1])? {
                env2.remove(name);
                local_fn_defs2.insert(
                    name.clone(),
                    InlinableFnDef {
                        param,
                        body,
                        capture: FnCapture::Lexical(env2.clone()),
                    },
                );
                continue;
            }
            if let Term::Symbol(sym) = pair[1]
                && !env2.contains_key(sym)
                && let Some(alias_fn) = resolve_inlinable_symbol(sym, fn_defs, &local_fn_defs2)
            {
                env2.remove(name);
                local_fn_defs2.insert(name.clone(), alias_fn);
                continue;
            }

            let rhs = plan_expr(
                pair[1],
                &env2,
                global_env,
                fn_defs,
                &local_fn_defs2,
                planner,
            )?;
            let idx = planner.alloc_local(rhs.ty())?;
            env2.insert(name.clone(), Local { idx, ty: rhs.ty() });
            local_fn_defs2.remove(name);
            bindings.push(LetBinding { idx, expr: rhs });
        }
        let mut body = Vec::with_capacity(xs.len() - 2);
        for x in xs.iter().skip(2) {
            body.push(plan_expr(
                x,
                &env2,
                global_env,
                fn_defs,
                &local_fn_defs2,
                planner,
            )?);
        }
        let ty = body
            .last()
            .map(PExpr::ty)
            .ok_or_else(|| Stage2CompileError::Internal("let planning failed".to_string()))?;
        return Ok(PExpr::Let { bindings, body, ty });
    }
    if xs.len() == 4
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
    {
        let lhs = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        let rhs = plan_expr(xs[3], env, global_env, fn_defs, local_fn_defs, planner)?;
        let (prim_op, ty) = infer_prim(op, lhs.ty(), rhs.ty())?;
        return Ok(PExpr::Prim {
            op: prim_op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            ty,
        });
    }
    if let Some((op_sym, lhs_t, rhs_t)) = match_curried_wrapper_call(&xs) {
        let lhs = plan_expr(&lhs_t, env, global_env, fn_defs, local_fn_defs, planner)?;
        let rhs = plan_expr(&rhs_t, env, global_env, fn_defs, local_fn_defs, planner)?;
        let (op, ty) = infer_prim(op_sym, lhs.ty(), rhs.ty())?;
        return Ok(PExpr::Prim {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            ty,
        });
    }
    if let Some(call_chain) =
        try_plan_application_chain(t, env, global_env, fn_defs, local_fn_defs, planner)?
    {
        return Ok(call_chain);
    }
    Err(Stage2CompileError::Unsupported(
        "unsupported expression form in stage2".to_string(),
    ))
}

fn try_plan_application_chain(
    t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<Option<PExpr>, Stage2CompileError> {
    let Some((head, args)) = flatten_application_chain(t) else {
        return Ok(None);
    };
    if args.is_empty() {
        return Ok(None);
    }

    let Some(mut callable) = resolve_callable_head(&head, env, global_env, fn_defs, local_fn_defs)?
    else {
        return Ok(None);
    };
    let mut pushed_name = None;
    if let Some(name) = callable.def_name.as_ref() {
        if planner.expanding_fn_defs.iter().any(|n| n == name) {
            return Err(Stage2CompileError::Unsupported(format!(
                "recursive function call is unsupported in stage2: {name}"
            )));
        }
        planner.expanding_fn_defs.push(name.clone());
        pushed_name = Some(name.clone());
    }

    let mut bindings: Vec<LetBinding> = Vec::with_capacity(args.len());
    let mut result_expr = None;

    for (i, arg) in args.iter().enumerate() {
        let arg_expr = match plan_expr(arg, env, global_env, fn_defs, local_fn_defs, planner) {
            Ok(v) => v,
            Err(e) => {
                if pushed_name.is_some() {
                    planner.expanding_fn_defs.pop();
                }
                return Err(e);
            }
        };
        let idx = planner.alloc_local(arg_expr.ty())?;
        let mut call_env = callable.base_env.clone();
        call_env.insert(
            callable.param.clone(),
            Local {
                idx,
                ty: arg_expr.ty(),
            },
        );
        bindings.push(LetBinding {
            idx,
            expr: arg_expr,
        });

        let is_last = i + 1 == args.len();
        if is_last {
            result_expr = Some(
                match plan_expr(
                    &callable.body,
                    &call_env,
                    global_env,
                    fn_defs,
                    local_fn_defs,
                    planner,
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        if pushed_name.is_some() {
                            planner.expanding_fn_defs.pop();
                        }
                        return Err(e);
                    }
                },
            );
            break;
        }

        let Some((next_param, next_body)) = desugar_fn_literal_to_unary(&callable.body)? else {
            if pushed_name.is_some() {
                planner.expanding_fn_defs.pop();
            }
            return Err(Stage2CompileError::Unsupported(
                "application chain expects function result at each intermediate step".to_string(),
            ));
        };
        callable = CallableHead {
            param: next_param,
            body: next_body,
            base_env: call_env,
            def_name: None,
        };
    }

    if pushed_name.is_some() {
        planner.expanding_fn_defs.pop();
    }

    let mut out = result_expr.ok_or_else(|| {
        Stage2CompileError::Internal("application chain planning produced no result".to_string())
    })?;
    for binding in bindings.into_iter().rev() {
        let ty = out.ty();
        out = PExpr::Let {
            bindings: vec![binding],
            body: vec![out],
            ty,
        };
    }
    Ok(Some(out))
}

fn flatten_application_chain(t: &Term) -> Option<(Term, Vec<Term>)> {
    let mut args_rev = Vec::new();
    let mut cur = t;
    loop {
        let Some(xs) = cur.as_proper_list() else {
            break;
        };
        if xs.len() != 2 {
            break;
        }
        args_rev.push(xs[1].clone());
        cur = xs[0];
    }
    if args_rev.is_empty() {
        return None;
    }
    args_rev.reverse();
    Some((cur.clone(), args_rev))
}

fn resolve_callable_head(
    head: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
) -> Result<Option<CallableHead>, Stage2CompileError> {
    if let Some((param, body)) = desugar_fn_literal_to_unary(head)? {
        return Ok(Some(CallableHead {
            param,
            body,
            base_env: env.clone(),
            def_name: None,
        }));
    }
    if let Term::Symbol(name) = head
        && !env.contains_key(name)
        && let Some(fn_def) = resolve_inlinable_symbol(name, fn_defs, local_fn_defs)
    {
        let base_env = match &fn_def.capture {
            FnCapture::GlobalFrame => global_env.clone(),
            FnCapture::Lexical(captured) => captured.clone(),
        };
        return Ok(Some(CallableHead {
            param: fn_def.param.clone(),
            body: fn_def.body.clone(),
            base_env,
            def_name: Some(name.clone()),
        }));
    }
    Ok(None)
}

fn resolve_inlinable_symbol(
    sym: &str,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
) -> Option<InlinableFnDef> {
    if let Some(existing) = local_fn_defs.get(sym) {
        return Some(existing.clone());
    }
    resolve_global_inlinable_symbol(sym, fn_defs)
}

fn resolve_global_inlinable_symbol(
    sym: &str,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
) -> Option<InlinableFnDef> {
    if let Some(existing) = fn_defs.get(sym) {
        return Some(existing.clone());
    }
    builtin_inlinable_fn(sym)
}

fn builtin_inlinable_fn(sym: &str) -> Option<InlinableFnDef> {
    let prim = match sym {
        "core/int::add" => "int/add",
        "core/int::sub" => "int/sub",
        "core/int::mul" => "int/mul",
        "core/int::eq?" => "int/eq?",
        "core/int::lt?" => "int/lt?",
        "core/eq?" => "core/eq?",
        _ => return None,
    };
    let body = Term::list(vec![
        Term::Symbol("fn".to_string()),
        Term::list(vec![Term::Symbol("b".to_string())]),
        Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol(prim.to_string()),
            Term::Symbol("a".to_string()),
            Term::Symbol("b".to_string()),
        ]),
    ]);
    Some(InlinableFnDef {
        param: "a".to_string(),
        body,
        capture: FnCapture::GlobalFrame,
    })
}

fn desugar_fn_literal_to_unary(t: &Term) -> Result<Option<(String, Term)>, Stage2CompileError> {
    let Some(items) = t.as_proper_list() else {
        return Ok(None);
    };
    if items.len() < 3 || !matches!(items[0], Term::Symbol(s) if s == "fn") {
        return Ok(None);
    }
    let params = items[1].as_proper_list().ok_or_else(|| {
        Stage2CompileError::Unsupported("(fn ...) params must be a list".to_string())
    })?;
    if params.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "(fn ...) requires at least 1 parameter".to_string(),
        ));
    }

    let mut body: Term = if items.len() == 3 {
        items[2].clone()
    } else {
        let mut xs = Vec::with_capacity(items.len() - 1);
        xs.push(Term::Symbol("begin".to_string()));
        for item in items.iter().skip(2) {
            xs.push((*item).clone());
        }
        Term::list(xs)
    };

    for p in params.iter().skip(1).rev() {
        let Term::Symbol(sym) = p else {
            return Err(Stage2CompileError::Unsupported(
                "(fn ...) params must be symbols".to_string(),
            ));
        };
        body = Term::list(vec![
            Term::Symbol("fn".to_string()),
            Term::list(vec![Term::Symbol(sym.clone())]),
            body,
        ]);
    }
    let Term::Symbol(param0) = params[0] else {
        return Err(Stage2CompileError::Unsupported(
            "(fn ...) params must be symbols".to_string(),
        ));
    };
    Ok(Some((param0.clone(), body)))
}

fn infer_prim(op: &str, a: Ty, b: Ty) -> Result<(PrimOp, Ty), Stage2CompileError> {
    match op {
        "int/add" | "int/sub" | "int/mul" => {
            if a == Ty::I64 && b == Ty::I64 {
                let prim = match op {
                    "int/add" => PrimOp::Add,
                    "int/sub" => PrimOp::Sub,
                    "int/mul" => PrimOp::Mul,
                    _ => unreachable!(),
                };
                Ok((prim, Ty::I64))
            } else {
                Err(Stage2CompileError::Unsupported(format!(
                    "{op} expects int arguments"
                )))
            }
        }
        "int/eq?" | "int/lt?" => {
            if a == Ty::I64 && b == Ty::I64 {
                let prim = match op {
                    "int/eq?" => PrimOp::EqI64,
                    "int/lt?" => PrimOp::Lt,
                    _ => unreachable!(),
                };
                Ok((prim, Ty::BoolI32))
            } else {
                Err(Stage2CompileError::Unsupported(format!(
                    "{op} expects int arguments"
                )))
            }
        }
        "core/eq?" => {
            match (a, b) {
                (Ty::I64, Ty::I64) => Ok((PrimOp::EqI64, Ty::BoolI32)),
                (Ty::BoolI32, Ty::BoolI32) | (Ty::NilI32, Ty::NilI32) => {
                    Ok((PrimOp::EqI32, Ty::BoolI32))
                }
                // Kernel structural equality across mixed scalar kinds is always false,
                // while still evaluating both operands first.
                _ => Ok((PrimOp::EqAlwaysFalse, Ty::BoolI32)),
            }
        }
        _ => Err(Stage2CompileError::Unsupported(format!(
            "prim {op} is unsupported in stage2"
        ))),
    }
}

fn match_curried_wrapper_call(xs: &[&Term]) -> Option<(&'static str, Term, Term)> {
    if xs.len() != 2 {
        return None;
    }
    let inner = xs[0].as_proper_list()?;
    if inner.len() != 2 {
        return None;
    }
    let op = match inner[0] {
        Term::Symbol(s) => match s.as_str() {
            "core/int::add" => "int/add",
            "core/int::sub" => "int/sub",
            "core/int::mul" => "int/mul",
            "core/int::eq?" => "int/eq?",
            "core/int::lt?" => "int/lt?",
            "core/eq?" => "core/eq?",
            _ => return None,
        },
        _ => return None,
    };
    Some((op, inner[1].clone(), xs[1].clone()))
}

fn emit_expr(f: &mut Function, expr: &PExpr) -> Result<Ty, Stage2CompileError> {
    match expr {
        PExpr::Nil => {
            f.instruction(&Instruction::I32Const(0));
            Ok(Ty::NilI32)
        }
        PExpr::Int(n) => {
            f.instruction(&Instruction::I64Const(*n));
            Ok(Ty::I64)
        }
        PExpr::Bool(b) => {
            f.instruction(&Instruction::I32Const(if *b { 1 } else { 0 }));
            Ok(Ty::BoolI32)
        }
        PExpr::Local(local) => {
            f.instruction(&Instruction::LocalGet(local.idx));
            Ok(local.ty)
        }
        PExpr::Prim { op, lhs, rhs, ty } => {
            let l = emit_expr(f, lhs)?;
            let r = emit_expr(f, rhs)?;
            if *op == PrimOp::EqAlwaysFalse {
                f.instruction(&Instruction::Drop);
                f.instruction(&Instruction::Drop);
                f.instruction(&Instruction::I32Const(0));
                if *ty != Ty::BoolI32 {
                    return Err(Stage2CompileError::Internal(
                        "planned core/eq? mixed-type result mismatch".to_string(),
                    ));
                }
                return Ok(Ty::BoolI32);
            }
            let (expected_op, expected_ty) = match (op, l, r) {
                (PrimOp::Add, Ty::I64, Ty::I64) => (Instruction::I64Add, Ty::I64),
                (PrimOp::Sub, Ty::I64, Ty::I64) => (Instruction::I64Sub, Ty::I64),
                (PrimOp::Mul, Ty::I64, Ty::I64) => (Instruction::I64Mul, Ty::I64),
                (PrimOp::EqI64, Ty::I64, Ty::I64) => (Instruction::I64Eq, Ty::BoolI32),
                (PrimOp::EqI32, Ty::BoolI32, Ty::BoolI32) => (Instruction::I32Eq, Ty::BoolI32),
                (PrimOp::EqI32, Ty::NilI32, Ty::NilI32) => (Instruction::I32Eq, Ty::BoolI32),
                (PrimOp::Lt, Ty::I64, Ty::I64) => (Instruction::I64LtS, Ty::BoolI32),
                _ => {
                    return Err(Stage2CompileError::Internal(
                        "planned prim has invalid operand types".to_string(),
                    ));
                }
            };
            f.instruction(&expected_op);
            if *ty != expected_ty {
                return Err(Stage2CompileError::Internal(
                    "planned prim result type mismatch".to_string(),
                ));
            }
            Ok(*ty)
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty,
        } => {
            let c = emit_expr(f, cond)?;
            if c != *cond_ty {
                return Err(Stage2CompileError::Internal(
                    "planned if condition type mismatch".to_string(),
                ));
            }
            // Kernel truthiness for scalar values:
            // bool(false) and nil are false; ints are always truthy.
            match cond_ty {
                Ty::BoolI32 => {}
                Ty::NilI32 => {
                    f.instruction(&Instruction::Drop);
                    f.instruction(&Instruction::I32Const(0));
                }
                Ty::I64 => {
                    f.instruction(&Instruction::Drop);
                    f.instruction(&Instruction::I32Const(1));
                }
            }
            f.instruction(&Instruction::If(BlockType::Result(val_ty(*ty))));
            let t_got = emit_expr(f, then_expr)?;
            if t_got != *ty {
                return Err(Stage2CompileError::Internal(
                    "planned if then branch type mismatch".to_string(),
                ));
            }
            f.instruction(&Instruction::Else);
            let e_got = emit_expr(f, else_expr)?;
            if e_got != *ty {
                return Err(Stage2CompileError::Internal(
                    "planned if else branch type mismatch".to_string(),
                ));
            }
            f.instruction(&Instruction::End);
            Ok(*ty)
        }
        PExpr::Begin { exprs, ty } => {
            if exprs.is_empty() {
                return Err(Stage2CompileError::Internal(
                    "planned begin has no expressions".to_string(),
                ));
            }
            let mut last = None;
            for (i, e) in exprs.iter().enumerate() {
                let got = emit_expr(f, e)?;
                if i + 1 < exprs.len() {
                    f.instruction(&Instruction::Drop);
                }
                last = Some(got);
            }
            let got = last.ok_or_else(|| {
                Stage2CompileError::Internal("planned begin emission failed".to_string())
            })?;
            if got != *ty {
                return Err(Stage2CompileError::Internal(
                    "planned begin result type mismatch".to_string(),
                ));
            }
            Ok(*ty)
        }
        PExpr::Let { bindings, body, ty } => {
            for b in bindings {
                let rhs_ty = emit_expr(f, &b.expr)?;
                if rhs_ty != b.expr.ty() {
                    return Err(Stage2CompileError::Internal(
                        "planned let binding type mismatch".to_string(),
                    ));
                }
                f.instruction(&Instruction::LocalSet(b.idx));
            }
            if body.is_empty() {
                return Err(Stage2CompileError::Internal(
                    "planned let has empty body".to_string(),
                ));
            }
            let mut last = None;
            for (i, e) in body.iter().enumerate() {
                let got = emit_expr(f, e)?;
                if i + 1 < body.len() {
                    f.instruction(&Instruction::Drop);
                }
                last = Some(got);
            }
            let got = last.ok_or_else(|| {
                Stage2CompileError::Internal("planned let emission failed".to_string())
            })?;
            if got != *ty {
                return Err(Stage2CompileError::Internal(
                    "planned let result type mismatch".to_string(),
                ));
            }
            Ok(*ty)
        }
    }
}

fn val_ty(t: Ty) -> ValType {
    match t {
        Ty::I64 => ValType::I64,
        Ty::BoolI32 => ValType::I32,
        Ty::NilI32 => ValType::I32,
    }
}

fn is_safe_defs_only_rhs(t: &Term) -> bool {
    match t {
        Term::Nil
        | Term::Bool(_)
        | Term::Int(_)
        | Term::Str(_)
        | Term::Bytes(_)
        | Term::Symbol(_) => true,
        _ => {
            let Some(xs) = t.as_proper_list() else {
                return false;
            };
            if xs.is_empty() {
                return false;
            }
            matches!(
                xs[0],
                Term::Symbol(s) if (s == "fn" && xs.len() == 3) || (s == "quote" && xs.len() == 2)
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use gc_coreform::{canonicalize_module, parse_module};

    use super::{Stage2ValueKind, stage2_validation_report};

    #[test]
    fn stage2_validates_simple_int_module() {
        let src = r#"
          (def x (prim int/add 40 2))
          x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
        assert!(r.wasm_bytes_len.unwrap_or(0) > 0);
    }

    #[test]
    fn stage2_validates_bool_comparison_module() {
        let src = r#"
          (prim int/lt? 1 2)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_begin_expression() {
        let src = r#"
          (begin
            (prim int/add 1 2)
            (prim int/mul 7 6))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_let_expression() {
        let src = r#"
          (let ((x 10) (y (prim int/add x 5)))
            (if (prim int/lt? y 20)
              (prim int/mul y 2)
              (prim int/sub y 1)))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_if_truthiness_for_int_condition() {
        let src = r#"
          (if (prim int/sub 3 3)
            7
            9)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_if_truthiness_for_nil_condition() {
        let src = r#"
          (if nil
            7
            9)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_quote_scalar_literals() {
        let src = r#"
          (if (quote false)
            (quote 10)
            (quote 11))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_immediate_lambda_application() {
        let src = r#"
          ((fn (x) (prim int/add x 1)) 41)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_immediate_lambda_application_with_capture() {
        let src = r#"
          (def base 40)
          ((fn (x)
             (prim int/add base x))
           2)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_immediate_lambda_application_with_multi_body() {
        let src = r#"
          ((fn (x)
             (prim int/add x 1)
             (prim int/mul x 2))
           5)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_def_bound_function_call() {
        let src = r#"
          (def add1 (fn (x) (prim int/add x 1)))
          (add1 41)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_def_bound_function_call_with_lexical_capture() {
        let src = r#"
          (def base 1)
          (def f (fn (x) (prim int/add x base)))
          (def base 10)
          (f base)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_def_bound_function_call_ignores_let_shadow_for_global_free_var() {
        let src = r#"
          (def base 1)
          (def f (fn (x) (prim int/add x base)))
          (let ((base 100))
            (f 1))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_def_bound_curried_call_chain() {
        let src = r#"
          (def add (fn (a b) (prim int/add a b)))
          ((add 1) 2)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_immediate_lambda_curried_call_chain() {
        let src = r#"
          (((fn (a b) (prim int/add a b)) 1) 2)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_def_alias_to_builtin_function_chain() {
        let src = r#"
          (def add core/int::add)
          ((add 1) 2)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_def_alias_to_user_defined_function() {
        let src = r#"
          (def inc (fn (x) (prim int/add x 1)))
          (def f inc)
          (f 41)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_let_bound_function_call() {
        let src = r#"
          (let ((f (fn (x) (prim int/add x 1))))
            (f 41))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_let_bound_function_lexical_capture_before_shadow() {
        let src = r#"
          (let ((a 1)
                (f (fn (x) (prim int/add x a)))
                (a 10))
            (f 1))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_let_bound_function_alias_chain() {
        let src = r#"
          (let ((f (fn (x) (prim int/add x 1)))
                (g f))
            (g 41))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_rejects_recursive_def_bound_function_call() {
        let src = r#"
          (def f (fn (x) (f x)))
          (f 1)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(!r.supported, "{r:?}");
        assert!(!r.ok, "{r:?}");
    }

    #[test]
    fn stage2_validates_curried_core_int_wrapper_calls() {
        let src = r#"
          (def x ((core/int::add 40) 2))
          (def y ((core/int::mul x) 3))
          ((core/int::sub y) 6)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_curried_core_int_predicate_calls() {
        let src = r#"
          (def x ((core/int::add 1) 2))
          ((core/int::lt? x) 10)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_core_eq_prim_for_ints_and_bools() {
        let src = r#"
          (def a (prim core/eq? (prim int/add 1 2) 3))
          (def b (prim core/eq? a true))
          b
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_curried_core_eq_wrapper_calls() {
        let src = r#"
          (def x ((core/int::add 1) 1))
          ((core/eq? x) 2)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_curried_core_eq_wrapper_calls_for_bool_and_nil() {
        let src = r#"
          (def a ((core/eq? true) true))
          (if a
            ((core/eq? nil) nil)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_core_eq_mixed_scalar_types_as_false() {
        let src = r#"
          (def a (prim core/eq? 1 true))
          (def b (prim core/eq? nil false))
          (if a
            1
            (if b 2 3))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_curried_core_eq_wrapper_call_for_mixed_scalar_types() {
        let src = r#"
          ((core/eq? (prim int/add 1 1)) true)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_defs_only_module_with_safe_rhs_and_nil_result() {
        let src = r#"
          (def add core/int::add)
          (def id (fn (x) x))
          (def marker (quote hello/world::marker))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
    }

    #[test]
    fn stage2_validates_defs_only_module_with_scalar_rhs_via_lowering() {
        let src = r#"
          (def x (prim int/add 1 2))
          (def y (prim int/mul x 10))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
    }

    #[test]
    fn stage2_validates_defs_only_module_with_quoted_scalar_rhs_via_lowering() {
        let src = r#"
          (def x (quote 42))
          (def y (quote true))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
    }

    #[test]
    fn stage2_rejects_defs_only_module_with_non_trivial_rhs() {
        let src = r#"
          (def x [1 2 3])
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(!r.supported, "{r:?}");
        assert!(!r.ok, "{r:?}");
    }

    #[test]
    fn stage2_reports_unsupported_for_effect_program() {
        let src = r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(!r.supported, "{r:?}");
        assert!(!r.ok, "{r:?}");
    }
}
