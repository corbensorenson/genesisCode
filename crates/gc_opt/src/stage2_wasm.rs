use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, hash_module};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;
use num_traits::ToPrimitive;
use thiserror::Error;
use wasm_encoder::{
    BlockType, CodeSection, ExportKind, ExportSection, Function, FunctionSection, Instruction,
    Module, TypeSection, ValType,
};
use wasmi::{Engine, Linker, Module as WasmiModule, Store, Val};

// Stage2 ownership: this root keeps pipeline orchestration and stable public API;
// lowering and helper details are split into focused submodules for AI-scale edits.
#[path = "stage2_wasm/callable_emit.rs"]
mod callable_emit;
#[path = "stage2_wasm/collections_lowering.rs"]
mod collections_lowering;
#[path = "stage2_wasm/expr_lowering.rs"]
mod expr_lowering;
#[path = "stage2_wasm/pipeline_exec.rs"]
mod pipeline_exec;
#[path = "stage2_wasm/planner_helpers.rs"]
mod planner_helpers;
#[path = "stage2_wasm/strings_bytes_lowering.rs"]
mod strings_bytes_lowering;

use callable_emit::*;
use collections_lowering::*;
use expr_lowering::*;
use pipeline_exec::*;
use planner_helpers::*;
use strings_bytes_lowering::*;

