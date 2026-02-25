use super::*;

pub(super) fn lower_int_to_str_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(n) = planner_const_int_value(planner, &arg) {
        let out_id = planner.intern_string(&n.to_string())?;
        let idx = planner.alloc_local(Ty::I64)?;
        return Ok(PExpr::Let {
            bindings: vec![LetBinding { idx, expr: arg }],
            body: vec![PExpr::Str(out_id)],
            ty: Ty::StrI32,
        });
    }
    match arg {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("int/to-str begin arg had no expressions".to_string())
            })?;
            let lowered = lower_int_to_str_expr(last, planner)?;
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
                Stage2CompileError::Internal("int/to-str let arg had empty body".to_string())
            })?;
            let lowered = lower_int_to_str_expr(last, planner)?;
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
            let Some(then_n) = planner_const_int_value(planner, &then_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "int/to-str currently requires stage2-known int values".to_string(),
                ));
            };
            let Some(else_n) = planner_const_int_value(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "int/to-str currently requires stage2-known int values".to_string(),
                ));
            };
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Str(planner.intern_string(&then_n.to_string())?)),
                else_expr: Box::new(PExpr::Str(planner.intern_string(&else_n.to_string())?)),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "int/to-str currently requires stage2-known int values".to_string(),
        )),
    }
}

pub(super) fn lower_sym_to_str_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_symbol_id(planner, &arg) {
        let out_id = planner.intern_string(&planner_symbol_for_id(planner, id)?)?;
        let idx = planner.alloc_local(Ty::SymI32)?;
        return Ok(PExpr::Let {
            bindings: vec![LetBinding { idx, expr: arg }],
            body: vec![PExpr::Str(out_id)],
            ty: Ty::StrI32,
        });
    }
    match arg {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("sym/to-str begin arg had no expressions".to_string())
            })?;
            let lowered = lower_sym_to_str_expr(last, planner)?;
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
                Stage2CompileError::Internal("sym/to-str let arg had empty body".to_string())
            })?;
            let lowered = lower_sym_to_str_expr(last, planner)?;
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
            ty: Ty::SymI32,
        } => {
            let Some(then_id) = planner_const_symbol_id(planner, &then_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "sym/to-str currently requires stage2-known symbol values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_symbol_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "sym/to-str currently requires stage2-known symbol values".to_string(),
                ));
            };
            let then_out = planner.intern_string(&planner_symbol_for_id(planner, then_id)?)?;
            let else_out = planner.intern_string(&planner_symbol_for_id(planner, else_id)?)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Str(then_out)),
                else_expr: Box::new(PExpr::Str(else_out)),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "sym/to-str currently requires stage2-known symbol values".to_string(),
        )),
    }
}

pub(super) fn lower_sym_from_str_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_string_id(planner, &arg) {
        let out_id = planner.intern_symbol(&planner_string_for_id(planner, id)?)?;
        let idx = planner.alloc_local(Ty::StrI32)?;
        return Ok(PExpr::Let {
            bindings: vec![LetBinding { idx, expr: arg }],
            body: vec![PExpr::Sym(out_id)],
            ty: Ty::SymI32,
        });
    }
    match arg {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal(
                    "sym/from-str begin arg had no expressions".to_string(),
                )
            })?;
            let lowered = lower_sym_from_str_expr(last, planner)?;
            exprs.push(lowered);
            Ok(PExpr::Begin {
                exprs,
                ty: Ty::SymI32,
            })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("sym/from-str let arg had empty body".to_string())
            })?;
            let lowered = lower_sym_from_str_expr(last, planner)?;
            body.push(lowered);
            Ok(PExpr::Let {
                bindings,
                body,
                ty: Ty::SymI32,
            })
        }
        PExpr::If {
            cond,
            then_expr,
            else_expr,
            cond_ty,
            ty: Ty::StrI32,
        } => {
            let Some(then_id) = planner_const_string_id(planner, &then_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "sym/from-str currently requires stage2-known string values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_string_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "sym/from-str currently requires stage2-known string values".to_string(),
                ));
            };
            let then_out = planner.intern_symbol(&planner_string_for_id(planner, then_id)?)?;
            let else_out = planner.intern_symbol(&planner_string_for_id(planner, else_id)?)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Sym(then_out)),
                else_expr: Box::new(PExpr::Sym(else_out)),
                cond_ty,
                ty: Ty::SymI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "sym/from-str currently requires stage2-known string values".to_string(),
        )),
    }
}

