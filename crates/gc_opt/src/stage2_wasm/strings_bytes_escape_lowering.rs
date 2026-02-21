use super::*;

pub(super) fn escape_coreform_str_literal(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub(super) fn escape_coreform_bytes_literal(b: &[u8]) -> String {
    let mut out = String::new();
    for &x in b {
        match x {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7E => out.push(x as char),
            _ => out.push_str(&format!("\\x{:02X}", x)),
        }
    }
    out
}

pub(super) fn lower_coreform_escape_str_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_string_id(planner, &arg) {
        let out_id = planner.intern_string(&escape_coreform_str_literal(
            &planner_string_for_id(planner, id)?,
        ))?;
        let idx = planner.alloc_local(Ty::StrI32)?;
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
                    "coreform/escape-str begin arg had no expressions".to_string(),
                )
            })?;
            let lowered = lower_coreform_escape_str_expr(last, planner)?;
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
                Stage2CompileError::Internal(
                    "coreform/escape-str let arg had empty body".to_string(),
                )
            })?;
            let lowered = lower_coreform_escape_str_expr(last, planner)?;
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
            let Some(then_id) = planner_const_string_id(planner, &then_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "coreform/escape-str currently requires stage2-known string values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_string_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "coreform/escape-str currently requires stage2-known string values".to_string(),
                ));
            };
            let then_out = planner.intern_string(&escape_coreform_str_literal(
                &planner_string_for_id(planner, then_id)?,
            ))?;
            let else_out = planner.intern_string(&escape_coreform_str_literal(
                &planner_string_for_id(planner, else_id)?,
            ))?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Str(then_out)),
                else_expr: Box::new(PExpr::Str(else_out)),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "coreform/escape-str currently requires stage2-known string values".to_string(),
        )),
    }
}

pub(super) fn lower_coreform_escape_bytes_expr(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_bytes_id(planner, &arg) {
        let out_id = planner.intern_string(&escape_coreform_bytes_literal(
            &planner_bytes_for_id(planner, id)?,
        ))?;
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
                    "coreform/escape-bytes begin arg had no expressions".to_string(),
                )
            })?;
            let lowered = lower_coreform_escape_bytes_expr(last, planner)?;
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
                Stage2CompileError::Internal(
                    "coreform/escape-bytes let arg had empty body".to_string(),
                )
            })?;
            let lowered = lower_coreform_escape_bytes_expr(last, planner)?;
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
                    "coreform/escape-bytes currently requires stage2-known byte values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_bytes_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "coreform/escape-bytes currently requires stage2-known byte values".to_string(),
                ));
            };
            let then_out = planner.intern_string(&escape_coreform_bytes_literal(
                &planner_bytes_for_id(planner, then_id)?,
            ))?;
            let else_out = planner.intern_string(&escape_coreform_bytes_literal(
                &planner_bytes_for_id(planner, else_id)?,
            ))?;
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Str(then_out)),
                else_expr: Box::new(PExpr::Str(else_out)),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "coreform/escape-bytes currently requires stage2-known byte values".to_string(),
        )),
    }
}
