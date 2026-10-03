use super::{CExpr, CompiledForm, VarResolution};
use crate::error::{KernelError, KernelErrorKind};
use std::collections::BTreeMap;

mod source;
mod terms;

fn invalid() -> KernelError {
    KernelError::new(
        KernelErrorKind::Internal,
        "compiled module semantic metadata is inconsistent",
    )
}

fn push<T>(items: &mut Vec<T>, item: T) -> Result<(), KernelError> {
    items.try_reserve(1).map_err(|_| {
        KernelError::new(
            KernelErrorKind::MemoryLimit,
            "host allocation failed for compiled semantic validation",
        )
    })?;
    items.push(item);
    Ok(())
}

enum Work<'a> {
    Expr(&'a CExpr),
    Bind(&'a str),
    Restore(usize),
}

pub(super) fn validate(forms: &[CompiledForm], names: &[String]) -> Result<(), KernelError> {
    let mut globals = BTreeMap::new();
    let mut next = 0usize;
    for form in forms {
        if let CompiledForm::Def {
            name, module_slot, ..
        } = form
        {
            if !globals.contains_key(name.as_str()) {
                if names.get(next) != Some(name) {
                    return Err(invalid());
                }
                globals.insert(name.as_str(), next);
                next += 1;
            }
            if globals.get(name.as_str()).copied() != usize::try_from(*module_slot).ok() {
                return Err(invalid());
            }
        }
    }
    if next != names.len() {
        return Err(invalid());
    }
    let mut work = Vec::new();
    let mut locals = Vec::new();
    for form in forms {
        let (CompiledForm::Def { expr, .. } | CompiledForm::Expr(expr)) = form;
        push(&mut work, Work::Expr(expr))?;
        while let Some(item) = work.pop() {
            match item {
                Work::Bind(name) => {
                    gc_coreform::validate_symbol_name(name).map_err(|_| invalid())?;
                    push(&mut locals, name)?;
                }
                Work::Restore(size) => locals.truncate(size),
                Work::Expr(expr) => match expr {
                    CExpr::Var {
                        name, resolution, ..
                    } => {
                        gc_coreform::validate_symbol_name(name).map_err(|_| invalid())?;
                        let expected = if let Some(depth) =
                            locals.iter().rev().position(|local| *local == name)
                        {
                            VarResolution::Local {
                                depth: u16::try_from(depth).map_err(|_| invalid())?,
                                slot: 0,
                            }
                        } else if let Some(slot) = globals.get(name.as_str()) {
                            VarResolution::Module {
                                slot: u32::try_from(*slot).map_err(|_| invalid())?,
                            }
                        } else {
                            VarResolution::External
                        };
                        if *resolution != expected {
                            return Err(invalid());
                        }
                    }
                    CExpr::Atom(term) => {
                        if matches!(
                            term,
                            gc_coreform::Term::Symbol(_)
                                | gc_coreform::Term::Pair(_, _)
                                | gc_coreform::Term::Vector(_)
                                | gc_coreform::Term::Map(_)
                        ) {
                            return Err(invalid());
                        }
                    }
                    CExpr::Vector(_) | CExpr::Quote(_) | CExpr::SealNew => {}
                    CExpr::Map(entries) => {
                        if entries.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
                            return Err(invalid());
                        }
                        for (_, value) in entries.iter().rev() {
                            push(&mut work, Work::Expr(value))?;
                        }
                    }
                    CExpr::If {
                        cond,
                        then_expr,
                        else_expr,
                        ..
                    } => {
                        push(&mut work, Work::Expr(else_expr))?;
                        push(&mut work, Work::Expr(then_expr))?;
                        push(&mut work, Work::Expr(cond))?;
                    }
                    CExpr::Begin(items) => {
                        if items.len() < 2 {
                            return Err(invalid());
                        }
                        for item in items.iter().rev() {
                            push(&mut work, Work::Expr(item))?;
                        }
                    }
                    CExpr::Let(bindings, body) => {
                        push(&mut work, Work::Restore(locals.len()))?;
                        push(&mut work, Work::Expr(body))?;
                        for (name, rhs) in bindings.iter().rev() {
                            push(&mut work, Work::Bind(name))?;
                            push(&mut work, Work::Expr(rhs))?;
                        }
                    }
                    CExpr::FnUnary {
                        param,
                        body_term,
                        body,
                        ..
                    } => {
                        gc_coreform::validate_symbol_name(param).map_err(|_| invalid())?;
                        source::validate(body_term, body)?;
                        push(&mut work, Work::Restore(locals.len()))?;
                        push(&mut work, Work::Expr(body))?;
                        push(&mut work, Work::Bind(param))?;
                    }
                    CExpr::PrimUnknown { op, args } => {
                        gc_coreform::validate_symbol_name(op).map_err(|_| invalid())?;
                        for arg in args.iter().rev() {
                            push(&mut work, Work::Expr(arg))?;
                        }
                    }
                    CExpr::Prim { args, .. } => {
                        for arg in args.iter().rev() {
                            push(&mut work, Work::Expr(arg))?;
                        }
                    }
                    CExpr::Seal(left, right)
                    | CExpr::Unseal(left, right)
                    | CExpr::App(left, right) => {
                        push(&mut work, Work::Expr(right))?;
                        push(&mut work, Work::Expr(left))?;
                    }
                    CExpr::AppN {
                        callee,
                        args,
                        extra_app_ticks,
                    } => {
                        if args.len() < 2
                            || usize::try_from(*extra_app_ticks).map_err(|_| invalid())?
                                >= args.len()
                            || matches!(callee.as_ref(), CExpr::App(_, _) | CExpr::AppN { .. })
                        {
                            return Err(invalid());
                        }
                        for arg in args.iter().rev() {
                            push(&mut work, Work::Expr(arg))?;
                        }
                        push(&mut work, Work::Expr(callee))?;
                    }
                },
            }
        }
        if !locals.is_empty() {
            return Err(invalid());
        }
    }
    Ok(())
}
