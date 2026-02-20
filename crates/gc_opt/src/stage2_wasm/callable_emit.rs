use super::*;

pub(super) fn flatten_application_chain(t: &Term) -> Option<(Term, Vec<Term>)> {
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

pub(super) fn resolve_callable_head(
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

pub(super) fn resolve_inlinable_symbol(
    sym: &str,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
    local_fn_defs: &BTreeMap<String, InlinableFnDef>,
) -> Option<InlinableFnDef> {
    if let Some(existing) = local_fn_defs.get(sym) {
        return Some(existing.clone());
    }
    resolve_global_inlinable_symbol(sym, fn_defs)
}

pub(super) fn resolve_global_inlinable_symbol(
    sym: &str,
    fn_defs: &BTreeMap<String, InlinableFnDef>,
) -> Option<InlinableFnDef> {
    if let Some(existing) = fn_defs.get(sym) {
        return Some(existing.clone());
    }
    builtin_inlinable_fn(sym)
}

pub(super) fn builtin_inlinable_fn(sym: &str) -> Option<InlinableFnDef> {
    if sym == "core/data::tag" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("data/tag".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/sym::eq?" {
        let body = Term::list(vec![
            Term::Symbol("fn".to_string()),
            Term::list(vec![Term::Symbol("b".to_string())]),
            Term::list(vec![
                Term::Symbol("prim".to_string()),
                Term::Symbol("sym/eq?".to_string()),
                Term::Symbol("a".to_string()),
                Term::Symbol("b".to_string()),
            ]),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/sym::to-str" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("sym/to-str".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/sym::from-str" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("sym/from-str".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/str::to-utf8" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("str/to-bytes-utf8".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/str::from-utf8" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("bytes/to-str-utf8".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/coreform::escape-str" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("coreform/escape-str".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/coreform::escape-bytes" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("coreform/escape-bytes".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/bytes::to-hex" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("bytes/to-hex".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/bytes::from-hex" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("bytes/from-hex".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/list::is-nil?" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("list/is-nil?".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/bytes::len" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("bytes/len".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/vec::len" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("vec/len".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/map::len" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("map/len".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/bytes::join" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("bytes/join".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/str::len" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("str/len".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    if sym == "core/int::to-str" {
        let body = Term::list(vec![
            Term::Symbol("prim".to_string()),
            Term::Symbol("int/to-str".to_string()),
            Term::Symbol("a".to_string()),
        ]);
        return Some(InlinableFnDef {
            param: "a".to_string(),
            body,
            capture: FnCapture::GlobalFrame,
        });
    }
    let prim = match sym {
        "core/int::add" => "int/add",
        "core/int::sub" => "int/sub",
        "core/int::mul" => "int/mul",
        "core/int::eq?" => "int/eq?",
        "core/int::lt?" => "int/lt?",
        "core/eq?" => "core/eq?",
        "core/str::concat" => "str/concat",
        "core/str::join" => "str/join",
        "core/str::repeat" => "str/repeat",
        "core/map::get" => "map/get",
        "core/vec::get" => "vec/get",
        "core/bytes::get" => "bytes/get",
        "core/bytes::concat" => "bytes/concat",
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

pub(super) fn desugar_fn_literal_to_unary(
    t: &Term,
) -> Result<Option<(String, Term)>, Stage2CompileError> {
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

pub(super) fn infer_prim(op: &str, a: Ty, b: Ty) -> Result<(PrimOp, Ty), Stage2CompileError> {
    match op {
        "int/add" | "int/sub" | "int/mul" => {
            if a == Ty::I64 && b == Ty::I64 {
                let prim = match op {
                    "int/add" => PrimOp::Add,
                    "int/sub" => PrimOp::Sub,
                    "int/mul" => PrimOp::Mul,
                    _ => {
                        return Err(Stage2CompileError::Unsupported(format!(
                            "unsupported primitive op: {op}"
                        )))
                    }
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
                    _ => {
                        return Err(Stage2CompileError::Unsupported(format!(
                            "unsupported primitive op: {op}"
                        )))
                    }
                };
                Ok((prim, Ty::BoolI32))
            } else {
                Err(Stage2CompileError::Unsupported(format!(
                    "{op} expects int arguments"
                )))
            }
        }
        "sym/eq?" => {
            if a == Ty::SymI32 && b == Ty::SymI32 {
                Ok((PrimOp::EqI32, Ty::BoolI32))
            } else {
                Err(Stage2CompileError::Unsupported(
                    "sym/eq? expects symbol arguments in stage2".to_string(),
                ))
            }
        }
        "core/eq?" => {
            match (a, b) {
                (Ty::I64, Ty::I64) => Ok((PrimOp::EqI64, Ty::BoolI32)),
                (Ty::BoolI32, Ty::BoolI32)
                | (Ty::NilI32, Ty::NilI32)
                | (Ty::SymI32, Ty::SymI32)
                | (Ty::StrI32, Ty::StrI32)
                | (Ty::BytesI32, Ty::BytesI32) => Ok((PrimOp::EqI32, Ty::BoolI32)),
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

pub(super) fn match_curried_wrapper_call(xs: &[&Term]) -> Option<(&'static str, Term, Term)> {
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
            "core/str::concat" => "str/concat",
            "core/str::join" => "str/join",
            "core/str::repeat" => "str/repeat",
            "core/map::get" => "map/get",
            "core/vec::get" => "vec/get",
            "core/bytes::get" => "bytes/get",
            "core/bytes::concat" => "bytes/concat",
            _ => return None,
        },
        _ => return None,
    };
    Some((op, inner[1].clone(), xs[1].clone()))
}

pub(super) fn emit_expr(f: &mut Function, expr: &PExpr) -> Result<Ty, Stage2CompileError> {
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
        PExpr::Sym(id) => {
            f.instruction(&Instruction::I32Const(*id));
            Ok(Ty::SymI32)
        }
        PExpr::Str(id) => {
            f.instruction(&Instruction::I32Const(*id));
            Ok(Ty::StrI32)
        }
        PExpr::Bytes(id) => {
            f.instruction(&Instruction::I32Const(*id));
            Ok(Ty::BytesI32)
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
                (PrimOp::EqI32, Ty::SymI32, Ty::SymI32) => (Instruction::I32Eq, Ty::BoolI32),
                (PrimOp::EqI32, Ty::StrI32, Ty::StrI32) => (Instruction::I32Eq, Ty::BoolI32),
                (PrimOp::EqI32, Ty::BytesI32, Ty::BytesI32) => (Instruction::I32Eq, Ty::BoolI32),
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
                Ty::SymI32 => {
                    f.instruction(&Instruction::Drop);
                    f.instruction(&Instruction::I32Const(1));
                }
                Ty::StrI32 => {
                    f.instruction(&Instruction::Drop);
                    f.instruction(&Instruction::I32Const(1));
                }
                Ty::BytesI32 => {
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

pub(super) fn val_ty(t: Ty) -> ValType {
    match t {
        Ty::I64 => ValType::I64,
        Ty::BoolI32 => ValType::I32,
        Ty::NilI32 => ValType::I32,
        Ty::SymI32 => ValType::I32,
        Ty::StrI32 => ValType::I32,
        Ty::BytesI32 => ValType::I32,
    }
}
