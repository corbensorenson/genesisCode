use std::sync::Arc;

use crate::error::{KernelError, KernelErrorKind};
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};

use super::{CExpr, COMPILED_MODULE_BLOB_MAGIC, CompiledForm, CompiledModule};

pub(super) fn encode_compiled_module_blob(m: &CompiledModule) -> Result<Vec<u8>, KernelError> {
    let mut out = Vec::new();
    out.extend_from_slice(COMPILED_MODULE_BLOB_MAGIC);
    push_u32(&mut out, m.forms.len())?;
    for f in &m.forms {
        match f {
            CompiledForm::Def(name, expr) => {
                out.push(0);
                push_str(&mut out, name)?;
                encode_cexpr(&mut out, expr)?;
            }
            CompiledForm::Expr(expr) => {
                out.push(1);
                encode_cexpr(&mut out, expr)?;
            }
        }
    }
    Ok(out)
}

pub(super) fn decode_compiled_module_blob(bytes: &[u8]) -> Result<CompiledModule, KernelError> {
    let mut cur = DecodeCursor { bytes, at: 0 };
    let got_magic = cur.read_exact(COMPILED_MODULE_BLOB_MAGIC.len())?;
    if got_magic != COMPILED_MODULE_BLOB_MAGIC {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "compiled module blob magic mismatch",
        ));
    }
    let forms_len = cur.read_u32()? as usize;
    let mut forms = Vec::with_capacity(forms_len);
    for _ in 0..forms_len {
        let tag = cur.read_u8()?;
        match tag {
            0 => {
                let name = cur.read_str()?;
                let expr = decode_cexpr(&mut cur)?;
                forms.push(CompiledForm::Def(name, expr));
            }
            1 => {
                let expr = decode_cexpr(&mut cur)?;
                forms.push(CompiledForm::Expr(expr));
            }
            _ => {
                return Err(KernelError::new(
                    KernelErrorKind::Internal,
                    format!("invalid compiled form tag: {tag}"),
                ));
            }
        }
    }
    if cur.remaining() != 0 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "compiled module blob has trailing bytes",
        ));
    }
    Ok(CompiledModule { forms })
}

fn push_u32(out: &mut Vec<u8>, n: usize) -> Result<(), KernelError> {
    let n: u32 = u32::try_from(n).map_err(|_| {
        KernelError::new(
            KernelErrorKind::Internal,
            "compiled module blob field exceeds u32 range",
        )
    })?;
    out.extend_from_slice(&n.to_le_bytes());
    Ok(())
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), KernelError> {
    push_u32(out, bytes.len())?;
    out.extend_from_slice(bytes);
    Ok(())
}

fn push_str(out: &mut Vec<u8>, s: &str) -> Result<(), KernelError> {
    push_bytes(out, s.as_bytes())
}

fn push_term(out: &mut Vec<u8>, t: &Term) -> Result<(), KernelError> {
    let rendered = print_term(t);
    push_str(out, &rendered)
}

fn encode_cexpr(out: &mut Vec<u8>, expr: &Arc<CExpr>) -> Result<(), KernelError> {
    match expr.as_ref() {
        CExpr::Atom(t) => {
            out.push(0);
            push_term(out, t)
        }
        CExpr::Var { name, site_id } => {
            out.push(1);
            push_str(out, name)?;
            push_str(out, site_id)
        }
        CExpr::Vector(items) => {
            out.push(2);
            push_u32(out, items.len())?;
            for t in items {
                push_term(out, t)?;
            }
            Ok(())
        }
        CExpr::Map(entries) => {
            out.push(3);
            push_u32(out, entries.len())?;
            for (k, v) in entries {
                push_term(out, &k.0)?;
                encode_cexpr(out, v)?;
            }
            Ok(())
        }
        CExpr::Quote(t) => {
            out.push(4);
            push_term(out, t)
        }
        CExpr::If {
            cond,
            then_expr,
            else_expr,
            site_id,
        } => {
            out.push(5);
            push_str(out, site_id)?;
            encode_cexpr(out, cond)?;
            encode_cexpr(out, then_expr)?;
            encode_cexpr(out, else_expr)
        }
        CExpr::Begin(items) => {
            out.push(6);
            push_u32(out, items.len())?;
            for it in items {
                encode_cexpr(out, it)?;
            }
            Ok(())
        }
        CExpr::Let(bindings, body) => {
            out.push(7);
            push_u32(out, bindings.len())?;
            for (name, rhs) in bindings {
                push_str(out, name)?;
                encode_cexpr(out, rhs)?;
            }
            encode_cexpr(out, body)
        }
        CExpr::FnUnary {
            param,
            body_term,
            body,
        } => {
            out.push(8);
            push_str(out, param)?;
            push_term(out, body_term)?;
            encode_cexpr(out, body)
        }
        CExpr::Prim { op, args } => {
            out.push(9);
            push_str(out, op)?;
            push_u32(out, args.len())?;
            for a in args {
                encode_cexpr(out, a)?;
            }
            Ok(())
        }
        CExpr::SealNew => {
            out.push(10);
            Ok(())
        }
        CExpr::Seal(v, tok) => {
            out.push(11);
            encode_cexpr(out, v)?;
            encode_cexpr(out, tok)
        }
        CExpr::Unseal(w, tok) => {
            out.push(12);
            encode_cexpr(out, w)?;
            encode_cexpr(out, tok)
        }
        CExpr::App(f, x) => {
            out.push(13);
            encode_cexpr(out, f)?;
            encode_cexpr(out, x)
        }
    }
}

