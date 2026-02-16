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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage2ValueKind {
    Int,
    Bool,
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
}

#[derive(Debug, Clone, Copy)]
struct Local {
    idx: u32,
    ty: Ty,
}

pub fn stage2_compile_module(forms: &[Term]) -> Result<Stage2CompileArtifact, Stage2CompileError> {
    let module_hash = hash_module(forms);
    let statements = parse_statements(forms)?;

    let mut locals_ty: BTreeMap<String, Ty> = BTreeMap::new();
    let mut local_order: Vec<(String, Ty)> = Vec::new();
    let mut expr_count = 0usize;
    let mut last_expr_ty = None;

    for stmt in &statements {
        match stmt {
            Stmt::Def(name, expr) => {
                let ty = infer_expr(expr, &locals_ty)?;
                locals_ty.insert(name.clone(), ty);
                local_order.push((name.clone(), ty));
            }
            Stmt::Expr(expr) => {
                expr_count = expr_count.saturating_add(1);
                last_expr_ty = Some(infer_expr(expr, &locals_ty)?);
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
    };

    let mut local_bindings: BTreeMap<String, Local> = BTreeMap::new();
    let mut locals_decl = Vec::new();
    for (i, (name, ty)) in local_order.iter().enumerate() {
        let idx = u32::try_from(i).map_err(|_| {
            Stage2CompileError::Internal("too many local definitions for wasm function".to_string())
        })?;
        locals_decl.push((1u32, val_ty(*ty)));
        local_bindings.insert(name.clone(), Local { idx, ty: *ty });
    }

    let mut func = Function::new(locals_decl);
    let mut seen_expr = 0usize;
    for stmt in &statements {
        match stmt {
            Stmt::Def(name, expr) => {
                let local = local_bindings.get(name).ok_or_else(|| {
                    Stage2CompileError::Internal(format!("missing local binding for {name}"))
                })?;
                let got = emit_expr(&mut func, expr, &local_bindings)?;
                if got != local.ty {
                    return Err(Stage2CompileError::Internal(format!(
                        "local type mismatch for {name}: expected {:?}, got {:?}",
                        local.ty, got
                    )));
                }
                func.instruction(&Instruction::LocalSet(local.idx));
            }
            Stmt::Expr(expr) => {
                seen_expr = seen_expr.saturating_add(1);
                let _ = emit_expr(&mut func, expr, &local_bindings)?;
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
    let mut ctx = EvalCtx::with_step_limit(None);
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
    }];
    func.call(&mut store, &[], &mut results)
        .map_err(|e| Stage2CompileError::Internal(format!("wasmi call eval: {e}")))?;

    match (kind, results[0].clone()) {
        (Stage2ValueKind::Int, Val::I64(v)) => Ok(Term::Int(v.into())),
        (Stage2ValueKind::Bool, Val::I32(v)) => Ok(Term::Bool(v != 0)),
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
    let mut has_expr = false;
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
        has_expr = true;
        out.push(Stmt::Expr(t.clone()));
    }
    if !has_expr {
        return Err(Stage2CompileError::Unsupported(
            "module has no executable expression forms".to_string(),
        ));
    }
    Ok(out)
}

fn infer_expr(t: &Term, env: &BTreeMap<String, Ty>) -> Result<Ty, Stage2CompileError> {
    match t {
        Term::Int(i) => {
            if i.to_i64().is_none() {
                return Err(Stage2CompileError::Unsupported(
                    "int literal out of i64 range for stage2".to_string(),
                ));
            }
            Ok(Ty::I64)
        }
        Term::Bool(_) => Ok(Ty::BoolI32),
        Term::Symbol(s) => env.get(s).copied().ok_or_else(|| {
            Stage2CompileError::Unsupported(format!("unknown symbol in stage2: {s}"))
        }),
        _ => infer_list_expr(t, env),
    }
}

fn infer_list_expr(t: &Term, env: &BTreeMap<String, Ty>) -> Result<Ty, Stage2CompileError> {
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
        let c = infer_expr(xs[1], env)?;
        if c != Ty::BoolI32 {
            return Err(Stage2CompileError::Unsupported(
                "if condition must be bool".to_string(),
            ));
        }
        let t_ty = infer_expr(xs[2], env)?;
        let e_ty = infer_expr(xs[3], env)?;
        if t_ty != e_ty {
            return Err(Stage2CompileError::Unsupported(
                "if branches must have matching types".to_string(),
            ));
        }
        return Ok(t_ty);
    }
    if xs.len() == 4
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
    {
        let a = infer_expr(xs[2], env)?;
        let b = infer_expr(xs[3], env)?;
        return infer_prim(op, a, b);
    }
    Err(Stage2CompileError::Unsupported(
        "unsupported expression form in stage2".to_string(),
    ))
}

fn infer_prim(op: &str, a: Ty, b: Ty) -> Result<Ty, Stage2CompileError> {
    match op {
        "int/add" | "int/sub" | "int/mul" => {
            if a == Ty::I64 && b == Ty::I64 {
                Ok(Ty::I64)
            } else {
                Err(Stage2CompileError::Unsupported(format!(
                    "{op} expects int arguments"
                )))
            }
        }
        "int/eq?" | "int/lt?" => {
            if a == Ty::I64 && b == Ty::I64 {
                Ok(Ty::BoolI32)
            } else {
                Err(Stage2CompileError::Unsupported(format!(
                    "{op} expects int arguments"
                )))
            }
        }
        _ => Err(Stage2CompileError::Unsupported(format!(
            "prim {op} is unsupported in stage2"
        ))),
    }
}

fn emit_expr(
    f: &mut Function,
    t: &Term,
    env: &BTreeMap<String, Local>,
) -> Result<Ty, Stage2CompileError> {
    match t {
        Term::Int(i) => {
            let n = i.to_i64().ok_or_else(|| {
                Stage2CompileError::Unsupported(
                    "int literal out of i64 range for stage2".to_string(),
                )
            })?;
            f.instruction(&Instruction::I64Const(n));
            Ok(Ty::I64)
        }
        Term::Bool(b) => {
            f.instruction(&Instruction::I32Const(if *b { 1 } else { 0 }));
            Ok(Ty::BoolI32)
        }
        Term::Symbol(s) => {
            let local = env.get(s).ok_or_else(|| {
                Stage2CompileError::Unsupported(format!("unknown symbol in stage2: {s}"))
            })?;
            f.instruction(&Instruction::LocalGet(local.idx));
            Ok(local.ty)
        }
        _ => emit_list_expr(f, t, env),
    }
}

fn emit_list_expr(
    f: &mut Function,
    t: &Term,
    env: &BTreeMap<String, Local>,
) -> Result<Ty, Stage2CompileError> {
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
        let c = emit_expr(f, xs[1], env)?;
        if c != Ty::BoolI32 {
            return Err(Stage2CompileError::Unsupported(
                "if condition must be bool".to_string(),
            ));
        }
        let ty_env: BTreeMap<String, Ty> = env.iter().map(|(k, v)| (k.clone(), v.ty)).collect();
        let t_ty = infer_expr(xs[2], &ty_env)?;
        let e_ty = infer_expr(xs[3], &ty_env)?;
        if t_ty != e_ty {
            return Err(Stage2CompileError::Unsupported(
                "if branches must have matching types".to_string(),
            ));
        }
        f.instruction(&Instruction::If(BlockType::Result(val_ty(t_ty))));
        let got_t = emit_expr(f, xs[2], env)?;
        if got_t != t_ty {
            return Err(Stage2CompileError::Internal(
                "if then branch type changed during emission".to_string(),
            ));
        }
        f.instruction(&Instruction::Else);
        let got_e = emit_expr(f, xs[3], env)?;
        if got_e != t_ty {
            return Err(Stage2CompileError::Internal(
                "if else branch type changed during emission".to_string(),
            ));
        }
        f.instruction(&Instruction::End);
        return Ok(t_ty);
    }
    if xs.len() == 4
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
    {
        let a = emit_expr(f, xs[2], env)?;
        let b = emit_expr(f, xs[3], env)?;
        let out = infer_prim(op, a, b)?;
        match op.as_str() {
            "int/add" => {
                f.instruction(&Instruction::I64Add);
            }
            "int/sub" => {
                f.instruction(&Instruction::I64Sub);
            }
            "int/mul" => {
                f.instruction(&Instruction::I64Mul);
            }
            "int/eq?" => {
                f.instruction(&Instruction::I64Eq);
            }
            "int/lt?" => {
                f.instruction(&Instruction::I64LtS);
            }
            _ => {
                return Err(Stage2CompileError::Unsupported(format!(
                    "prim {op} is unsupported in stage2"
                )));
            }
        }
        return Ok(out);
    }
    Err(Stage2CompileError::Unsupported(
        "unsupported expression form in stage2".to_string(),
    ))
}

fn val_ty(t: Ty) -> ValType {
    match t {
        Ty::I64 => ValType::I64,
        Ty::BoolI32 => ValType::I32,
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
