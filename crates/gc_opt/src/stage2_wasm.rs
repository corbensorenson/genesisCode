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
#[path = "stage2_wasm/pipeline_exec.rs"]
mod pipeline_exec;
#[path = "stage2_wasm/planner_helpers.rs"]
mod planner_helpers;
#[path = "stage2_wasm/strings_bytes_lowering.rs"]
mod strings_bytes_lowering;

use callable_emit::*;
use collections_lowering::*;
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
}

#[derive(Debug, Clone)]
pub struct Stage2CompileArtifact {
    pub wasm_bytes: Vec<u8>,
    pub wasm_hash: [u8; 32],
    pub module_hash: [u8; 32],
    pub value_kind: Stage2ValueKind,
    pub symbol_table: Vec<String>,
    pub string_table: Vec<String>,
    pub bytes_table: Vec<Vec<u8>>,
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

fn lower_str_join_terms(
    parts_t: &Term,
    sep_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let sep = plan_expr(sep_t, env, global_env, fn_defs, local_fn_defs, planner)?;
    if sep.ty() != Ty::StrI32 {
        return Err(Stage2CompileError::Unsupported(
            "str/join expects (vector-of-strings, string) arguments in stage2".to_string(),
        ));
    }
    lower_str_join_parts_term(
        parts_t,
        sep,
        env,
        global_env,
        fn_defs,
        local_fn_defs,
        planner,
    )
}

fn lower_str_join_parts_term(
    parts_t: &Term,
    sep: PExpr,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let scope = VecGetScope {
        env,
        global_env,
        fn_defs,
        local_fn_defs,
    };
    let vec_aliases: BTreeMap<String, Vec<Term>> = BTreeMap::new();
    lower_str_join_parts_term_with_aliases(parts_t, sep, &scope, &vec_aliases, planner)
}

fn lower_str_join_parts_term_with_aliases(
    parts_t: &Term,
    sep: PExpr,
    scope: &VecGetScope<'_>,
    vec_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let global_vec_aliases = planner.global_const_vector_aliases.clone();
    if let Some(parts_ids) = term_const_string_vector_ids_with_aliases(
        parts_t,
        vec_aliases,
        &global_vec_aliases,
        planner,
    )? {
        return lower_str_join_sep_expr(parts_ids, sep, planner);
    }

    let Some(xs) = parts_t.as_proper_list() else {
        return Err(Stage2CompileError::Unsupported(
            "str/join currently requires stage2-known vector literals".to_string(),
        ));
    };
    if xs.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "str/join currently requires stage2-known vector literals".to_string(),
        ));
    }

    if matches!(xs[0], Term::Symbol(s) if s == "begin") {
        if xs.len() < 2 {
            return Err(Stage2CompileError::Unsupported(
                "begin must have at least one expression".to_string(),
            ));
        }
        let mut exprs = Vec::with_capacity(xs.len() - 1);
        for x in xs.iter().skip(1).take(xs.len().saturating_sub(2)) {
            exprs.push(plan_expr(
                x,
                scope.env,
                scope.global_env,
                scope.fn_defs,
                scope.local_fn_defs,
                planner,
            )?);
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("str/join begin had no body".to_string())
        })?;
        exprs.push(lower_str_join_parts_term_with_aliases(
            last,
            sep,
            scope,
            vec_aliases,
            planner,
        )?);
        return Ok(PExpr::Begin {
            exprs,
            ty: Ty::StrI32,
        });
    }

    if matches!(xs[0], Term::Symbol(s) if s == "if") {
        if xs.len() != 4 {
            return Err(Stage2CompileError::Unsupported(
                "if must have exactly 3 arguments".to_string(),
            ));
        }
        let cond = plan_expr(
            xs[1],
            scope.env,
            scope.global_env,
            scope.fn_defs,
            scope.local_fn_defs,
            planner,
        )?;
        let cond_ty = cond.ty();
        ensure_scalar_cond_ty(cond_ty)?;
        let then_expr = lower_str_join_parts_term_with_aliases(
            xs[2],
            sep.clone(),
            scope,
            vec_aliases,
            planner,
        )?;
        let else_expr =
            lower_str_join_parts_term_with_aliases(xs[3], sep, scope, vec_aliases, planner)?;
        return Ok(PExpr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
            cond_ty,
            ty: Ty::StrI32,
        });
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
        let mut env2 = scope.env.clone();
        let mut local_fn_defs2 = scope.local_fn_defs.clone();
        let mut vec_aliases2 = vec_aliases.clone();
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
                &vec_aliases2,
                &planner.global_const_vector_aliases,
            )? {
                env2.remove(name);
                local_fn_defs2.remove(name);
                vec_aliases2.insert(name.clone(), items);
                continue;
            }
            if let Term::Symbol(sym) = pair[1]
                && !env2.contains_key(sym)
                && !local_fn_defs2.contains_key(sym)
            {
                if let Some(items) = vec_aliases2.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    vec_aliases2.insert(name.clone(), items);
                    continue;
                }
                if let Some(items) = planner.global_const_vector_aliases.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    vec_aliases2.insert(name.clone(), items);
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
                && let Some(alias_fn) =
                    resolve_inlinable_symbol(sym, scope.fn_defs, &local_fn_defs2)
            {
                env2.remove(name);
                local_fn_defs2.insert(name.clone(), alias_fn);
                continue;
            }

            let rhs = plan_expr(
                pair[1],
                &env2,
                scope.global_env,
                scope.fn_defs,
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
        if xs.len() > 3 {
            for x in xs.iter().skip(2).take(xs.len() - 3) {
                body.push(plan_expr(
                    x,
                    &env2,
                    scope.global_env,
                    scope.fn_defs,
                    &local_fn_defs2,
                    planner,
                )?);
            }
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("str/join let had empty body".to_string())
        })?;
        let scope2 = VecGetScope {
            env: &env2,
            global_env: scope.global_env,
            fn_defs: scope.fn_defs,
            local_fn_defs: &local_fn_defs2,
        };
        body.push(lower_str_join_parts_term_with_aliases(
            last,
            sep,
            &scope2,
            &vec_aliases2,
            planner,
        )?);
        return Ok(PExpr::Let {
            bindings,
            body,
            ty: Ty::StrI32,
        });
    }

    Err(Stage2CompileError::Unsupported(
        "str/join currently requires stage2-known vector literals".to_string(),
    ))
}

