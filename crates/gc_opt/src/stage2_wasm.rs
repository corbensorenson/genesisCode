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

#[path = "stage2_wasm/callable_emit.rs"]
mod callable_emit;
#[path = "stage2_wasm/collections_lowering.rs"]
mod collections_lowering;
#[path = "stage2_wasm/pipeline_exec.rs"]
mod pipeline_exec;
#[path = "stage2_wasm/strings_bytes_lowering.rs"]
mod strings_bytes_lowering;

use callable_emit::*;
use collections_lowering::*;
use pipeline_exec::*;
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

fn lower_list_is_nil(arg: PExpr, planner: &mut Planner) -> Result<PExpr, Stage2CompileError> {
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

fn lower_data_tag(arg: PExpr, planner: &mut Planner) -> Result<PExpr, Stage2CompileError> {
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

fn planner_string_for_id(planner: &Planner, id: i32) -> Result<String, Stage2CompileError> {
    for (s, sid) in &planner.string_ids {
        if *sid == id {
            return Ok(s.clone());
        }
    }
    Err(Stage2CompileError::Internal(
        "string id missing from planner table".to_string(),
    ))
}

fn planner_symbol_for_id(planner: &Planner, id: i32) -> Result<String, Stage2CompileError> {
    for (s, sid) in &planner.symbol_ids {
        if *sid == id {
            return Ok(s.clone());
        }
    }
    Err(Stage2CompileError::Internal(
        "symbol id missing from planner table".to_string(),
    ))
}

fn planner_bytes_for_id(planner: &Planner, id: i32) -> Result<Vec<u8>, Stage2CompileError> {
    for (bs, bid) in &planner.bytes_ids {
        if *bid == id {
            return Ok(bs.clone());
        }
    }
    Err(Stage2CompileError::Internal(
        "bytes id missing from planner table".to_string(),
    ))
}

fn planner_const_string_id(planner: &Planner, expr: &PExpr) -> Option<i32> {
    const_string_id_with_map(expr, &planner.local_const_string_ids)
}

fn planner_const_int_value(planner: &Planner, expr: &PExpr) -> Option<i64> {
    const_int_value_with_map(expr, &planner.local_const_int_values)
}

fn planner_const_symbol_id(planner: &Planner, expr: &PExpr) -> Option<i32> {
    const_symbol_id_with_map(expr, &planner.local_const_symbol_ids)
}

fn planner_const_bytes_id(planner: &Planner, expr: &PExpr) -> Option<i32> {
    const_bytes_id_with_map(expr, &planner.local_const_bytes_ids)
}

fn lower_str_concat(
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

fn lower_str_repeat(
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

fn lower_bytes_concat(
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

fn lower_bytes_get(
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

fn ensure_scalar_cond_ty(cond_ty: Ty) -> Result<(), Stage2CompileError> {
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

fn term_const_vector_expr_with_aliases(
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

fn term_const_string_vector_ids_with_aliases(
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

fn term_const_bytes_vector_ids_with_aliases(
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

fn scalar_term_to_pexpr(
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

fn term_const_scalar_vector_exprs_with_aliases(
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

fn term_const_data_term(t: &Term) -> Option<Term> {
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

fn term_const_quoted_data_term(t: &Term) -> Option<Term> {
    let xs = t.as_proper_list()?;
    if xs.len() == 2 && matches!(xs[0], Term::Symbol(s) if s == "quote") {
        return Some(xs[1].clone());
    }
    None
}

fn term_const_data_expr(t: &Term) -> Option<Term> {
    term_const_quoted_data_term(t).or_else(|| term_const_data_term(t))
}

fn term_const_if_condition_expr(t: &Term) -> Option<Term> {
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

fn term_truthy(t: &Term) -> bool {
    !matches!(t, Term::Nil | Term::Bool(false))
}

fn term_const_i64_expr(t: &Term) -> Option<i64> {
    let Term::Int(i) = term_const_data_expr(t)? else {
        return None;
    };
    i.to_i64()
}

fn term_const_map_expr_with_aliases(
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

fn resolve_map_alias_term(
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

fn resolve_collection_aliases_term(
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

fn resolve_scalar_aliases_term(t: &Term, scalar_aliases: &BTreeMap<String, Term>) -> Term {
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

fn scalar_term_from_pexpr_const(planner: &Planner, expr: &PExpr) -> Option<Term> {
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

struct VecGetScope<'a> {
    env: &'a BTreeMap<String, Local>,
    global_env: &'a BTreeMap<String, Local>,
    fn_defs: &'a BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &'a BTreeMap<String, InlinableFnDef>,
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
    fn stage2_validates_if_truthiness_for_symbol_condition() {
        let src = r#"
          (if (quote :feature/on)
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
    fn stage2_validates_if_truthiness_for_string_and_bytes_condition() {
        let src = r#"
          (if "x"
            (if b"\x01"
              7
              8)
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
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("recursive function call is unsupported in stage2")),
            "{r:?}"
        );
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
    fn stage2_validates_list_is_nil_prim_for_nil_and_non_nil_scalars() {
        let src = r#"
          (def a (prim list/is-nil? nil))
          (def b (prim list/is-nil? false))
          (if a
            (if b 0 1)
            2)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Int));
    }

    #[test]
    fn stage2_validates_core_list_is_nil_wrapper_call() {
        let src = r#"
          (core/list::is-nil? nil)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_quote_symbol_via_core_eq() {
        let src = r#"
          (prim core/eq? (quote :k) (quote :k))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_quote_string_and_bytes_literals() {
        let src = r#"
          (if (prim core/eq? (quote "alpha") "alpha")
            (prim core/eq? (quote b"\xAA\xBB") b"\xAA\xBB")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_str_concat_and_len_prims_on_literals() {
        let src = r#"
          (def s (prim str/concat "hello, " "world"))
          (if (prim core/eq? s "hello, world")
            (prim int/eq? (prim str/len "hello, world") 12)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_bytes_concat_and_len_prims_on_literals() {
        let src = r#"
          (def b (prim bytes/concat b"\x01\x02" b"\x03"))
          (if (prim core/eq? b b"\x01\x02\x03")
            (prim int/eq? (prim bytes/len b"\x01\x02\x03") 3)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_str_and_bytes_wrapper_calls_on_literals() {
        let src = r#"
          (def s ((core/str::concat "a") "b"))
          (def b ((core/bytes::concat b"\xAA") b"\xBB"))
          (if (prim core/eq? s "ab")
            (if (prim core/eq? b b"\xAA\xBB")
              (if (prim int/eq? (core/str::len "abc") 3)
                (prim int/eq? (core/bytes::len b"\x10\x20\x30") 3)
                false)
              false)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_len_wrappers_on_def_bound_constant_values() {
        let src = r#"
          (def s ((core/str::concat "ab") "c"))
          (def b ((core/bytes::concat b"\x01") b"\x02\x03"))
          (if (prim int/eq? (core/str::len s) 3)
            (prim int/eq? (core/bytes::len b) 3)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_len_wrappers_on_let_bound_constant_values() {
        let src = r#"
          (let ((s ((core/str::concat "hel") "lo"))
                (b ((core/bytes::concat b"\xAA") b"\xBB")))
            (if (prim int/eq? (core/str::len s) 5)
              (prim int/eq? (core/bytes::len b) 2)
              false))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_concat_wrappers_on_bound_constant_values() {
        let src = r#"
          (def a "hello")
          (def b ", world")
          (def x b"\x01")
          (def y b"\x02\x03")
          (def s ((core/str::concat a) b))
          (def bs ((core/bytes::concat x) y))
          (if (prim core/eq? s "hello, world")
            (prim core/eq? bs b"\x01\x02\x03")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_len_wrappers_on_if_stable_constant_values() {
        let src = r#"
          (def s (if true "abc" "abc"))
          (def b (if true b"\x10\x20" b"\x10\x20"))
          (if (prim int/eq? (core/str::len s) 3)
            (prim int/eq? (core/bytes::len b) 2)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_len_prims_on_if_variant_constant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq? (prim str/len (if cond "abc" "abcd")) 3)
            (prim int/eq? (prim bytes/len (if cond b"\x10\x20" b"\x10\x20\x30")) 2)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_len_wrappers_on_if_variant_constant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq? (core/str::len (if cond "abc" "abcd")) 3)
            (prim int/eq? (core/bytes::len (if cond b"\x10\x20" b"\x10\x20\x30")) 2)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_len_wrappers_on_nested_let_if_variant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (core/str::len
                  (let ((x 1))
                    (if cond "abc" "abcd")))
                3)
            (prim int/eq?
              (core/bytes::len
                (let ((x 1))
                  (if cond b"\x10\x20" b"\x10\x20\x30")))
              2)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_int_to_str_prim_on_literals() {
        let src = r#"
          (if (prim core/eq? (prim int/to-str 42) "42")
            (prim core/eq? (prim int/to-str -7) "-7")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_int_to_str_wrapper_on_bound_constant_values() {
        let src = r#"
          (def n (prim int/add 40 2))
          (let ((m (prim int/sub n 10)))
            (if (prim core/eq? (core/int::to-str n) "42")
              (prim core/eq? (core/int::to-str m) "32")
              false))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_int_to_str_wrapper_on_if_variant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/int::to-str
                  (let ((x 1))
                    (if cond 42 420)))
                "42")
            (prim core/eq? (core/int::to-str (if cond -7 -70)) "-7")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_str_repeat_prim_on_literals() {
        let src = r#"
          (if (prim core/eq? (prim str/repeat "ab" 3) "ababab")
            (prim core/eq? (prim str/repeat "z" 0) "")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_str_repeat_wrapper_on_bound_constant_values() {
        let src = r#"
          (def s ((core/str::repeat "ab") 3))
          (def n (prim int/add 1 1))
          (if (prim core/eq? s "ababab")
            (prim core/eq? ((core/str::repeat "z") n) "zz")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_str_repeat_wrapper_on_if_variant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                ((core/str::repeat
                   (let ((x 1))
                     (if cond "ab" "abc")))
                 (if cond 2 3))
                "abab")
            (prim core/eq?
              ((core/str::repeat "z")
               (if cond 0 1))
              "")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_str_join_prim_on_literal_vectors() {
        let src = r#"
          (if (prim core/eq? (prim str/join ["a" "b" "c"] ",") "a,b,c")
            (prim core/eq? (prim str/join [] ",") "")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_str_join_wrapper_on_if_variant_vectors() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                ((core/str::join
                   (let ((x 1))
                     (if cond ["ab" "cd"] ["x" "y"])))
                 (if cond "-" ":"))
                "ab-cd")
            (prim core/eq?
              ((core/str::join
                 (if cond [] ["q"]))
               ",")
              "")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_bytes_join_prim_on_literal_vectors() {
        let src = r#"
          (if (prim core/eq? (prim bytes/join [b"\x01\x02" b"\xFF"]) b"\x01\x02\xFF")
            (prim core/eq? (prim bytes/join []) b"")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_bytes_join_wrapper_on_if_variant_vectors() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/bytes::join
                  (let ((x 1))
                    (if cond [b"\xAA" b"\xBB"] [b"\xCC"])))
                b"\xAA\xBB")
            (prim core/eq?
              (core/bytes::join
                (if cond [] [b"\x01"]))
              b"")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_vec_len_prim_on_literal_vectors() {
        let src = r#"
          (if (prim int/eq? (prim vec/len [10 20 30]) 3)
            (prim int/eq? (prim vec/len []) 0)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_vec_len_wrapper_on_if_variant_vectors() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (core/vec::len
                  (if cond [1 2 3] [4]))
                3)
            (prim int/eq?
              (core/vec::len
                (let ((x 1))
                  (if cond [] [0])))
              0)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_vec_len_on_let_bound_vector_alias() {
        let src = r#"
          (if (prim int/eq?
                (core/vec::len
                  (let ((v [1 2 3 4]))
                    v))
                4)
            (prim int/eq?
              (prim vec/len
                (let ((v (prim vec/push [8] 9)))
                  v))
              2)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_map_len_prim_on_literal_maps() {
        let src = r#"
          (if (prim int/eq? (prim map/len {:a 1 :b 2}) 2)
            (prim int/eq? (prim map/len {}) 0)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_map_len_wrapper_on_if_variant_maps() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (core/map::len
                  (if cond {:a 1 :b 2} {:z 9}))
                2)
            (prim int/eq?
              (core/map::len
                (let ((x 1))
                  (if cond {} {:k 1})))
              0)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_map_get_prim_on_literal_maps() {
        let src = r#"
          (if (prim int/eq? (prim map/get {:a 1 :b 2} (quote :a)) 1)
            (prim list/is-nil? (prim map/get {:a 1} (quote :z)))
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_map_get_wrapper_on_if_variant_maps_and_keys() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                ((core/map::get
                   (if cond {:a 7 :b 8} {:a 1 :b 2}))
                 (if cond (quote :a) (quote :b)))
                7)
            (prim list/is-nil?
              ((core/map::get
                 (let ((x 1))
                   (if cond {:k 1} {:m 2})))
               (if cond (quote :z) (quote :y))))
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_map_get_len_on_put_merge_constant_forms() {
        let src = r#"
          (if (prim int/eq?
                (prim map/get
                  (prim map/put {:a 1} (quote :b) 2)
                  (quote :b))
                2)
            (if (prim int/eq?
                  (prim map/len
                    (prim map/merge {:a 1} {:b 2 :c 3}))
                  3)
              (prim int/eq?
                (prim map/get
                  (((core/map::put {:x 1}) (quote :y)) 9)
                  (quote :y))
                9)
              false)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_collection_constant_composition_on_alias_sources() {
        let src = r#"
          (def v0 [1 2])
          (def v1 (prim vec/push v0 3))
          (def m0 {:a 1})
          (def m1 (prim map/put m0 (quote :b) 2))
          (def m2 (prim map/merge m1 {:c 3}))
          (if (prim int/eq? (prim vec/get v1 2) 3)
            (if (prim int/eq? (prim map/get m2 (quote :b)) 2)
              (prim int/eq? (core/map::len m2) 3)
              false)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_map_get_len_on_let_bound_map_aliases() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (prim map/get
                  (let ((m1 {:a 1 :b 2})
                        (m2 {:a 10 :b 20}))
                    (if cond m1 m2))
                  (quote :b))
                2)
            (if (prim int/eq?
                  (core/map::len
                    (let ((m1 (prim map/put {} (quote :x) 9))
                          (m2 (prim map/merge {:a 1} {:b 2})))
                      (if cond m1 m2)))
                  1)
              (prim list/is-nil?
                (prim map/get
                  (let ((m1 (prim map/merge {:a 1} {:b 2}))
                        (m2 {:y 0}))
                    (if cond m1 m2))
                  (quote :z)))
              false)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_collection_ops_on_def_bound_aliases() {
        let src = r#"
          (def v [1 2 3])
          (def m {:a 7 :b 8})
          (def parts ["a" "b"])
          (def bytes-parts [b"\x01" b"\x02"])
          (if (prim int/eq? (prim vec/get v 1) 2)
            (if (prim int/eq? (core/vec::len v) 3)
              (if (prim int/eq? ((core/map::get m) (quote :a)) 7)
                (if (prim int/eq? (core/map::len m) 2)
                  (if (prim core/eq? (core/str::join parts "-") "a-b")
                    (prim core/eq? (core/bytes::join bytes-parts) b"\x01\x02")
                    false)
                  false)
                false)
              false)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_collection_ops_on_def_bound_alias_chains() {
        let src = r#"
          (def v1 [1 2 3])
          (def v2 v1)
          (def v3 v2)
          (def m1 {:a 7 :b 8})
          (def m2 m1)
          (def m3 m2)
          (def parts1 ["a" "b"])
          (def parts2 parts1)
          (def parts3 parts2)
          (def bytes1 [b"\x01" b"\x02"])
          (def bytes2 bytes1)
          (def bytes3 bytes2)
          (if (prim int/eq? (prim vec/get v3 1) 2)
            (if (prim int/eq? (core/vec::len v3) 3)
              (if (prim int/eq? ((core/map::get m3) (quote :a)) 7)
                (if (prim int/eq? (core/map::len m3) 2)
                  (if (prim core/eq? (core/str::join parts3 "-") "a-b")
                    (prim core/eq? (core/bytes::join bytes3) b"\x01\x02")
                    false)
                  false)
                false)
              false)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_collection_ops_on_let_bound_alias_chains() {
        let src = r#"
          (if (prim int/eq?
                (prim vec/get
                  (let ((v1 [1 2 3])
                        (v2 v1)
                        (v3 v2))
                    v3)
                  2)
                3)
            (if (prim int/eq?
                  (prim map/get
                    (let ((m1 {:a 7 :b 8})
                          (m2 m1)
                          (m3 m2))
                      m3)
                    (quote :b))
                  8)
              (if (prim core/eq?
                    (prim str/join
                      (let ((s1 ["a" "b"])
                            (s2 s1)
                            (s3 s2))
                        s3)
                      "-")
                    "a-b")
                (prim core/eq?
                  (core/bytes::join
                    (let ((b1 [b"\x01" b"\x02"])
                          (b2 b1)
                          (b3 b2))
                      b3))
                  b"\x01\x02")
                false)
              false)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_generic_let_collection_alias_flow() {
        let src = r#"
          (let ((v [1 2 3])
                (m {:a 7 :b 8})
                (parts ["a" "b"])
                (bparts [b"\x01" b"\x02"]))
            (if (prim int/eq? (prim vec/get v 1) 2)
              (if (prim int/eq? (prim map/get m (quote :b)) 8)
                (if (prim core/eq? (prim str/join parts "-") "a-b")
                  (prim core/eq? (core/bytes::join bparts) b"\x01\x02")
                  false)
                false)
              false))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_defs_only_module_with_data_literal_rhs() {
        let src = r#"
          (def v [1 2 3])
          (def m {:a 1 :b 2})
          (def p '(x y))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
    }

    #[test]
    fn stage2_validates_vec_get_len_on_push_constant_forms() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                (prim vec/get
                  (if cond
                    (prim vec/push [1 2] 3)
                    (prim vec/push [1 2] 4))
                  (if cond 2 1))
                3)
            (if (prim int/eq?
                  (core/vec::len
                    (if cond
                      ((core/vec::push [7]) 10)
                      ((core/vec::push [8 9]) 10)))
                  2)
              (prim list/is-nil?
                (prim vec/get
                  ((core/vec::push []) 5)
                  9))
              false)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_join_on_vec_push_constant_forms() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (prim str/join
                  (if cond
                    (prim vec/push ["a"] "b")
                    (prim vec/push ["x"] "b"))
                  (if cond "-" ":"))
                "a-b")
            (prim core/eq?
              (core/bytes::join
                (if cond
                  ((core/vec::push [b"\x01"]) b"\x02")
                  ((core/vec::push [b"\xAA"]) b"\x02")))
              b"\x01\x02")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_join_on_let_bound_vector_aliases() {
        let src = r#"
          (if (prim core/eq?
                (prim str/join
                  (let ((parts ["a" "b"]))
                    parts)
                  "-")
                "a-b")
            (prim core/eq?
              (core/bytes::join
                (let ((parts (prim vec/push [b"\x01"] b"\x02")))
                  parts))
              b"\x01\x02")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_vec_get_prim_on_literal_vectors() {
        let src = r#"
          (if (prim int/eq? (prim vec/get [10 20 30] 1) 20)
            (prim list/is-nil? (prim vec/get [10] 5))
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_vec_get_wrapper_on_if_variant_vectors_and_indices() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                ((core/vec::get
                   (if cond [7 8] [9 10]))
                 (if cond 0 1))
                7)
            (prim list/is-nil?
              ((core/vec::get
                 (let ((x 1))
                   (if cond [1] [2])))
               (if cond 5 7)))
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_vec_get_on_let_bound_vector_alias() {
        let src = r#"
          (if (prim int/eq?
                (prim vec/get
                  (let ((v [5 6 7]))
                    v)
                  1)
                6)
            (prim list/is-nil?
              (prim vec/get
                (let ((v (prim vec/push [] 9)))
                  v)
                5))
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_bytes_get_prim_on_literals() {
        let src = r#"
          (if (prim int/eq? (prim bytes/get b"\x00\x7f\xff" 2) 255)
            (prim int/eq? (prim bytes/get b"AZ" 0) 65)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_bytes_get_wrapper_on_bound_constant_values() {
        let src = r#"
          (def bs b"\x10\x20\x30")
          (def i (prim int/add 1 1))
          (if (prim int/eq? ((core/bytes::get bs) i) 48)
            (prim int/eq? ((core/bytes::get bs) 0) 16)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_bytes_get_wrapper_on_if_variant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim int/eq?
                ((core/bytes::get
                   (let ((x 1))
                     (if cond b"\x01\x02" b"\x03\x04")))
                 (if cond 1 0))
                2)
            (prim int/eq?
              ((core/bytes::get b"\x09\x08")
               (if cond 0 1))
              9)
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_coreform_escape_prims_on_literals() {
        let src = r#"
          (if (prim core/eq? (prim coreform/escape-str "a\n\t\"\\") "a\\n\\t\\\"\\\\")
            (prim core/eq? (prim coreform/escape-bytes b"\x00\xFF") "\\x00\\xFF")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_coreform_escape_wrappers_on_bound_constant_values() {
        let src = r#"
          (def s (core/coreform::escape-str "x\n"))
          (def b (core/coreform::escape-bytes b"\n"))
          (if (prim core/eq? s "x\\n")
            (prim core/eq? b "\\n")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_coreform_escape_wrappers_on_if_variant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/coreform::escape-str
                  (if cond "a\n" "b\t"))
                "a\\n")
            (prim core/eq?
              (core/coreform::escape-bytes
                (let ((x 1))
                  (if cond b"\x00" b"\xFF")))
              "\\x00")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_sym_string_conversion_prims_on_literals() {
        let src = r#"
          (if (prim core/eq? (prim sym/to-str (quote :alpha/ns::k)) ":alpha/ns::k")
            (prim sym/eq? (prim sym/from-str ":alpha/ns::k") (quote :alpha/ns::k))
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_sym_string_wrapper_conversion_on_bound_constant_values() {
        let src = r#"
          (def s (core/sym::to-str (quote :alpha/ns::k)))
          (def k (core/sym::from-str s))
          (if ((core/sym::eq? k) (quote :alpha/ns::k))
            (prim core/eq? s ":alpha/ns::k")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_sym_string_wrapper_conversion_on_if_variant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/sym::to-str
                  (let ((x 1))
                    (if cond (quote :alpha) (quote :beta))))
                ":alpha")
            ((core/sym::eq?
               (core/sym::from-str
                 (if cond ":alpha" ":beta")))
             (quote :alpha))
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_utf8_conversion_prims_on_literals() {
        let src = r#"
          (if (prim core/eq? (prim bytes/to-str-utf8 (prim str/to-bytes-utf8 "alpha")) "alpha")
            (prim core/eq? (prim str/to-bytes-utf8 (prim bytes/to-str-utf8 b"\xCE\xB1")) b"\xCE\xB1")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_utf8_wrapper_conversion_on_bound_constant_values() {
        let src = r#"
          (def b (core/str::to-utf8 "hello"))
          (def s (core/str::from-utf8 b))
          (if (prim core/eq? s "hello")
            (prim core/eq? b b"hello")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_utf8_wrapper_conversion_on_if_variant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/str::from-utf8
                  (let ((x 1))
                    (if cond b"alpha" b"beta")))
                "alpha")
            (prim core/eq?
              (core/str::to-utf8
                (if cond "alpha" "beta"))
              b"alpha")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_hex_conversion_prims_on_literals() {
        let src = r#"
          (if (prim core/eq? (prim bytes/to-hex b"\x00\xff") "00ff")
            (prim core/eq? (prim bytes/from-hex "00ff") b"\x00\xff")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_hex_wrapper_conversion_on_bound_constant_values() {
        let src = r#"
          (def hx (core/bytes::to-hex b"\xAA\xBB"))
          (def bs (core/bytes::from-hex hx))
          (if (prim core/eq? hx "aabb")
            (prim core/eq? bs b"\xAA\xBB")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_hex_wrapper_conversion_on_if_variant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                (core/bytes::to-hex
                  (let ((x 1))
                    (if cond b"\xAA\xBB" b"\xCC\xDD")))
                "aabb")
            (prim core/eq?
              (core/bytes::from-hex
                (if cond "aabb" "ccdd"))
              b"\xAA\xBB")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_concat_prims_on_if_variant_constant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq? (prim str/concat (if cond "ab" "abc") "!") "ab!")
            (prim core/eq? (prim bytes/concat (if cond b"\x01" b"\x01\x02") b"\xFF") b"\x01\xFF")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_concat_wrappers_on_if_variant_constant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq? ((core/str::concat (if cond "ab" "abc")) "!") "ab!")
            (prim core/eq? ((core/bytes::concat (if cond b"\x01" b"\x01\x02")) b"\xFF") b"\x01\xFF")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_concat_wrappers_on_nested_let_if_variant_values() {
        let src = r#"
          (def cond (prim int/lt? 0 1))
          (if (prim core/eq?
                ((core/str::concat
                   (let ((x 1))
                     (if cond "ab" "abc")))
                 (begin 0 "!"))
                "ab!")
            (prim core/eq?
              ((core/bytes::concat
                 (let ((x 1))
                   (if cond b"\x01" b"\x01\x02")))
               (begin 0 b"\xFF"))
              b"\x01\xFF")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_concat_prims_on_both_sides_if_variant_constants() {
        let src = r#"
          (def lhs-cond (prim int/lt? 0 1))
          (def rhs-cond (prim int/lt? 1 2))
          (if (prim core/eq?
                (prim str/concat
                  (if lhs-cond "ab" "abc")
                  (if rhs-cond "!" "!!"))
                "ab!")
            (prim core/eq?
              (prim bytes/concat
                (if lhs-cond b"\x01" b"\x01\x02")
                (if rhs-cond b"\xFF" b"\xFE"))
              b"\x01\xFF")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_concat_wrappers_on_both_sides_if_variant_constants() {
        let src = r#"
          (def lhs-cond (prim int/lt? 0 1))
          (def rhs-cond (prim int/lt? 1 2))
          (if (prim core/eq?
                ((core/str::concat (if lhs-cond "ab" "abc"))
                 (if rhs-cond "!" "!!"))
                "ab!")
            (prim core/eq?
              ((core/bytes::concat (if lhs-cond b"\x01" b"\x01\x02"))
               (if rhs-cond b"\xFF" b"\xFE"))
              b"\x01\xFF")
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_symbol_top_level_result() {
        let src = r#"
          (quote :hello/world::flag)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Sym));
    }

    #[test]
    fn stage2_validates_sym_eq_prim_and_wrapper_with_data_tag() {
        let src = r#"
          (def t (prim data/tag 7))
          (def a (prim sym/eq? t (quote :int)))
          ((core/sym::eq? (core/data::tag nil)) (quote :nil))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_data_tag_for_string_and_bytes() {
        let src = r#"
          (def a ((core/sym::eq? (core/data::tag "s")) (quote :str)))
          (if a
            ((core/sym::eq? (core/data::tag b"\x00")) (quote :bytes))
            false)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Bool));
    }

    #[test]
    fn stage2_validates_string_and_bytes_top_level_results() {
        let src_str = r#"
          "hello/world"
        "#;
        let forms_str = canonicalize_module(parse_module(src_str).unwrap()).unwrap();
        let r_str = stage2_validation_report(&forms_str);
        assert!(r_str.supported, "{r_str:?}");
        assert!(r_str.ok, "{r_str:?}");
        assert_eq!(r_str.value_kind, Some(Stage2ValueKind::Str));

        let src_bytes = r#"
          b"\x10\x20"
        "#;
        let forms_bytes = canonicalize_module(parse_module(src_bytes).unwrap()).unwrap();
        let r_bytes = stage2_validation_report(&forms_bytes);
        assert!(r_bytes.supported, "{r_bytes:?}");
        assert!(r_bytes.ok, "{r_bytes:?}");
        assert_eq!(r_bytes.value_kind, Some(Stage2ValueKind::Bytes));
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
    fn stage2_validates_defs_only_module_with_collection_composition_rhs() {
        let src = r#"
          (def base {:a 1})
          (def merged (prim map/merge base {:b 2}))
          (def updated (prim map/put merged (quote :c) 3))
          (def v0 [1 2])
          (def v1 (prim vec/push v0 3))
          (def v2 ((core/vec::push v1) 4))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
    }

    #[test]
    fn stage2_validates_defs_only_module_with_if_selected_collection_rhs() {
        let src = r#"
          (def selected-map (if true {:a 1} {:b 2}))
          (def selected-vec (if false [1 2] [3 4]))
          (def merged (prim map/put selected-map (quote :c) 3))
          (def pushed (prim vec/push selected-vec 5))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
    }

    #[test]
    fn stage2_validates_defs_only_module_with_if_selected_collection_rhs_via_prim_condition() {
        let src = r#"
          (def selected-map (if (prim int/lt? 0 1) {:a 1} {:b 2}))
          (def selected-vec (if ((core/int::eq? 1) 2) [1 2] [3 4]))
          (def merged (prim map/put selected-map (quote :c) 3))
          (def pushed (prim vec/push selected-vec 5))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let r = stage2_validation_report(&forms);
        assert!(r.supported, "{r:?}");
        assert!(r.ok, "{r:?}");
        assert_eq!(r.value_kind, Some(Stage2ValueKind::Nil));
    }

    #[test]
    fn stage2_validates_defs_only_module_with_if_selected_collection_rhs_via_def_condition_aliases()
    {
        let src = r#"
          (def cond0 (prim int/lt? 0 1))
          (def cond1 cond0)
          (def selected-map (if cond1 {:a 1} {:b 2}))
          (def selected-vec (if cond1 [1 2] [3 4]))
          (def merged (prim map/put selected-map (quote :c) 3))
          (def pushed (prim vec/push selected-vec 5))
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
          (def x (if cond {:a 1} {:b 2}))
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
