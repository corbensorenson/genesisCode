use super::*;

#[path = "strings_bytes_escape_lowering.rs"]
mod strings_bytes_escape_lowering;
#[path = "strings_bytes_hex_lowering.rs"]
mod strings_bytes_hex_lowering;
#[path = "strings_bytes_scalar_lowering.rs"]
mod strings_bytes_scalar_lowering;

pub(super) fn lower_bytes_len(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::BytesI32 {
        return Err(Stage2CompileError::Unsupported(
            "bytes/len expects bytes in stage2".to_string(),
        ));
    }
    lower_bytes_len_expr(arg, planner)
}

pub(super) fn lower_str_len(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::StrI32 {
        return Err(Stage2CompileError::Unsupported(
            "str/len expects string in stage2".to_string(),
        ));
    }
    lower_str_len_expr(arg, planner)
}

pub(super) fn lower_int_to_str(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::I64 {
        return Err(Stage2CompileError::Unsupported(
            "int/to-str expects int in stage2".to_string(),
        ));
    }
    strings_bytes_scalar_lowering::lower_int_to_str_expr(arg, planner)
}

pub(super) fn lower_sym_to_str(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::SymI32 {
        return Err(Stage2CompileError::Unsupported(
            "sym/to-str expects symbol in stage2".to_string(),
        ));
    }
    strings_bytes_scalar_lowering::lower_sym_to_str_expr(arg, planner)
}

pub(super) fn lower_sym_from_str(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::StrI32 {
        return Err(Stage2CompileError::Unsupported(
            "sym/from-str expects string in stage2".to_string(),
        ));
    }
    strings_bytes_scalar_lowering::lower_sym_from_str_expr(arg, planner)
}

pub(super) fn lower_str_to_utf8(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::StrI32 {
        return Err(Stage2CompileError::Unsupported(
            "str/to-bytes-utf8 expects string in stage2".to_string(),
        ));
    }
    strings_bytes_scalar_lowering::lower_str_to_utf8_expr(arg, planner)
}

pub(super) fn lower_bytes_to_str_utf8(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::BytesI32 {
        return Err(Stage2CompileError::Unsupported(
            "bytes/to-str-utf8 expects bytes in stage2".to_string(),
        ));
    }
    strings_bytes_scalar_lowering::lower_bytes_to_str_utf8_expr(arg, planner)
}

pub(super) fn lower_bytes_to_hex(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::BytesI32 {
        return Err(Stage2CompileError::Unsupported(
            "bytes/to-hex expects bytes in stage2".to_string(),
        ));
    }
    strings_bytes_hex_lowering::lower_bytes_to_hex_expr(arg, planner)
}

pub(super) fn lower_bytes_from_hex(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::StrI32 {
        return Err(Stage2CompileError::Unsupported(
            "bytes/from-hex expects string in stage2".to_string(),
        ));
    }
    strings_bytes_hex_lowering::lower_bytes_from_hex_expr(arg, planner)
}

pub(super) fn lower_coreform_escape_str(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::StrI32 {
        return Err(Stage2CompileError::Unsupported(
            "coreform/escape-str expects string in stage2".to_string(),
        ));
    }
    lower_coreform_escape_str_expr(arg, planner)
}

pub(super) fn lower_coreform_escape_bytes(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::BytesI32 {
        return Err(Stage2CompileError::Unsupported(
            "coreform/escape-bytes expects bytes in stage2".to_string(),
        ));
    }
    lower_coreform_escape_bytes_expr(arg, planner)
}

pub(super) fn string_len_i64_for_id(planner: &Planner, id: i32) -> Result<i64, Stage2CompileError> {
    let len = planner_string_for_id(planner, id)?.len();
    i64::try_from(len).map_err(|_| {
        Stage2CompileError::Unsupported("str/len result out of i64 range in stage2".to_string())
    })
}

pub(super) fn bytes_len_i64_for_id(planner: &Planner, id: i32) -> Result<i64, Stage2CompileError> {
    let len = planner_bytes_for_id(planner, id)?.len();
    i64::try_from(len).map_err(|_| {
        Stage2CompileError::Unsupported("bytes/len result out of i64 range in stage2".to_string())
    })
}

pub(super) fn lower_str_len_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_string_id(planner, &arg) {
        let n = string_len_i64_for_id(planner, id)?;
        let idx = planner.alloc_local(Ty::StrI32)?;
        return Ok(PExpr::Let {
            bindings: vec![LetBinding { idx, expr: arg }],
            body: vec![PExpr::Int(n)],
            ty: Ty::I64,
        });
    }
    match arg {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/len begin arg had no expressions".to_string())
            })?;
            let lowered = lower_str_len_expr(last, planner)?;
            exprs.push(lowered);
            Ok(PExpr::Begin { exprs, ty: Ty::I64 })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/len let arg had empty body".to_string())
            })?;
            let lowered = lower_str_len_expr(last, planner)?;
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
            ty: Ty::StrI32,
        } => {
            let Some(then_id) = planner_const_string_id(planner, &then_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "str/len currently requires stage2-known string values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_string_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "str/len currently requires stage2-known string values".to_string(),
                ));
            };
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Int(string_len_i64_for_id(planner, then_id)?)),
                else_expr: Box::new(PExpr::Int(string_len_i64_for_id(planner, else_id)?)),
                cond_ty,
                ty: Ty::I64,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "str/len currently requires stage2-known string values".to_string(),
        )),
    }
}

pub(super) fn lower_bytes_len_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_bytes_id(planner, &arg) {
        let n = bytes_len_i64_for_id(planner, id)?;
        let idx = planner.alloc_local(Ty::BytesI32)?;
        return Ok(PExpr::Let {
            bindings: vec![LetBinding { idx, expr: arg }],
            body: vec![PExpr::Int(n)],
            ty: Ty::I64,
        });
    }
    match arg {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("bytes/len begin arg had no expressions".to_string())
            })?;
            let lowered = lower_bytes_len_expr(last, planner)?;
            exprs.push(lowered);
            Ok(PExpr::Begin { exprs, ty: Ty::I64 })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("bytes/len let arg had empty body".to_string())
            })?;
            let lowered = lower_bytes_len_expr(last, planner)?;
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
            ty: Ty::BytesI32,
        } => {
            let Some(then_id) = planner_const_bytes_id(planner, &then_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "bytes/len currently requires stage2-known byte values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_bytes_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "bytes/len currently requires stage2-known byte values".to_string(),
                ));
            };
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Int(bytes_len_i64_for_id(planner, then_id)?)),
                else_expr: Box::new(PExpr::Int(bytes_len_i64_for_id(planner, else_id)?)),
                cond_ty,
                ty: Ty::I64,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "bytes/len currently requires stage2-known byte values".to_string(),
        )),
    }
}

pub(super) fn lower_coreform_escape_str_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    strings_bytes_escape_lowering::lower_coreform_escape_str_expr(arg, planner)
}

pub(super) fn lower_coreform_escape_bytes_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    strings_bytes_escape_lowering::lower_coreform_escape_bytes_expr(arg, planner)
}