fn lower_str_join_sep_expr(
    parts_ids: Vec<i32>,
    sep: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(sep_id) = planner_const_string_id(planner, &sep) {
        return lower_str_join_const_pair(parts_ids, sep, sep_id, planner);
    }
    match sep {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal(
                    "str/join separator begin had no expressions".to_string(),
                )
            })?;
            let lowered = lower_str_join_sep_expr(parts_ids, last, planner)?;
            exprs.push(lowered);
            Ok(PExpr::Begin {
                exprs,
                ty: Ty::StrI32,
            })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/join separator let had empty body".to_string())
            })?;
            let lowered = lower_str_join_sep_expr(parts_ids, last, planner)?;
            body.push(lowered);
            Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::StrI32,
            })
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::StrI32,
        } => {
            let then_lowered = lower_str_join_sep_expr(parts_ids.clone(), *then_expr, planner)?;
            let else_lowered = lower_str_join_sep_expr(parts_ids, *else_expr, planner)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "str/join currently requires a stage2-known string separator".to_string(),
        )),
    }
}

fn lower_bytes_join_term(
    parts_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    lower_bytes_join_parts_term(parts_t, env, global_env, fn_defs, local_fn_defs, planner)
}

fn lower_bytes_join_parts_term(
    parts_t: &Term,
    env: &BTreeMap<String, Local>,
    global_env: &BTreeMap<String, Local>,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let scope = VecGetScope {
        env,
        global_env,
        fn_defs,
        local_fn_defs,
    };
    let vec_aliases: BTreeMap<String, Vec<Term>> = BTreeMap::new();
    lower_bytes_join_parts_term_with_aliases(parts_t, &scope, &vec_aliases, planner)
}

