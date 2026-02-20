use super::*;

pub(super) fn lower_str_concat_const_pair(
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

pub(super) fn lower_str_join_const_pair(
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

pub(super) fn lower_str_repeat_const_pair(
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

pub(super) fn lower_bytes_join_const_parts(
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

pub(super) fn lower_bytes_concat_const_pair(
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

pub(super) fn lower_bytes_get_const_pair(
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

pub(super) fn lower_vec_get_const_pair(
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

pub(super) fn lower_str_repeat_expr(
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

pub(super) fn lower_str_concat_expr(
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

pub(super) fn lower_bytes_concat_expr(
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

pub(super) fn lower_bytes_get_expr(
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
