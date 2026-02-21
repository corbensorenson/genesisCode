use super::*;

#[path = "strings_bytes_escape_lowering.rs"]
mod strings_bytes_escape_lowering;

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
    lower_int_to_str_expr(arg, planner)
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
    lower_sym_to_str_expr(arg, planner)
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
    lower_sym_from_str_expr(arg, planner)
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
    lower_str_to_utf8_expr(arg, planner)
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
    lower_bytes_to_str_utf8_expr(arg, planner)
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
    lower_bytes_to_hex_expr(arg, planner)
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
    lower_bytes_from_hex_expr(arg, planner)
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

pub(super) fn decode_hex_bytes(s: &str) -> Result<Vec<u8>, Stage2CompileError> {
    if !s.len().is_multiple_of(2) {
        return Err(Stage2CompileError::Unsupported(
            "bytes/from-hex currently requires even-length hex strings".to_string(),
        ));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let hi = bytes[i];
        let lo = bytes[i + 1];
        let h = match hi {
            b'0'..=b'9' => hi - b'0',
            b'a'..=b'f' => hi - b'a' + 10,
            b'A'..=b'F' => hi - b'A' + 10,
            _ => {
                return Err(Stage2CompileError::Unsupported(
                    "bytes/from-hex currently requires valid hex strings".to_string(),
                ));
            }
        };
        let l = match lo {
            b'0'..=b'9' => lo - b'0',
            b'a'..=b'f' => lo - b'a' + 10,
            b'A'..=b'F' => lo - b'A' + 10,
            _ => {
                return Err(Stage2CompileError::Unsupported(
                    "bytes/from-hex currently requires valid hex strings".to_string(),
                ));
            }
        };
        out.push((h << 4) | l);
        i += 2;
    }
    Ok(out)
}

pub(super) fn lower_bytes_to_hex_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_bytes_id(planner, &arg) {
        let bs = planner_bytes_for_id(planner, id)?;
        let mut out = String::with_capacity(bs.len() * 2);
        for b in &bs {
            use std::fmt::Write;
            write!(&mut out, "{:02x}", b).map_err(|_| {
                Stage2CompileError::Internal("failed to format hex string".to_string())
            })?;
        }
        let out_id = planner.intern_string(&out)?;
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
                    "bytes/to-hex begin arg had no expressions".to_string(),
                )
            })?;
            let lowered = lower_bytes_to_hex_expr(last, planner)?;
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
                Stage2CompileError::Internal("bytes/to-hex let arg had empty body".to_string())
            })?;
            let lowered = lower_bytes_to_hex_expr(last, planner)?;
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
                    "bytes/to-hex currently requires stage2-known byte values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_bytes_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "bytes/to-hex currently requires stage2-known byte values".to_string(),
                ));
            };
            let then_bs = planner_bytes_for_id(planner, then_id)?;
            let else_bs = planner_bytes_for_id(planner, else_id)?;
            let mut then_hex = String::with_capacity(then_bs.len() * 2);
            for b in &then_bs {
                use std::fmt::Write;
                write!(&mut then_hex, "{:02x}", b).map_err(|_| {
                    Stage2CompileError::Internal("failed to format hex string".to_string())
                })?;
            }
            let mut else_hex = String::with_capacity(else_bs.len() * 2);
            for b in &else_bs {
                use std::fmt::Write;
                write!(&mut else_hex, "{:02x}", b).map_err(|_| {
                    Stage2CompileError::Internal("failed to format hex string".to_string())
                })?;
            }
            let then_out = planner.intern_string(&then_hex)?;
            let else_out = planner.intern_string(&else_hex)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Str(then_out)),
                else_expr: Box::new(PExpr::Str(else_out)),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "bytes/to-hex currently requires stage2-known byte values".to_string(),
        )),
    }
}

pub(super) fn lower_bytes_from_hex_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_string_id(planner, &arg) {
        let s = planner_string_for_id(planner, id)?;
        let out_id = planner.intern_bytes(&decode_hex_bytes(&s)?)?;
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
                    "bytes/from-hex begin arg had no expressions".to_string(),
                )
            })?;
            let lowered = lower_bytes_from_hex_expr(last, planner)?;
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
                Stage2CompileError::Internal("bytes/from-hex let arg had empty body".to_string())
            })?;
            let lowered = lower_bytes_from_hex_expr(last, planner)?;
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
                    "bytes/from-hex currently requires stage2-known string values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_string_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "bytes/from-hex currently requires stage2-known string values".to_string(),
                ));
            };
            let then_s = planner_string_for_id(planner, then_id)?;
            let else_s = planner_string_for_id(planner, else_id)?;
            let then_out = planner.intern_bytes(&decode_hex_bytes(&then_s)?)?;
            let else_out = planner.intern_bytes(&decode_hex_bytes(&else_s)?)?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Bytes(then_out)),
                else_expr: Box::new(PExpr::Bytes(else_out)),
                cond_ty,
                ty: Ty::BytesI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "bytes/from-hex currently requires stage2-known string values".to_string(),
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