fn lower_bytes_join_parts_term_with_aliases(
    parts_t: &Term,
    scope: &VecGetScope<'_>,
    vec_aliases: &BTreeMap<String, Vec<Term>>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let global_vec_aliases = planner.global_const_vector_aliases.clone();
    if let Some(parts_ids) = term_const_bytes_vector_ids_with_aliases(
        parts_t,
        vec_aliases,
        &global_vec_aliases,
        planner,
    )? {
        return lower_bytes_join_const_parts(parts_ids, planner);
    }

    let Some(xs) = parts_t.as_proper_list() else {
        return Err(Stage2CompileError::Unsupported(
            "bytes/join currently requires stage2-known vector literals".to_string(),
        ));
    };
    if xs.is_empty() {
        return Err(Stage2CompileError::Unsupported(
            "bytes/join currently requires stage2-known vector literals".to_string(),
        ));
    }

    if matches!(xs[0], Term::Symbol(s) if s == "begin") {
        if xs.len() < 2 {
            return Err(Stage2CompileError::Unsupported(
                "begin must have at least one expression".to_string(),
            ));
        }
        let mut exprs = Vec::with_capacity(xs.len() - 1);
        for x in xs.iter().skip(1).take(xs.len().saturating_sub(2)) {
            exprs.push(plan_expr(
                x,
                scope.env,
                scope.global_env,
                scope.fn_defs,
                scope.local_fn_defs,
                planner,
            )?);
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("bytes/join begin had no body".to_string())
        })?;
        exprs.push(lower_bytes_join_parts_term_with_aliases(
            last,
            scope,
            vec_aliases,
            planner,
        )?);
        return Ok(PExpr::Begin {
            exprs,
            ty: Ty::BytesI32,
        });
    }

    if matches!(xs[0], Term::Symbol(s) if s == "if") {
        if xs.len() != 4 {
            return Err(Stage2CompileError::Unsupported(
                "if must have exactly 3 arguments".to_string(),
            ));
        }
        let cond = plan_expr(
            xs[1],
            scope.env,
            scope.global_env,
            scope.fn_defs,
            scope.local_fn_defs,
            planner,
        )?;
        let cond_ty = cond.ty();
        ensure_scalar_cond_ty(cond_ty)?;
        let then_expr =
            lower_bytes_join_parts_term_with_aliases(xs[2], scope, vec_aliases, planner)?;
        let else_expr =
            lower_bytes_join_parts_term_with_aliases(xs[3], scope, vec_aliases, planner)?;
        return Ok(PExpr::If {
            cond: Box::new(cond),
            then_expr: Box::new(then_expr),
            else_expr: Box::new(else_expr),
            cond_ty,
            ty: Ty::BytesI32,
        });
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
        let mut env2 = scope.env.clone();
        let mut local_fn_defs2 = scope.local_fn_defs.clone();
        let mut vec_aliases2 = vec_aliases.clone();
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
                &vec_aliases2,
                &planner.global_const_vector_aliases,
            )? {
                env2.remove(name);
                local_fn_defs2.remove(name);
                vec_aliases2.insert(name.clone(), items);
                continue;
            }
            if let Term::Symbol(sym) = pair[1]
                && !env2.contains_key(sym)
                && !local_fn_defs2.contains_key(sym)
            {
                if let Some(items) = vec_aliases2.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    vec_aliases2.insert(name.clone(), items);
                    continue;
                }
                if let Some(items) = planner.global_const_vector_aliases.get(sym).cloned() {
                    env2.remove(name);
                    local_fn_defs2.remove(name);
                    vec_aliases2.insert(name.clone(), items);
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
                && let Some(alias_fn) =
                    resolve_inlinable_symbol(sym, scope.fn_defs, &local_fn_defs2)
            {
                env2.remove(name);
                local_fn_defs2.insert(name.clone(), alias_fn);
                continue;
            }

            let rhs = plan_expr(
                pair[1],
                &env2,
                scope.global_env,
                scope.fn_defs,
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
        if xs.len() > 3 {
            for x in xs.iter().skip(2).take(xs.len() - 3) {
                body.push(plan_expr(
                    x,
                    &env2,
                    scope.global_env,
                    scope.fn_defs,
                    &local_fn_defs2,
                    planner,
                )?);
            }
        }
        let last = xs.last().copied().ok_or_else(|| {
            Stage2CompileError::Internal("bytes/join let had empty body".to_string())
        })?;
        let scope2 = VecGetScope {
            env: &env2,
            global_env: scope.global_env,
            fn_defs: scope.fn_defs,
            local_fn_defs: &local_fn_defs2,
        };
        body.push(lower_bytes_join_parts_term_with_aliases(
            last,
            &scope2,
            &vec_aliases2,
            planner,
        )?);
        return Ok(PExpr::Let {
            bindings,
            body,
            ty: Ty::BytesI32,
        });
    }

    Err(Stage2CompileError::Unsupported(
        "bytes/join currently requires stage2-known vector literals".to_string(),
    ))
}