const STAGE2_BASELINE_STEP_LIMIT: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage2ValueKind {
    Int,
    Bool,
    Nil,
    Sym,
    Str,
    Bytes,
    Term,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage2LoweringMode {
    Strict,
    ConstantFallback,
}

#[derive(Debug, Clone)]
pub struct Stage2CompileArtifact {
    pub wasm_bytes: Vec<u8>,
    pub wasm_hash: [u8; 32],
    pub module_hash: [u8; 32],
    pub lowering_mode: Stage2LoweringMode,
    pub value_kind: Stage2ValueKind,
    pub symbol_table: Vec<String>,
    pub string_table: Vec<String>,
    pub bytes_table: Vec<Vec<u8>>,
    pub term_table: Vec<Term>,
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
    pub lowering_mode: Option<Stage2LoweringMode>,
    pub wasm_hash: Option<[u8; 32]>,
    pub value_kind: Option<Stage2ValueKind>,
    pub original_value_hash: Option<[u8; 32]>,
    pub wasm_value_hash: Option<[u8; 32]>,
    pub wasm_bytes_len: Option<usize>,
    pub errors: Vec<String>,
}

#[path = "stage2_wasm/planner_ir.rs"]
mod planner_ir;

use planner_ir::*;

pub fn stage2_compile_module(forms: &[Term]) -> Result<Stage2CompileArtifact, Stage2CompileError> {
    stage2_compile_module_pipeline(forms)
}

pub fn stage2_validation_report(forms: &[Term]) -> Stage2ValidationReport {
    stage2_validation_report_pipeline(forms)
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
        Term::Str(s) => Ok(PExpr::Str(planner.intern_string(s)?)),
        Term::Bytes(bs) => Ok(PExpr::Bytes(planner.intern_bytes(bs)?)),
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
        if !matches!(
            cond_ty,
            Ty::BoolI32 | Ty::NilI32 | Ty::I64 | Ty::SymI32 | Ty::StrI32 | Ty::BytesI32
        ) {
            return Err(Stage2CompileError::Unsupported(
                "if condition must be scalar (bool, nil, int, symbol, string, or bytes)"
                    .to_string(),
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
            Term::Symbol(sym) => Ok(PExpr::Sym(planner.intern_symbol(sym)?)),
            Term::Str(s) => Ok(PExpr::Str(planner.intern_string(s)?)),
            Term::Bytes(bs) => Ok(PExpr::Bytes(planner.intern_bytes(bs)?)),
            _ => Err(Stage2CompileError::Unsupported(
                "quote is stage2-supported only for scalar nil/bool/int/symbol/string/bytes"
                    .to_string(),
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
        let mut vec_aliases: BTreeMap<String, Vec<Term>> = BTreeMap::new();
        let mut map_aliases: BTreeMap<String, BTreeMap<TermOrdKey, Term>> = BTreeMap::new();
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
            if let Some(items) = term_const_vector_expr_with_aliases(
                pair[1],
                &vec_aliases,
                &planner.global_const_vector_aliases,
            )? {
                env2.remove(name);
                local_fn_defs2.remove(name);
                vec_aliases.insert(name.clone(), items);
                continue;
            }
            if let Some(items) = term_const_map_expr_with_aliases(
                pair[1],
                &map_aliases,
                &planner.global_const_map_aliases,
            )? {
                env2.remove(name);
                local_fn_defs2.remove(name);
                map_aliases.insert(name.clone(), items);
                continue;
            }
            if let Term::Symbol(sym) = pair[1]
                && !env2.contains_key(sym)
                && !local_fn_defs2.contains_key(sym)
            {
                if let Some(items) = map_aliases.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    map_aliases.insert(name.clone(), items);
                    continue;
                }
                if let Some(items) = planner.global_const_map_aliases.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    map_aliases.insert(name.clone(), items);
                    continue;
                }
            }
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
            record_local_const_ids(planner, idx, &rhs);
            env2.insert(name.clone(), Local { idx, ty: rhs.ty() });
            local_fn_defs2.remove(name);
            bindings.push(LetBinding { idx, expr: rhs });
        }
        let mut body = Vec::with_capacity(xs.len() - 2);
        for x in xs.iter().skip(2) {
            let resolved = resolve_collection_aliases_term(x, &vec_aliases, &map_aliases);
            body.push(plan_expr(
                &resolved,
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
        if op == "str/join" {
            return lower_str_join_terms(
                xs[2],
                xs[3],
                env,
                global_env,
                fn_defs,
                local_fn_defs,
                planner,
            );
        }
        if op == "map/get" {
            return lower_map_get_terms(
                xs[2],
                xs[3],
                env,
                global_env,
                fn_defs,
                local_fn_defs,
                planner,
            );
        }
        if op == "vec/get" {
            return lower_vec_get_terms(
                xs[2],
                xs[3],
                env,
                global_env,
                fn_defs,
                local_fn_defs,
                planner,
            );
        }
        let lhs = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        let rhs = plan_expr(xs[3], env, global_env, fn_defs, local_fn_defs, planner)?;
        if op == "str/concat" {
            return lower_str_concat(lhs, rhs, planner);
        }
        if op == "str/repeat" {
            return lower_str_repeat(lhs, rhs, planner);
        }
        if op == "bytes/get" {
            return lower_bytes_get(lhs, rhs, planner);
        }
        if op == "bytes/concat" {
            return lower_bytes_concat(lhs, rhs, planner);
        }
        let (prim_op, ty) = infer_prim(op, lhs.ty(), rhs.ty())?;
        return Ok(PExpr::Prim {
            op: prim_op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            ty,
        });
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "vec/len"
    {
        return lower_vec_len_term(xs[2], env, global_env, fn_defs, local_fn_defs, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "map/len"
    {
        return lower_map_len_term(xs[2], env, global_env, fn_defs, local_fn_defs, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "bytes/join"
    {
        return lower_bytes_join_term(xs[2], env, global_env, fn_defs, local_fn_defs, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "list/is-nil?"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_list_is_nil(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "data/tag"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_data_tag(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "sym/to-str"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_sym_to_str(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "sym/from-str"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_sym_from_str(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "str/to-bytes-utf8"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_str_to_utf8(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "bytes/to-str-utf8"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_bytes_to_str_utf8(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "bytes/to-hex"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_bytes_to_hex(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "bytes/from-hex"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_bytes_from_hex(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "int/to-str"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_int_to_str(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "bytes/len"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_bytes_len(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "str/len"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_str_len(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "coreform/escape-str"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_coreform_escape_str(arg, planner);
    }
    if xs.len() == 3
        && matches!(xs[0], Term::Symbol(s) if s == "prim")
        && let Term::Symbol(op) = &xs[1]
        && op == "coreform/escape-bytes"
    {
        let arg = plan_expr(xs[2], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_coreform_escape_bytes(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/str::len") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_str_len(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/int::to-str") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_int_to_str(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/sym::to-str") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_sym_to_str(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/sym::from-str") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_sym_from_str(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/str::to-utf8") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_str_to_utf8(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/str::from-utf8") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_bytes_to_str_utf8(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/bytes::to-hex") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_bytes_to_hex(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/bytes::from-hex") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_bytes_from_hex(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/coreform::escape-str") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_coreform_escape_str(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/coreform::escape-bytes") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_coreform_escape_bytes(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/bytes::len") {
        let arg = plan_expr(xs[1], env, global_env, fn_defs, local_fn_defs, planner)?;
        return lower_bytes_len(arg, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/map::len") {
        return lower_map_len_term(xs[1], env, global_env, fn_defs, local_fn_defs, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/vec::len") {
        return lower_vec_len_term(xs[1], env, global_env, fn_defs, local_fn_defs, planner);
    }
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "core/bytes::join") {
        return lower_bytes_join_term(xs[1], env, global_env, fn_defs, local_fn_defs, planner);
    }
    if let Some((op_sym, lhs_t, rhs_t)) = match_curried_wrapper_call(&xs) {
        if op_sym == "str/join" {
            return lower_str_join_terms(
                &lhs_t,
                &rhs_t,
                env,
                global_env,
                fn_defs,
                local_fn_defs,
                planner,
            );
        }
        if op_sym == "map/get" {
            return lower_map_get_terms(
                &lhs_t,
                &rhs_t,
                env,
                global_env,
                fn_defs,
                local_fn_defs,
                planner,
            );
        }
        if op_sym == "vec/get" {
            return lower_vec_get_terms(
                &lhs_t,
                &rhs_t,
                env,
                global_env,
                fn_defs,
                local_fn_defs,
                planner,
            );
        }
        let lhs = plan_expr(&lhs_t, env, global_env, fn_defs, local_fn_defs, planner)?;
        let rhs = plan_expr(&rhs_t, env, global_env, fn_defs, local_fn_defs, planner)?;
        if op_sym == "str/concat" {
            return lower_str_concat(lhs, rhs, planner);
        }
        if op_sym == "str/repeat" {
            return lower_str_repeat(lhs, rhs, planner);
        }
        if op_sym == "bytes/get" {
            return lower_bytes_get(lhs, rhs, planner);
        }
        if op_sym == "bytes/concat" {
            return lower_bytes_concat(lhs, rhs, planner);
        }
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

#[cfg(test)]
#[path = "stage2_wasm/tests/mod.rs"]
mod tests;
