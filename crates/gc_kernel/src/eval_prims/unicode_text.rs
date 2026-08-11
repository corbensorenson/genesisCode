use super::*;
use crate::fallible_alloc::clone_str;
use crate::text_profile::{
    GraphemeRangeError, grapheme_len, grapheme_slice_bounds, normalize_nfc, scalar_len,
};

pub(super) fn dispatch_unicode_text_prim(
    ctx: &mut EvalCtx,
    op: PrimOp,
    args: &[Value],
) -> Result<Value, KernelError> {
    match op {
        PrimOp::StrScalarLen => {
            if args.len() != 1 {
                return type_err(ctx, "str/scalar-len expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "str/scalar-len expects string");
            };
            Ok(usize_to_int_value(scalar_len(s)))
        }
        PrimOp::StrGraphemeLen => {
            if args.len() != 1 {
                return type_err(ctx, "str/grapheme-len expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "str/grapheme-len expects string");
            };
            Ok(usize_to_int_value(grapheme_len(s)))
        }
        PrimOp::StrGraphemeSlice => {
            if args.len() != 3 {
                return type_err(ctx, "str/grapheme-slice expects 3 args");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "str/grapheme-slice expects string");
            };
            let Some(start) = value_to_bigint(&args[1]).and_then(|value| value.to_usize()) else {
                return text_range_err(ctx, "str/grapheme-slice start out of range");
            };
            let Some(len) = value_to_bigint(&args[2]).and_then(|value| value.to_usize()) else {
                return text_range_err(ctx, "str/grapheme-slice len out of range");
            };
            let (start_byte, end_byte) = match grapheme_slice_bounds(s, start, len) {
                Ok(bounds) => bounds,
                Err(GraphemeRangeError::IndexOverflow | GraphemeRangeError::OutOfRange) => {
                    return text_range_err(ctx, "str/grapheme-slice range out of bounds");
                }
            };
            let output = clone_str(&s[start_byte..end_byte], "str/grapheme-slice")?;
            ctx.mem_observe_string_len(output.len())?;
            Ok(Value::data(Term::Str(output)))
        }
        PrimOp::StrNfc => {
            if args.len() != 1 {
                return type_err(ctx, "str/nfc expects 1 arg");
            }
            let Some(Term::Str(s)) = args[0].as_data() else {
                return type_err(ctx, "str/nfc expects string");
            };
            let output = normalize_nfc(s)?;
            ctx.mem_observe_string_len(output.len())?;
            Ok(Value::data(Term::Str(output)))
        }
        _ => Err(KernelError::new(
            KernelErrorKind::Internal,
            format!(
                "non-Unicode prim routed to Unicode dispatcher: {}",
                op.as_str()
            ),
        )),
    }
}

fn text_range_err(ctx: &mut EvalCtx, msg: &str) -> Result<Value, KernelError> {
    sealed_error(
        ctx,
        "core/text-range-error",
        "text-range",
        msg,
        KernelErrorKind::Type,
    )
}