fn lower_str_concat_const_pair(
    lhs: PExpr,
    rhs: PExpr,
    lhs_id: i32,
    rhs_id: i32,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let mut out = planner_string_for_id(planner, lhs_id)?;
    out.push_str(&planner_string_for_id(planner, rhs_id)?);
    let out_id = planner.intern_string(&out)?;
    let lhs_idx = planner.alloc_local(Ty::StrI32)?;
    let rhs_idx = planner.alloc_local(Ty::StrI32)?;
    Ok(PExpr::Let {
        bindings: vec![
            LetBinding {
                idx: lhs_idx,
                expr: lhs,
            },
            LetBinding {
                idx: rhs_idx,
                expr: rhs,
            },
        ],
        body: vec![PExpr::Str(out_id)],
        ty: Ty::StrI32,
    })
}

fn lower_str_join_const_pair(
    parts_ids: Vec<i32>,
    sep: PExpr,
    sep_id: i32,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let sep_s = planner_string_for_id(planner, sep_id)?;
    let mut out = String::new();
    for (i, pid) in parts_ids.iter().enumerate() {
        if i > 0 {
            out.push_str(&sep_s);
        }
        out.push_str(&planner_string_for_id(planner, *pid)?);
    }
    let out_id = planner.intern_string(&out)?;
    let sep_idx = planner.alloc_local(Ty::StrI32)?;
    Ok(PExpr::Let {
        bindings: vec![LetBinding {
            idx: sep_idx,
            expr: sep,
        }],
        body: vec![PExpr::Str(out_id)],
        ty: Ty::StrI32,
    })
}

fn lower_str_repeat_const_pair(
    lhs: PExpr,
    rhs: PExpr,
    lhs_id: i32,
    rhs_n: i64,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let n = usize::try_from(rhs_n).map_err(|_| {
        Stage2CompileError::Unsupported(
            "str/repeat currently requires non-negative int counts".to_string(),
        )
    })?;
    let out = planner_string_for_id(planner, lhs_id)?.repeat(n);
    let out_id = planner.intern_string(&out)?;
    let lhs_idx = planner.alloc_local(Ty::StrI32)?;
    let rhs_idx = planner.alloc_local(Ty::I64)?;
    Ok(PExpr::Let {
        bindings: vec![
            LetBinding {
                idx: lhs_idx,
                expr: lhs,
            },
            LetBinding {
                idx: rhs_idx,
                expr: rhs,
            },
        ],
        body: vec![PExpr::Str(out_id)],
        ty: Ty::StrI32,
    })
}