pub(super) fn lower_str_to_utf8_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_string_id(planner, &arg) {
        let out_id = planner.intern_bytes(planner_string_for_id(planner, id)?.as_bytes())?;
        let idx = planner.alloc_local(Ty::StrI32)?;
        return Ok(PExpr::Let {
            bindings: vec![LetBinding { idx, expr: arg }],
            body: vec![PExpr::Bytes(out_id)],
            ty: Ty::BytesI32,
        });
    }
    match arg {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal(
                    "str/to-bytes-utf8 begin arg had no expressions".to_string(),
                )
            })?;
            let lowered = lower_str_to_utf8_expr(last, planner)?;
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
                Stage2CompileError::Internal("str/to-bytes-utf8 let arg had empty body".to_string())
            })?;
            let lowered = lower_str_to_utf8_expr(last, planner)?;
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
            ty: Ty::StrI32,
        } => {
            let Some(then_id) = planner_const_string_id(planner, &then_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "str/to-bytes-utf8 currently requires stage2-known string values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_string_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "str/to-bytes-utf8 currently requires stage2-known string values".to_string(),
                ));
            };
            let then_out =
                planner.intern_bytes(planner_string_for_id(planner, then_id)?.as_bytes())?;
            let else_out =
                planner.intern_bytes(planner_string_for_id(planner, else_id)?.as_bytes())?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Bytes(then_out)),
                else_expr: Box::new(PExpr::Bytes(else_out)),
                cond_ty,
                ty: Ty::BytesI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "str/to-bytes-utf8 currently requires stage2-known string values".to_string(),
        )),
    }
}

pub(super) fn lower_bytes_to_str_utf8_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_bytes_id(planner, &arg) {
        let bs = planner_bytes_for_id(planner, id)?;
        let decoded = String::from_utf8(bs).map_err(|_| {
            Stage2CompileError::Unsupported(
                "bytes/to-str-utf8 currently requires valid UTF-8 byte values".to_string(),
            )
        })?;
        let out_id = planner.intern_string(&decoded)?;
        let idx = planner.alloc_local(Ty::BytesI32)?;
        return Ok(PExpr::Let {
            bindings: vec![LetBinding { idx, expr: arg }],
            body: vec![PExpr::Str(out_id)],
            ty: Ty::StrI32,
        });
    }
    match arg {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal(
                    "bytes/to-str-utf8 begin arg had no expressions".to_string(),
                )
            })?;
            let lowered = lower_bytes_to_str_utf8_expr(last, planner)?;
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
                Stage2CompileError::Internal("bytes/to-str-utf8 let arg had empty body".to_string())
            })?;
            let lowered = lower_bytes_to_str_utf8_expr(last, planner)?;
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
            ty: Ty::BytesI32,
        } => {
            let Some(then_id) = planner_const_bytes_id(planner, &then_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "bytes/to-str-utf8 currently requires stage2-known byte values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_bytes_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "bytes/to-str-utf8 currently requires stage2-known byte values".to_string(),
                ));
            };
            let then_decoded =
                String::from_utf8(planner_bytes_for_id(planner, then_id)?).map_err(|_| {
                    Stage2CompileError::Unsupported(
                        "bytes/to-str-utf8 currently requires valid UTF-8 byte values".to_string(),
                    )
                })?;
            let else_decoded =
                String::from_utf8(planner_bytes_for_id(planner, else_id)?).map_err(|_| {
                    Stage2CompileError::Unsupported(
                        "bytes/to-str-utf8 currently requires valid UTF-8 byte values".to_string(),
                    )
                })?;
            let then_out = planner.intern_string(&then_decoded)?;
            let else_out = planner.intern_string(&else_decoded)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Str(then_out)),
                else_expr: Box::new(PExpr::Str(else_out)),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "bytes/to-str-utf8 currently requires stage2-known byte values".to_string(),
        )),
    }
}
