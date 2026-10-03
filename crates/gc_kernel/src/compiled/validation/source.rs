use super::{invalid, push, terms};
use crate::compiled::CExpr;
use crate::error::KernelError;
use gc_coreform::Term;

enum Source<'a> {
    Term(&'a Term),
    Body(Vec<&'a Term>),
    Function {
        params: Vec<&'a Term>,
        body: Vec<&'a Term>,
    },
}

type Work<'a> = Vec<(Source<'a>, &'a CExpr)>;

fn body<'a>(work: &mut Work<'a>, items: Vec<&'a Term>, expr: &'a CExpr) -> Result<(), KernelError> {
    if items.is_empty() {
        return Err(invalid());
    }
    push(work, (Source::Body(items), expr))
}

fn children<'a>(
    work: &mut Work<'a>,
    items: &[&'a Term],
    exprs: &'a [std::sync::Arc<CExpr>],
) -> Result<(), KernelError> {
    if items.len() != exprs.len() {
        return Err(invalid());
    }
    for (term, expr) in items.iter().zip(exprs).rev() {
        push(work, (Source::Term(term), expr))?;
    }
    Ok(())
}

fn copy<'a>(items: &[&'a Term]) -> Result<Vec<&'a Term>, KernelError> {
    let mut out =
        crate::fallible_alloc::vec_with_capacity(items.len(), "compiled source references")?;
    out.extend_from_slice(items);
    Ok(out)
}

// The checker follows the source grammar independently of the compiler. It does
// not recompile a candidate or accept a candidate's own capture/forward plans.
pub(super) fn validate(term: &Term, expr: &CExpr) -> Result<(), KernelError> {
    let mut work = Vec::new();
    push(&mut work, (Source::Term(term), expr))?;
    while let Some((source, expr)) = work.pop() {
        match source {
            Source::Body(mut items) => {
                if items.len() == 1 {
                    push(&mut work, (Source::Term(items.remove(0)), expr))?;
                } else if let CExpr::Begin(exprs) = expr {
                    children(&mut work, &items, exprs)?;
                } else {
                    return Err(invalid());
                }
            }
            Source::Function {
                mut params,
                body: source_body,
            } => {
                if params.is_empty() {
                    return Err(invalid());
                }
                let Term::Symbol(expected) = params.remove(0) else {
                    return Err(invalid());
                };
                let CExpr::FnUnary {
                    param,
                    body: actual,
                    ..
                } = expr
                else {
                    return Err(invalid());
                };
                if param != expected {
                    return Err(invalid());
                }
                if params.is_empty() {
                    body(&mut work, source_body, actual)?;
                } else {
                    push(
                        &mut work,
                        (
                            Source::Function {
                                params,
                                body: source_body,
                            },
                            actual,
                        ),
                    )?;
                }
            }
            Source::Term(term) => match term {
                Term::Nil | Term::Bool(_) | Term::Int(_) | Term::Str(_) | Term::Bytes(_) => {
                    let CExpr::Atom(actual) = expr else {
                        return Err(invalid());
                    };
                    if !terms::equal(term, actual)? {
                        return Err(invalid());
                    }
                }
                Term::Symbol(name) => {
                    if !matches!(expr, CExpr::Var { name: actual, .. } if actual == name) {
                        return Err(invalid());
                    }
                }
                Term::Vector(items) => {
                    let CExpr::Vector(actual) = expr else {
                        return Err(invalid());
                    };
                    if items.len() != actual.len() {
                        return Err(invalid());
                    }
                    for (left, right) in items.iter().zip(actual) {
                        if !terms::equal(left, right)? {
                            return Err(invalid());
                        }
                    }
                }
                Term::Map(items) => {
                    let CExpr::Map(actual) = expr else {
                        return Err(invalid());
                    };
                    if items.len() != actual.len() {
                        return Err(invalid());
                    }
                    for ((key, value), (actual_key, actual_value)) in items.iter().zip(actual).rev()
                    {
                        if !terms::equal(&key.0, &actual_key.0)? {
                            return Err(invalid());
                        }
                        push(&mut work, (Source::Term(value), actual_value))?;
                    }
                }
                Term::Pair(_, _) => {
                    let items = terms::list(term)?;
                    if items.is_empty() {
                        return Err(invalid());
                    }
                    let head = if let Term::Symbol(name) = items[0] {
                        name.as_str()
                    } else {
                        ""
                    };
                    match head {
                        "quote" => {
                            let CExpr::Quote(actual) = expr else {
                                return Err(invalid());
                            };
                            if items.len() != 2 || !terms::equal(items[1], actual)? {
                                return Err(invalid());
                            }
                        }
                        "fn" => {
                            if items.len() < 3 {
                                return Err(invalid());
                            }
                            let params = terms::list(items[1])?;
                            if params.is_empty()
                                || params.iter().any(|p| !matches!(p, Term::Symbol(_)))
                            {
                                return Err(invalid());
                            }
                            push(
                                &mut work,
                                (
                                    Source::Function {
                                        params,
                                        body: copy(&items[2..])?,
                                    },
                                    expr,
                                ),
                            )?;
                        }
                        "begin" => body(&mut work, copy(&items[1..])?, expr)?,
                        "if" => {
                            let CExpr::If {
                                cond,
                                then_expr,
                                else_expr,
                                ..
                            } = expr
                            else {
                                return Err(invalid());
                            };
                            if items.len() != 4 {
                                return Err(invalid());
                            }
                            push(&mut work, (Source::Term(items[3]), else_expr))?;
                            push(&mut work, (Source::Term(items[2]), then_expr))?;
                            push(&mut work, (Source::Term(items[1]), cond))?;
                        }
                        "let" => {
                            let CExpr::Let(bindings, actual_body) = expr else {
                                return Err(invalid());
                            };
                            if items.len() < 3 {
                                return Err(invalid());
                            }
                            let expected = terms::list(items[1])?;
                            if expected.len() != bindings.len() {
                                return Err(invalid());
                            }
                            body(&mut work, copy(&items[2..])?, actual_body)?;
                            for (binding, (name, rhs)) in expected.iter().zip(bindings).rev() {
                                let pair = terms::list(binding)?;
                                if pair.len() != 2
                                    || !matches!(pair[0],Term::Symbol(expected) if expected==name)
                                {
                                    return Err(invalid());
                                }
                                push(&mut work, (Source::Term(pair[1]), rhs))?;
                            }
                        }
                        "prim" => {
                            if items.len() < 2 {
                                return Err(invalid());
                            }
                            let Term::Symbol(expected) = items[1] else {
                                return Err(invalid());
                            };
                            let (op, args) = match expr {
                                CExpr::Prim { op, args } => (op.as_str(), args),
                                CExpr::PrimUnknown { op, args } => (op.as_str(), args),
                                _ => return Err(invalid()),
                            };
                            if op != expected {
                                return Err(invalid());
                            }
                            children(&mut work, &items[2..], args)?;
                        }
                        "seal" | "unseal" => {
                            if head == "seal" && items.len() == 1 && matches!(expr, CExpr::SealNew)
                            {
                                continue;
                            }
                            let pair = match (head, expr) {
                                ("seal", CExpr::Seal(a, b)) | ("unseal", CExpr::Unseal(a, b)) => {
                                    (a, b)
                                }
                                _ => return Err(invalid()),
                            };
                            if items.len() != 3 {
                                return Err(invalid());
                            }
                            push(&mut work, (Source::Term(items[2]), pair.1))?;
                            push(&mut work, (Source::Term(items[1]), pair.0))?;
                        }
                        "def" => return Err(invalid()),
                        _ => application(&mut work, term, expr)?,
                    }
                }
            },
        }
    }
    Ok(())
}

