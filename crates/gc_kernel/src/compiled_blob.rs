use std::sync::{Arc, OnceLock};

use crate::error::{KernelError, KernelErrorKind};
use crate::eval::PrimOp;
use crate::fallible_alloc::{clone_str, vec_with_capacity};
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};

use super::{
    CExpr, COMPILED_MODULE_BLOB_MAGIC, CompiledCoverageSites, CompiledForm, CompiledModule,
    SymbolInterner, VarResolution,
};

pub(super) fn encode_compiled_module_blob(m: &CompiledModule) -> Result<Vec<u8>, KernelError> {
    let mut out = Vec::new();
    out.extend_from_slice(COMPILED_MODULE_BLOB_MAGIC);
    push_u32(&mut out, m.module_names.len())?;
    for name in &m.module_names {
        push_str(&mut out, name)?;
    }
    push_str_slice(&mut out, m.coverage_sites.statement_sites())?;
    push_str_slice(&mut out, m.coverage_sites.decision_sites())?;
    push_u32(&mut out, m.forms.len())?;
    for f in &m.forms {
        match f {
            CompiledForm::Def {
                name,
                module_slot,
                expr,
            } => {
                out.push(0);
                push_str(&mut out, name)?;
                out.extend_from_slice(&module_slot.to_le_bytes());
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
    let module_names_len = cur.read_count(4, "module names")?;
    let mut module_names = vec_with_capacity(module_names_len, "compiled module names")?;
    for _ in 0..module_names_len {
        module_names.push(cur.read_str()?);
    }
    let statement_sites = cur.read_str_vec()?;
    let decision_sites = cur.read_str_vec()?;
    let forms_len = cur.read_count(1, "forms")?;
    let mut interner = SymbolInterner::default();
    let mut forms = vec_with_capacity(forms_len, "compiled module forms")?;
    for _ in 0..forms_len {
        let tag = cur.read_u8()?;
        match tag {
            0 => {
                let name = cur.read_str()?;
                let module_slot = cur.read_u32()?;
                let expr = decode_cexpr(&mut cur, &mut interner)?;
                forms.push(CompiledForm::Def {
                    name,
                    module_slot,
                    expr,
                });
            }
            1 => {
                let expr = decode_cexpr(&mut cur, &mut interner)?;
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
    let decision_conditions = super::compiled_coverage::collect_decision_conditions_and_validate(
        &forms,
        statement_sites.len(),
        decision_sites.len(),
    )?;
    let coverage_sites =
        CompiledCoverageSites::from_parts(statement_sites, decision_sites, decision_conditions)?;
    Ok(CompiledModule {
        forms,
        module_names,
        coverage_sites,
    })
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

fn push_str_slice(out: &mut Vec<u8>, xs: &[String]) -> Result<(), KernelError> {
    push_u32(out, xs.len())?;
    for x in xs {
        push_str(out, x)?;
    }
    Ok(())
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
        CExpr::Var {
            name,
            sym: _,
            resolution,
            statement_site,
        } => {
            out.push(1);
            push_str(out, name)?;
            out.extend_from_slice(&statement_site.to_le_bytes());
            encode_var_resolution(out, *resolution)
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
            decision_site,
        } => {
            out.push(5);
            out.extend_from_slice(&decision_site.to_le_bytes());
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
            ..
        } => {
            out.push(8);
            push_str(out, param)?;
            push_term(out, body_term)?;
            encode_cexpr(out, body)
        }
        CExpr::Prim { op, args } => {
            out.push(9);
            push_str(out, op.as_str())?;
            push_u32(out, args.len())?;
            for a in args {
                encode_cexpr(out, a)?;
            }
            Ok(())
        }
        CExpr::PrimUnknown { op, args } => {
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
        CExpr::AppN {
            callee,
            args,
            extra_app_ticks,
        } => {
            out.push(14);
            out.extend_from_slice(&extra_app_ticks.to_le_bytes());
            encode_cexpr(out, callee)?;
            push_u32(out, args.len())?;
            for a in args.iter() {
                encode_cexpr(out, a)?;
            }
            Ok(())
        }
    }
}

fn encode_var_resolution(out: &mut Vec<u8>, resolution: VarResolution) -> Result<(), KernelError> {
    match resolution {
        VarResolution::Local { depth, slot } => {
            out.push(0);
            out.extend_from_slice(&depth.to_le_bytes());
            out.extend_from_slice(&slot.to_le_bytes());
        }
        VarResolution::Module { slot } => {
            out.push(1);
            out.extend_from_slice(&slot.to_le_bytes());
        }
        VarResolution::External => {
            out.push(2);
        }
    }
    Ok(())
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

    fn read_u16(&mut self) -> Result<u16, KernelError> {
        let mut buf = [0u8; 2];
        buf.copy_from_slice(self.read_exact(2)?);
        Ok(u16::from_le_bytes(buf))
    }

    fn read_count(
        &mut self,
        minimum_item_bytes: usize,
        field: &'static str,
    ) -> Result<usize, KernelError> {
        let count = self.read_u32()? as usize;
        if count > self.remaining() / minimum_item_bytes {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("compiled module blob {field} count exceeds remaining input"),
            ));
        }
        Ok(count)
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
        clone_str(s, "compiled module string")
    }

    fn read_str_vec(&mut self) -> Result<Vec<String>, KernelError> {
        let n = self.read_count(4, "string vector")?;
        let mut out = vec_with_capacity(n, "compiled string vector")?;
        for _ in 0..n {
            out.push(self.read_str()?);
        }
        Ok(out)
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

fn decode_cexpr(
    cur: &mut DecodeCursor<'_>,
    interner: &mut SymbolInterner,
) -> Result<Arc<CExpr>, KernelError> {
    let tag = cur.read_u8()?;
    let out = match tag {
        0 => CExpr::Atom(cur.read_term()?),
        1 => {
            let name = cur.read_str()?;
            let statement_site = cur.read_u32()?;
            let resolution = decode_var_resolution(cur)?;
            let sym = interner.intern(&name)?;
            CExpr::Var {
                name,
                sym,
                resolution,
                statement_site,
            }
        }
        2 => {
            let n = cur.read_count(4, "vector items")?;
            let mut items = vec_with_capacity(n, "compiled vector items")?;
            for _ in 0..n {
                items.push(cur.read_term()?);
            }
            CExpr::Vector(items)
        }
        3 => {
            let n = cur.read_count(5, "map entries")?;
            let mut entries = vec_with_capacity(n, "compiled map entries")?;
            for _ in 0..n {
                let key = TermOrdKey(cur.read_term()?);
                let val = decode_cexpr(cur, interner)?;
                entries.push((key, val));
            }
            CExpr::Map(entries)
        }
        4 => CExpr::Quote(cur.read_term()?),
        5 => CExpr::If {
            decision_site: cur.read_u32()?,
            cond: decode_cexpr(cur, interner)?,
            then_expr: decode_cexpr(cur, interner)?,
            else_expr: decode_cexpr(cur, interner)?,
        },
        6 => {
            let n = cur.read_count(1, "begin expressions")?;
            let mut items = vec_with_capacity(n, "compiled begin expressions")?;
            for _ in 0..n {
                items.push(decode_cexpr(cur, interner)?);
            }
            CExpr::Begin(items)
        }
        7 => {
            let n = cur.read_count(5, "let bindings")?;
            let mut bindings = vec_with_capacity(n, "compiled let bindings")?;
            for _ in 0..n {
                let name = cur.read_str()?;
                let rhs = decode_cexpr(cur, interner)?;
                bindings.push((name, rhs));
            }
            let body = decode_cexpr(cur, interner)?;
            CExpr::Let(bindings, body)
        }
        8 => {
            let param = cur.read_str()?;
            let body_term = cur.read_term()?;
            let body = decode_cexpr(cur, interner)?;
            CExpr::FnUnary {
                param,
                body_term,
                body,
                capture_plan: OnceLock::new(),
                primitive_forward_plan: OnceLock::new(),
            }
        }
        9 => {
            let op = cur.read_str()?;
            let n = cur.read_count(1, "primitive arguments")?;
            let mut args = vec_with_capacity(n, "compiled primitive arguments")?;
            for _ in 0..n {
                args.push(decode_cexpr(cur, interner)?);
            }
            if let Some(op) = PrimOp::from_str(&op) {
                CExpr::Prim { op, args }
            } else {
                CExpr::PrimUnknown { op, args }
            }
        }
        10 => CExpr::SealNew,
        11 => CExpr::Seal(decode_cexpr(cur, interner)?, decode_cexpr(cur, interner)?),
        12 => CExpr::Unseal(decode_cexpr(cur, interner)?, decode_cexpr(cur, interner)?),
        13 => CExpr::App(decode_cexpr(cur, interner)?, decode_cexpr(cur, interner)?),
        14 => {
            let extra_app_ticks = cur.read_u32()?;
            let callee = decode_cexpr(cur, interner)?;
            let n = cur.read_count(1, "call arguments")?;
            let mut args = vec_with_capacity(n, "compiled call arguments")?;
            for _ in 0..n {
                args.push(decode_cexpr(cur, interner)?);
            }
            CExpr::AppN {
                callee,
                args: args.into_boxed_slice(),
                extra_app_ticks,
            }
        }
        _ => {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("invalid compiled expr tag: {tag}"),
            ));
        }
    };
    Ok(Arc::new(out))
}

fn decode_var_resolution(cur: &mut DecodeCursor<'_>) -> Result<VarResolution, KernelError> {
    match cur.read_u8()? {
        0 => Ok(VarResolution::Local {
            depth: cur.read_u16()?,
            slot: cur.read_u16()?,
        }),
        1 => Ok(VarResolution::Module {
            slot: cur.read_u32()?,
        }),
        2 => Ok(VarResolution::External),
        tag => Err(KernelError::new(
            KernelErrorKind::Internal,
            format!("invalid compiled var resolution tag: {tag}"),
        )),
    }
}