fn lower_bytes_join_const_parts(
    parts_ids: Vec<i32>,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let mut out = Vec::new();
    for pid in parts_ids {
        out.extend_from_slice(&planner_bytes_for_id(planner, pid)?);
    }
    let out_id = planner.intern_bytes(&out)?;
    Ok(PExpr::Bytes(out_id))
}

fn lower_bytes_concat_const_pair(
    lhs: PExpr,
    rhs: PExpr,
    lhs_id: i32,
    rhs_id: i32,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let lhs_bytes = planner_bytes_for_id(planner, lhs_id)?;
    let rhs_bytes = planner_bytes_for_id(planner, rhs_id)?;
    let mut out = Vec::with_capacity(lhs_bytes.len().saturating_add(rhs_bytes.len()));
    out.extend_from_slice(&lhs_bytes);
    out.extend_from_slice(&rhs_bytes);
    let out_id = planner.intern_bytes(&out)?;
    let lhs_idx = planner.alloc_local(Ty::BytesI32)?;
    let rhs_idx = planner.alloc_local(Ty::BytesI32)?;
    Ok(PExpr::Let {
        bindings: vec![
            LetBinding {
                idx: lhs_idx,
                expr: lhs,
            },
            LetBinding {
                idx: rhs_idx,
                expr: rhs,
            },
        ],
        body: vec![PExpr::Bytes(out_id)],
        ty: Ty::BytesI32,
    })
}

fn lower_bytes_get_const_pair(
    lhs: PExpr,
    rhs: PExpr,
    lhs_id: i32,
    rhs_n: i64,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let idx = usize::try_from(rhs_n).map_err(|_| {
        Stage2CompileError::Unsupported(
            "bytes/get currently requires non-negative in-range indices".to_string(),
        )
    })?;
    let bs = planner_bytes_for_id(planner, lhs_id)?;
    let b = bs.get(idx).copied().ok_or_else(|| {
        Stage2CompileError::Unsupported(
            "bytes/get currently requires non-negative in-range indices".to_string(),
        )
    })?;
    let lhs_idx = planner.alloc_local(Ty::BytesI32)?;
    let rhs_idx = planner.alloc_local(Ty::I64)?;
    Ok(PExpr::Let {
        bindings: vec![
            LetBinding {
                idx: lhs_idx,
                expr: lhs,
            },
            LetBinding {
                idx: rhs_idx,
                expr: rhs,
            },
        ],
        body: vec![PExpr::Int(i64::from(b))],
        ty: Ty::I64,
    })
}

fn lower_vec_get_const_pair(
    items: Vec<PExpr>,
    idx: PExpr,
    idx_n: i64,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    let idx_usize = usize::try_from(idx_n).map_err(|_| {
        Stage2CompileError::Unsupported(
            "vec/get currently requires non-negative int indices".to_string(),
        )
    })?;
    let chosen = items.get(idx_usize).cloned().unwrap_or(PExpr::Nil);
    let idx_local = planner.alloc_local(Ty::I64)?;
    let ty = chosen.ty();
    Ok(PExpr::Let {
        bindings: vec![LetBinding {
            idx: idx_local,
            expr: idx,
        }],
        body: vec![chosen],
        ty,
    })
}