struct DecodeCursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> DecodeCursor<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn read_exact(&mut self, n: usize) -> Result<&'a [u8], KernelError> {
        if self.remaining() < n {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "compiled module blob truncated",
            ));
        }
        let start = self.at;
        self.at += n;
        Ok(&self.bytes[start..start + n])
    }

    fn read_u8(&mut self) -> Result<u8, KernelError> {
        Ok(self.read_exact(1)?[0])
    }

    fn read_u32(&mut self) -> Result<u32, KernelError> {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(self.read_exact(4)?);
        Ok(u32::from_le_bytes(buf))
    }

    fn read_bytes(&mut self) -> Result<&'a [u8], KernelError> {
        let n = self.read_u32()? as usize;
        self.read_exact(n)
    }

    fn read_str(&mut self) -> Result<String, KernelError> {
        let b = self.read_bytes()?;
        let s = std::str::from_utf8(b).map_err(|e| {
            KernelError::new(
                KernelErrorKind::Internal,
                format!("compiled module blob invalid utf-8: {e}"),
            )
        })?;
        Ok(s.to_string())
    }

    fn read_term(&mut self) -> Result<Term, KernelError> {
        let s = self.read_str()?;
        parse_term(&s).map_err(|e| {
            KernelError::new(
                KernelErrorKind::Internal,
                format!("compiled module blob term parse failed: {e}"),
            )
        })
    }
}

fn decode_cexpr(cur: &mut DecodeCursor<'_>) -> Result<Arc<CExpr>, KernelError> {
    let tag = cur.read_u8()?;
    let out = match tag {
        0 => CExpr::Atom(cur.read_term()?),
        1 => CExpr::Var {
            name: cur.read_str()?,
            site_id: cur.read_str()?,
        },
        2 => {
            let n = cur.read_u32()? as usize;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(cur.read_term()?);
            }
            CExpr::Vector(items)
        }
        3 => {
            let n = cur.read_u32()? as usize;
            let mut entries = Vec::with_capacity(n);
            for _ in 0..n {
                let key = TermOrdKey(cur.read_term()?);
                let val = decode_cexpr(cur)?;
                entries.push((key, val));
            }
            CExpr::Map(entries)
        }
        4 => CExpr::Quote(cur.read_term()?),
        5 => CExpr::If {
            site_id: cur.read_str()?,
            cond: decode_cexpr(cur)?,
            then_expr: decode_cexpr(cur)?,
            else_expr: decode_cexpr(cur)?,
        },
        6 => {
            let n = cur.read_u32()? as usize;
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(decode_cexpr(cur)?);
            }
            CExpr::Begin(items)
        }
        7 => {
            let n = cur.read_u32()? as usize;
            let mut bindings = Vec::with_capacity(n);
            for _ in 0..n {
                let name = cur.read_str()?;
                let rhs = decode_cexpr(cur)?;
                bindings.push((name, rhs));
            }
            let body = decode_cexpr(cur)?;
            CExpr::Let(bindings, body)
        }
        8 => {
            let param = cur.read_str()?;
            let body_term = cur.read_term()?;
            let body = decode_cexpr(cur)?;
            CExpr::FnUnary {
                param,
                body_term,
                body,
            }
        }
        9 => {
            let op = cur.read_str()?;
            let n = cur.read_u32()? as usize;
            let mut args = Vec::with_capacity(n);
            for _ in 0..n {
                args.push(decode_cexpr(cur)?);
            }
            CExpr::Prim { op, args }
        }
        10 => CExpr::SealNew,
        11 => CExpr::Seal(decode_cexpr(cur)?, decode_cexpr(cur)?),
        12 => CExpr::Unseal(decode_cexpr(cur)?, decode_cexpr(cur)?),
        13 => CExpr::App(decode_cexpr(cur)?, decode_cexpr(cur)?),
        _ => {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("invalid compiled expr tag: {tag}"),
            ));
        }
    };
    Ok(Arc::new(out))
}
