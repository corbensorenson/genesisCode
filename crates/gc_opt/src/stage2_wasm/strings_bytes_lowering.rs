use super::*;
use gc_kernel::text_profile::{grapheme_len, normalize_nfc, scalar_len};

#[path = "strings_bytes_escape_lowering.rs"]
mod strings_bytes_escape_lowering;
#[path = "strings_bytes_hex_lowering.rs"]
mod strings_bytes_hex_lowering;
#[path = "strings_bytes_scalar_lowering.rs"]
mod strings_bytes_scalar_lowering;

pub(super) enum UnicodeStringOp {
    Bytes,
    Scalars,
    Graphemes,
    Nfc,
}

impl UnicodeStringOp {
    pub(super) fn from_primitive(name: &str) -> Option<Self> {
        match name {
            "str/len" => Some(Self::Bytes),
            "str/scalar-len" => Some(Self::Scalars),
            "str/grapheme-len" => Some(Self::Graphemes),
            "str/nfc" => Some(Self::Nfc),
            _ => None,
        }
    }

    pub(super) fn from_wrapper(name: &str) -> Option<Self> {
        match name {
            "core/str::len" | "core/str::byte-len" => Some(Self::Bytes),
            "core/str::scalar-len" => Some(Self::Scalars),
            "core/str::grapheme-len" => Some(Self::Graphemes),
            "core/str::nfc" => Some(Self::Nfc),
            _ => None,
        }
    }

    pub(super) fn lower(
        self,
        arg: PExpr,
        planner: &mut Planner,
    ) -> Result<PExpr, Stage2CompileError> {
        match self {
            Self::Bytes => lower_str_len(arg, planner),
            Self::Scalars => lower_str_scalar_len(arg, planner),
            Self::Graphemes => lower_str_grapheme_len(arg, planner),
            Self::Nfc => lower_str_nfc(arg, planner),
        }
    }
}

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

pub(super) fn lower_str_scalar_len(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    lower_str_metric(arg, planner, StringMetric::Scalars)
}

pub(super) fn lower_str_grapheme_len(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    lower_str_metric(arg, planner, StringMetric::Graphemes)
}

pub(super) fn lower_str_nfc(
    arg: PExpr,
    planner: &mut Planner,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::StrI32 {
        return Err(Stage2CompileError::Unsupported(
            "str/nfc expects string in stage2".to_string(),
        ));
    }
    lower_str_nfc_expr(arg, planner)
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

#[derive(Clone, Copy)]
enum StringMetric {
    Bytes,
    Scalars,
    Graphemes,
}

impl StringMetric {
    fn op(self) -> &'static str {
        match self {
            Self::Bytes => "str/len",
            Self::Scalars => "str/scalar-len",
            Self::Graphemes => "str/grapheme-len",
        }
    }
}

fn lower_str_metric(
    arg: PExpr,
    planner: &mut Planner,
    metric: StringMetric,
) -> Result<PExpr, Stage2CompileError> {
    if arg.ty() != Ty::StrI32 {
        return Err(Stage2CompileError::Unsupported(format!(
            "{} expects string in stage2",
            metric.op()
        )));
    }
    lower_str_metric_expr(arg, planner, metric)
}

fn string_metric_i64_for_id(
    planner: &Planner,
    id: i32,
    metric: StringMetric,
) -> Result<i64, Stage2CompileError> {
    let string = planner_string_for_id(planner, id)?;
    let len = match metric {
        StringMetric::Bytes => string.len(),
        StringMetric::Scalars => scalar_len(&string),
        StringMetric::Graphemes => grapheme_len(&string),
    };
    i64::try_from(len).map_err(|_| {
        Stage2CompileError::Unsupported(format!(
            "{} result out of i64 range in stage2",
            metric.op()
        ))
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
    lower_str_metric_expr(arg, planner, StringMetric::Bytes)
}

fn lower_str_metric_expr(
    arg: PExpr,
    planner: &mut Planner,
    metric: StringMetric,
) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_string_id(planner, &arg) {
        let n = string_metric_i64_for_id(planner, id, metric)?;
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
                Stage2CompileError::Internal(format!(
                    "{} begin arg had no expressions",
                    metric.op()
                ))
            })?;
            let lowered = lower_str_metric_expr(last, planner, metric)?;
            exprs.push(lowered);
            Ok(PExpr::Begin { exprs, ty: Ty::I64 })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal(format!("{} let arg had empty body", metric.op()))
            })?;
            let lowered = lower_str_metric_expr(last, planner, metric)?;
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
                return Err(Stage2CompileError::Unsupported(format!(
                    "{} currently requires stage2-known string values",
                    metric.op()
                )));
            };
            let Some(else_id) = planner_const_string_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(format!(
                    "{} currently requires stage2-known string values",
                    metric.op()
                )));
            };
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Int(string_metric_i64_for_id(
                    planner, then_id, metric,
                )?)),
                else_expr: Box::new(PExpr::Int(string_metric_i64_for_id(
                    planner, else_id, metric,
                )?)),
                cond_ty,
                ty: Ty::I64,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(format!(
            "{} currently requires stage2-known string values",
            metric.op()
        ))),
    }
}

fn normalized_string_id(planner: &mut Planner, id: i32) -> Result<i32, Stage2CompileError> {
    let input = planner_string_for_id(planner, id)?;
    let normalized = normalize_nfc(&input).map_err(|error| {
        Stage2CompileError::Unsupported(format!("str/nfc could not allocate output: {error}"))
    })?;
    planner.intern_string(&normalized)
}

fn lower_str_nfc_expr(arg: PExpr, planner: &mut Planner) -> Result<PExpr, Stage2CompileError> {
    if let Some(id) = planner_const_string_id(planner, &arg) {
        let output_id = normalized_string_id(planner, id)?;
        let idx = planner.alloc_local(Ty::StrI32)?;
        return Ok(PExpr::Let {
            bindings: vec![LetBinding { idx, expr: arg }],
            body: vec![PExpr::Str(output_id)],
            ty: Ty::StrI32,
        });
    }
    match arg {
        PExpr::Begin { mut exprs, .. } => {
            let last = exprs.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/nfc begin arg had no expressions".to_string())
            })?;
            exprs.push(lower_str_nfc_expr(last, planner)?);
            Ok(PExpr::Begin {
                exprs,
                ty: Ty::StrI32,
            })
        }
        PExpr::Let {
            bindings, mut body, ..
        } => {
            let last = body.pop().ok_or_else(|| {
                Stage2CompileError::Internal("str/nfc let arg had empty body".to_string())
            })?;
            body.push(lower_str_nfc_expr(last, planner)?);
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
                    "str/nfc currently requires stage2-known string values".to_string(),
                ));
            };
            let Some(else_id) = planner_const_string_id(planner, &else_expr) else {
                return Err(Stage2CompileError::Unsupported(
                    "str/nfc currently requires stage2-known string values".to_string(),
                ));
            };
            Ok(PExpr::If {
                cond,
                then_expr: Box::new(PExpr::Str(normalized_string_id(planner, then_id)?)),
                else_expr: Box::new(PExpr::Str(normalized_string_id(planner, else_id)?)),
                cond_ty,
                ty: Ty::StrI32,
            })
        }
        _ => Err(Stage2CompileError::Unsupported(
            "str/nfc currently requires stage2-known string values".to_string(),
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