fn lower_str_repeat_expr(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let (Some(lhs_id), Some(rhs_n)) = (
        planner_const_string_id(planner, &lhs),
        planner_const_int_value(planner, &rhs),
    ) {
        return lower_str_repeat_const_pair(lhs, rhs, lhs_id, rhs_n, planner);
    }

    let lhs = match lhs {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/repeat lhs begin had no expressions".to_string())
            })?;
            let lowered = lower_str_repeat_expr(last, rhs, planner)?;
            exprs.push(lowered);
            return Ok(PExpr::Begin {
                exprs,
                ty: Ty::StrI32,
            });
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/repeat lhs let had empty body".to_string())
            })?;
            let lowered = lower_str_repeat_expr(last, rhs, planner)?;
            body.push(lowered);
            return Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::StrI32,
            });
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::StrI32,
        } => {
            let then_lowered = lower_str_repeat_expr(*then_expr, rhs.clone(), planner)?;
            let else_lowered = lower_str_repeat_expr(*else_expr, rhs, planner)?;
            return Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: Ty::StrI32,
            });
        }
        other => other,
    };

    match rhs {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/repeat rhs begin had no expressions".to_string())
            })?;
            let lowered = lower_str_repeat_expr(lhs, last, planner)?;
            exprs.push(lowered);
            Ok(PExpr::Begin {
                exprs,
                ty: Ty::StrI32,
            })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/repeat rhs let had empty body".to_string())
            })?;
            let lowered = lower_str_repeat_expr(lhs, last, planner)?;
            body.push(lowered);
            Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::StrI32,
            })
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::I64,
        } => {
            let then_lowered = lower_str_repeat_expr(lhs.clone(), *then_expr, planner)?;
            let else_lowered = lower_str_repeat_expr(lhs, *else_expr, planner)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "str/repeat currently requires stage2-known string and int values".to_string(),
        )),
    }
}

fn lower_str_concat_expr(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let (Some(lhs_id), Some(rhs_id)) = (
        planner_const_string_id(planner, &lhs),
        planner_const_string_id(planner, &rhs),
    ) {
        return lower_str_concat_const_pair(lhs, rhs, lhs_id, rhs_id, planner);
    }

    let lhs = match lhs {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/concat lhs begin had no expressions".to_string())
            })?;
            let lowered = lower_str_concat_expr(last, rhs, planner)?;
            exprs.push(lowered);
            return Ok(PExpr::Begin {
                exprs,
                ty: Ty::StrI32,
            });
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/concat lhs let had empty body".to_string())
            })?;
            let lowered = lower_str_concat_expr(last, rhs, planner)?;
            body.push(lowered);
            return Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::StrI32,
            });
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::StrI32,
        } => {
            let then_lowered = lower_str_concat_expr(*then_expr, rhs.clone(), planner)?;
            let else_lowered = lower_str_concat_expr(*else_expr, rhs, planner)?;
            return Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: Ty::StrI32,
            });
        }
        other => other,
    };

    match rhs {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/concat rhs begin had no expressions".to_string())
            })?;
            let lowered = lower_str_concat_expr(lhs, last, planner)?;
            exprs.push(lowered);
            Ok(PExpr::Begin {
                exprs,
                ty: Ty::StrI32,
            })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/concat rhs let had empty body".to_string())
            })?;
            let lowered = lower_str_concat_expr(lhs, last, planner)?;
            body.push(lowered);
            Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::StrI32,
            })
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::StrI32,
        } => {
            let then_lowered = lower_str_concat_expr(lhs.clone(), *then_expr, planner)?;
            let else_lowered = lower_str_concat_expr(lhs, *else_expr, planner)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "str/concat currently requires stage2-known string values".to_string(),
        )),
    }
}

