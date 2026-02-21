use gc_coreform::{
    ParseError, Term, canonicalize_module, hash_module, hash_term, parse_module, parse_term,
    print_module, print_term,
};
use gc_kernel::{EvalCtx, KernelError, KernelErrorKind, Value};

use super::{mk_error_with, value_to_data_term};

fn hex32(h: [u8; 32]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in h {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}

fn parse_error_code_at(e: &ParseError) -> (&'static str, usize) {
    match e {
        ParseError::Eof => ("core/parse/eof", 0),
        ParseError::Unexpected { at, .. } => ("core/parse/unexpected", *at),
        ParseError::Escape { at, .. } => ("core/parse/escape", *at),
        ParseError::Int { at, .. } => ("core/parse/int", *at),
    }
}

fn arg_utf8_src(args: &[Value], idx: usize) -> Result<String, KernelError> {
    match args.get(idx) {
        Some(Value::Data(Term::Str(s))) => Ok(s.clone()),
        Some(Value::Data(Term::Bytes(bs))) => std::str::from_utf8(bs)
            .map(str::to_owned)
            .map_err(|e| KernelError::new(KernelErrorKind::BadForm, e.to_string())),
        Some(other) => Err(KernelError::new(
            KernelErrorKind::Type,
            format!("expected utf8 string or bytes, got {}", other.debug_repr()),
        )),
        None => Err(KernelError::new(
            KernelErrorKind::BadForm,
            "missing argument".to_string(),
        )),
    }
}

fn term_vec_from_value(v: &Value) -> Result<Vec<Term>, KernelError> {
    match v {
        Value::Vector(xs) => xs
            .iter()
            .map(value_to_data_term)
            .collect::<Result<Vec<_>, _>>(),
        Value::Data(Term::Vector(xs)) => Ok(xs.clone()),
        other => Err(KernelError::new(
            KernelErrorKind::Type,
            format!("expected vector of terms, got {}", other.debug_repr()),
        )),
    }
}

pub(super) fn nf_coreform_parse_term(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let src = match arg_utf8_src(&args, 0) {
        Ok(s) => s,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    match parse_term(&src) {
        Ok(t) => Ok(Value::Data(t)),
        Err(e) => {
            let (code, at) = parse_error_code_at(&e);
            Ok(mk_error_with(ctx, code, e.to_string(), Some(at)))
        }
    }
}

pub(super) fn nf_coreform_parse_module(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let src = match arg_utf8_src(&args, 0) {
        Ok(s) => s,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    match parse_module(&src) {
        Ok(forms) => Ok(Value::Data(Term::Vector(forms))),
        Err(e) => {
            let (code, at) = parse_error_code_at(&e);
            Ok(mk_error_with(ctx, code, e.to_string(), Some(at)))
        }
    }
}

pub(super) fn nf_coreform_canonicalize_module(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let Some(arg0) = args.first() else {
        return Ok(mk_error_with(
            ctx,
            "core/bad-form",
            "missing argument".to_string(),
            None,
        ));
    };
    let forms = match term_vec_from_value(arg0) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    match canonicalize_module(forms) {
        Ok(canon) => Ok(Value::Data(Term::Vector(canon))),
        Err(e) => Ok(mk_error_with(ctx, "core/bad-form", e.to_string(), None)),
    }
}

pub(super) fn nf_coreform_print_term(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let t = match value_to_data_term(
        args.first()
            .ok_or_else(|| KernelError::new(KernelErrorKind::BadForm, "missing argument"))?,
    ) {
        Ok(t) => t,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(print_term(&t))))
}

pub(super) fn nf_coreform_print_module(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let forms = match term_vec_from_value(
        args.first()
            .ok_or_else(|| KernelError::new(KernelErrorKind::BadForm, "missing argument"))?,
    ) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(print_module(&forms))))
}

pub(super) fn nf_coreform_fmt_module(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let src = match arg_utf8_src(&args, 0) {
        Ok(s) => s,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    let forms = match parse_module(&src) {
        Ok(f) => f,
        Err(e) => {
            let (code, at) = parse_error_code_at(&e);
            return Ok(mk_error_with(ctx, code, e.to_string(), Some(at)));
        }
    };
    let canon = match canonicalize_module(forms) {
        Ok(c) => c,
        Err(e) => return Ok(mk_error_with(ctx, "core/bad-form", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(print_module(&canon))))
}

pub(super) fn nf_coreform_hash_term(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let Some(arg0) = args.first() else {
        return Ok(mk_error_with(
            ctx,
            "core/bad-form",
            "missing argument".to_string(),
            None,
        ));
    };
    let t = match value_to_data_term(arg0) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(hex32(hash_term(&t)))))
}

pub(super) fn nf_coreform_hash_module(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let Some(arg0) = args.first() else {
        return Ok(mk_error_with(
            ctx,
            "core/bad-form",
            "missing argument".to_string(),
            None,
        ));
    };
    let forms = match term_vec_from_value(arg0) {
        Ok(x) => x,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(hex32(hash_module(&forms)))))
}

pub(super) fn nf_coreform_hash_module_src(
    ctx: &mut EvalCtx,
    args: Vec<Value>,
) -> Result<Value, KernelError> {
    let src = match arg_utf8_src(&args, 0) {
        Ok(s) => s,
        Err(e) => return Ok(mk_error_with(ctx, "core/type-error", e.to_string(), None)),
    };
    let forms = match parse_module(&src) {
        Ok(f) => f,
        Err(e) => {
            let (code, at) = parse_error_code_at(&e);
            return Ok(mk_error_with(ctx, code, e.to_string(), Some(at)));
        }
    };
    let canon = match canonicalize_module(forms) {
        Ok(c) => c,
        Err(e) => return Ok(mk_error_with(ctx, "core/bad-form", e.to_string(), None)),
    };
    Ok(Value::Data(Term::Str(hex32(hash_module(&canon)))))
}