fn application<'a>(
    work: &mut Work<'a>,
    term: &'a Term,
    expr: &'a CExpr,
) -> Result<(), KernelError> {
    let mut current = term;
    let mut groups = Vec::new();
    loop {
        let Term::Pair(_, _) = current else {
            break;
        };
        let items = terms::list(current)?;
        let head = if let Term::Symbol(name) = items[0] {
            name.as_str()
        } else {
            ""
        };
        if head == "begin" && items.len() == 2 {
            current = items[1];
            continue;
        }
        if matches!(
            head,
            "quote" | "fn" | "if" | "begin" | "let" | "prim" | "seal" | "unseal" | "def"
        ) {
            break;
        }
        current = items[0];
        if items.len() > 1 {
            push(&mut groups, copy(&items[1..])?)?;
        }
    }
    if groups.is_empty() {
        return push(work, (Source::Term(current), expr));
    }
    let ticks = groups.len() - 1;
    let mut args = Vec::new();
    for group in groups.into_iter().rev() {
        for arg in group {
            push(&mut args, arg)?;
        }
    }
    match expr {
        CExpr::App(callee, arg) if args.len() == 1 && ticks == 0 => {
            push(work, (Source::Term(args[0]), arg))?;
            push(work, (Source::Term(current), callee))?;
        }
        CExpr::AppN {
            callee,
            args: actual,
            extra_app_ticks,
        } if usize::try_from(*extra_app_ticks).ok() == Some(ticks) => {
            children(work, &args, actual)?;
            push(work, (Source::Term(current), callee))?;
        }
        _ => return Err(invalid()),
    }
    Ok(())
}