fn lower_bytes_concat_expr(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let (Some(lhs_id), Some(rhs_id)) = (
        planner_const_bytes_id(planner, &lhs),
        planner_const_bytes_id(planner, &rhs),
    ) {
        return lower_bytes_concat_const_pair(lhs, rhs, lhs_id, rhs_id, planner);
    }

    let lhs = match lhs {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal(
                    "bytes/concat lhs begin had no expressions".to_string(),
                )
            })?;
            let lowered = lower_bytes_concat_expr(last, rhs, planner)?;
            exprs.push(lowered);
            return Ok(PExpr::Begin {
                exprs,
                ty: Ty::BytesI32,
            });
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("bytes/concat lhs let had empty body".to_string())
            })?;
            let lowered = lower_bytes_concat_expr(last, rhs, planner)?;
            body.push(lowered);
            return Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::BytesI32,
            });
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::BytesI32,
        } => {
            let then_lowered = lower_bytes_concat_expr(*then_expr, rhs.clone(), planner)?;
            let else_lowered = lower_bytes_concat_expr(*else_expr, rhs, planner)?;
            return Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: Ty::BytesI32,
            });
        }
        other => other,
    };

    match rhs {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal(
                    "bytes/concat rhs begin had no expressions".to_string(),
                )
            })?;
            let lowered = lower_bytes_concat_expr(lhs, last, planner)?;
            exprs.push(lowered);
            Ok(PExpr::Begin {
                exprs,
                ty: Ty::BytesI32,
            })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("bytes/concat rhs let had empty body".to_string())
            })?;
            let lowered = lower_bytes_concat_expr(lhs, last, planner)?;
            body.push(lowered);
            Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::BytesI32,
            })
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::BytesI32,
        } => {
            let then_lowered = lower_bytes_concat_expr(lhs.clone(), *then_expr, planner)?;
            let else_lowered = lower_bytes_concat_expr(lhs, *else_expr, planner)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: Ty::BytesI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "bytes/concat currently requires stage2-known byte values".to_string(),
        )),
    }
}

fn lower_bytes_get_expr(
    lhs: PExpr,
    rhs: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let (Some(lhs_id), Some(rhs_n)) = (
        planner_const_bytes_id(planner, &lhs),
        planner_const_int_value(planner, &rhs),
    ) {
        return lower_bytes_get_const_pair(lhs, rhs, lhs_id, rhs_n, planner);
    }

    let lhs = match lhs {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("bytes/get lhs begin had no expressions".to_string())
            })?;
            let lowered = lower_bytes_get_expr(last, rhs, planner)?;
            exprs.push(lowered);
            return Ok(PExpr::Begin { exprs, ty: Ty::I64 });
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("bytes/get lhs let had empty body".to_string())
            })?;
            let lowered = lower_bytes_get_expr(last, rhs, planner)?;
            body.push(lowered);
            return Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::I64,
            });
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::BytesI32,
        } => {
            let then_lowered = lower_bytes_get_expr(*then_expr, rhs.clone(), planner)?;
            let else_lowered = lower_bytes_get_expr(*else_expr, rhs, planner)?;
            return Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: Ty::I64,
            });
        }
        other => other,
    };

    match rhs {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("bytes/get rhs begin had no expressions".to_string())
            })?;
            let lowered = lower_bytes_get_expr(lhs, last, planner)?;
            exprs.push(lowered);
            Ok(PExpr::Begin { exprs, ty: Ty::I64 })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("bytes/get rhs let had empty body".to_string())
            })?;
            let lowered = lower_bytes_get_expr(lhs, last, planner)?;
            body.push(lowered);
            Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::I64,
            })
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::I64,
        } => {
            let then_lowered = lower_bytes_get_expr(lhs.clone(), *then_expr, planner)?;
            let else_lowered = lower_bytes_get_expr(lhs, *else_expr, planner)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(then_lowered),
                else_expr: Box::new(else_lowered),
                cond_ty,
                ty: Ty::I64,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "bytes/get currently requires stage2-known bytes and int values".to_string(),
        )),
    }
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
        record_local_const_ids(planner, idx, &arg_expr);
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

fn is_safe_defs_only_rhs(t: &Term) -> bool {
    match t {
        Term::Nil
        | Term::Bool(_)
        | Term::Int(_)
        | Term::Str(_)
        | Term::Bytes(_)
        | Term::Symbol(_)
        | Term::Vector(_)
        | Term::Map(_)
            if term_const_data_term(t).is_some() =>
        {
            return true;
        }
        _ => {}
    }
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

#[cfg(test)]
#[path = "stage2_wasm/tests/mod.rs"]
mod tests;
