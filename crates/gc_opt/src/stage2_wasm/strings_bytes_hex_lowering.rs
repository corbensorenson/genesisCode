use super::*;

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
