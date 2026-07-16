use super::*;

pub(super) fn compile_module_with_site_namespace_impl(
    forms: &[Term],
    site_namespace: &str,
) -> Result<CompiledModule, KernelError> {
    let mut module_slots = BTreeMap::new();
    let mut module_names = Vec::new();
    for form in forms {
        if let Some((name, _expr)) = parse_def(form)
            && !module_slots.contains_key(&name)
        {
            let slot = u32::try_from(module_names.len()).map_err(|_| {
                KernelError::new(
                    KernelErrorKind::Internal,
                    "compiled module global slot exceeds u32 range",
                )
            })?;
            module_slots.insert(name.clone(), slot);
            module_names.push(name);
        }
    }

    let mut session = CompileSession {
        interner: SymbolInterner::default(),
        module_slots,
        site_namespace,
        statement_sites: Vec::new(),
        statement_site_indices: BTreeMap::new(),
        decision_sites: Vec::new(),
        decision_site_indices: BTreeMap::new(),
    };
    let empty_scopes = LexicalScopes::default();
    let mut out = Vec::with_capacity(forms.len());
    for (form_idx, form) in forms.iter().enumerate() {
        let form_idx = u32::try_from(form_idx).map_err(|_| {
            KernelError::new(
                KernelErrorKind::Internal,
                "compiled module form index exceeds u32 range",
            )
        })?;
        if let Some((name, expr)) = parse_def(form) {
            let Some(module_slot) = session.module_slots.get(&name).copied() else {
                return Err(KernelError::new(
                    KernelErrorKind::Internal,
                    format!("compiled module missing slot for def {name}"),
                ));
            };
            let mut path = vec![form_idx, 2];
            out.push(CompiledForm::Def {
                name,
                module_slot,
                expr: compile_term_with_site_path(&expr, &mut path, &mut session, &empty_scopes)?,
            });
        } else {
            let mut path = vec![form_idx];
            out.push(CompiledForm::Expr(compile_term_with_site_path(
                form,
                &mut path,
                &mut session,
                &empty_scopes,
            )?));
        }
    }
    let decision_conditions = compiled_coverage::collect_decision_conditions_and_validate(
        &out,
        session.statement_sites.len(),
        session.decision_sites.len(),
    )?;
    let coverage_sites = CompiledCoverageSites::from_parts(
        session.statement_sites,
        session.decision_sites,
        decision_conditions,
    )?;
    Ok(CompiledModule {
        forms: out,
        module_names,
        coverage_sites,
    })
}

struct CompileSession<'a> {
    interner: SymbolInterner,
    module_slots: BTreeMap<String, u32>,
    site_namespace: &'a str,
    statement_sites: Vec<String>,
    statement_site_indices: BTreeMap<String, u32>,
    decision_sites: Vec<String>,
    decision_site_indices: BTreeMap<String, u32>,
}

impl CompileSession<'_> {
    fn statement_site_index(&mut self, path: &[u32]) -> Result<u32, KernelError> {
        let site_id = site_id("stmt", self.site_namespace, path);
        intern_site(
            site_id,
            &mut self.statement_sites,
            &mut self.statement_site_indices,
        )
    }

    fn decision_site_index(&mut self, path: &[u32]) -> Result<u32, KernelError> {
        let site_id = site_id("decision", self.site_namespace, path);
        intern_site(
            site_id,
            &mut self.decision_sites,
            &mut self.decision_site_indices,
        )
    }
}

#[derive(Clone, Default)]
struct LexicalScopes {
    frames: Vec<BTreeMap<String, u16>>,
}

impl LexicalScopes {
    fn with_single_slot(&self, name: String) -> Self {
        let mut frames = self.frames.clone();
        let mut frame = BTreeMap::new();
        frame.insert(name, 0);
        frames.push(frame);
        Self { frames }
    }

    fn resolve(&self, name: &str) -> Result<Option<(u16, u16)>, KernelError> {
        for (depth, frame) in self.frames.iter().rev().enumerate() {
            if let Some(slot) = frame.get(name) {
                let depth = u16::try_from(depth).map_err(|_| {
                    KernelError::new(
                        KernelErrorKind::Internal,
                        "compiled lexical depth exceeds u16 range",
                    )
                })?;
                return Ok(Some((depth, *slot)));
            }
        }
        Ok(None)
    }
}

fn parse_def(t: &Term) -> Option<(String, Term)> {
    let items = t.as_proper_list()?;
    if items.len() != 3 {
        return None;
    }
    if !matches!(items[0], Term::Symbol(s) if s == "def") {
        return None;
    }
    let Term::Symbol(name) = items[1] else {
        return None;
    };
    Some((name.clone(), items[2].clone()))
}

fn site_id(kind: &str, site_namespace: &str, path: &[u32]) -> String {
    let path_str = path
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".");
    if site_namespace.is_empty() {
        format!("{kind}:{path_str}")
    } else {
        format!("{site_namespace}::{kind}:{path_str}")
    }
}

fn intern_site(
    site_id: String,
    sites: &mut Vec<String>,
    indices: &mut BTreeMap<String, u32>,
) -> Result<u32, KernelError> {
    if let Some(index) = indices.get(&site_id) {
        return Ok(*index);
    }
    let index = u32::try_from(sites.len()).map_err(|_| {
        KernelError::new(
            KernelErrorKind::Internal,
            "compiled coverage site index exceeds u32 range",
        )
    })?;
    sites.push(site_id.clone());
    indices.insert(site_id, index);
    Ok(index)
}

fn child_index(i: usize) -> Result<u32, KernelError> {
    u32::try_from(i).map_err(|_| {
        KernelError::new(
            KernelErrorKind::Internal,
            "compiled expression child index exceeds u32 range",
        )
    })
}

fn with_child_path<T>(
    path: &mut Vec<u32>,
    child: u32,
    f: impl FnOnce(&mut Vec<u32>) -> Result<T, KernelError>,
) -> Result<T, KernelError> {
    path.push(child);
    let out = f(path);
    path.pop();
    out
}

pub(super) fn compile_term(t: &Term) -> Result<CompiledExprBundle, KernelError> {
    let mut path = vec![0];
    let mut session = CompileSession {
        interner: SymbolInterner::default(),
        module_slots: BTreeMap::new(),
        site_namespace: "",
        statement_sites: Vec::new(),
        statement_site_indices: BTreeMap::new(),
        decision_sites: Vec::new(),
        decision_site_indices: BTreeMap::new(),
    };
    let expr = compile_term_with_site_path(t, &mut path, &mut session, &LexicalScopes::default())?;
    let forms = [CompiledForm::Expr(expr.clone())];
    let decision_conditions = compiled_coverage::collect_decision_conditions_and_validate(
        &forms,
        session.statement_sites.len(),
        session.decision_sites.len(),
    )?;
    let coverage_sites = CompiledCoverageSites::from_parts(
        session.statement_sites,
        session.decision_sites,
        decision_conditions,
    )?;
    Ok(CompiledExprBundle {
        expr,
        coverage_sites,
    })
}

fn compile_term_with_site_path(
    t: &Term,
    path: &mut Vec<u32>,
    session: &mut CompileSession<'_>,
    scopes: &LexicalScopes,
) -> Result<Arc<CExpr>, KernelError> {
    Ok(Arc::new(compile_term_inner(t, path, session, scopes)?))
}

fn compile_term_inner(
    t: &Term,
    path: &mut Vec<u32>,
    session: &mut CompileSession<'_>,
    scopes: &LexicalScopes,
) -> Result<CExpr, KernelError> {
    match t {
        Term::Nil | Term::Bool(_) | Term::Int(_) | Term::Str(_) | Term::Bytes(_) => {
            Ok(CExpr::Atom(t.clone()))
        }
        Term::Symbol(s) => compile_symbol(s, path, session, scopes),
        Term::Vector(xs) => Ok(CExpr::Vector(xs.clone())),
        Term::Map(m) => {
            let mut out = Vec::with_capacity(m.len());
            for (idx, (k, v)) in m.iter().enumerate() {
                let child = child_index(idx)?;
                out.push((
                    k.clone(),
                    with_child_path(path, child, |p| {
                        compile_term_with_site_path(v, p, session, scopes)
                    })?,
                ));
            }
            Ok(CExpr::Map(out))
        }
        Term::Pair(_, _) => {
            let Some(items) = t.as_proper_list() else {
                return Err(KernelError::new(
                    KernelErrorKind::BadForm,
                    "improper list is not a valid form",
                ));
            };
            compile_list(items, path, session, scopes)
        }
    }
}

fn compile_symbol(
    name: &str,
    path: &[u32],
    session: &mut CompileSession<'_>,
    scopes: &LexicalScopes,
) -> Result<CExpr, KernelError> {
    let sym = session.interner.intern(name)?;
    let resolution = if let Some((depth, slot)) = scopes.resolve(name)? {
        VarResolution::Local { depth, slot }
    } else if let Some(slot) = session.module_slots.get(name).copied() {
        VarResolution::Module { slot }
    } else {
        VarResolution::External
    };
    Ok(CExpr::Var {
        name: name.to_string(),
        sym,
        resolution,
        statement_site: session.statement_site_index(path)?,
    })
}

fn compile_list(
    items: Vec<&Term>,
    path: &mut Vec<u32>,
    session: &mut CompileSession<'_>,
    scopes: &LexicalScopes,
) -> Result<CExpr, KernelError> {
    if items.is_empty() {
        return Ok(CExpr::Atom(Term::Nil));
    }

    if let Term::Symbol(h) = items[0] {
        match h.as_str() {
            "quote" => {
                if items.len() != 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(quote datum) expects exactly 1 argument",
                    ));
                }
                return Ok(CExpr::Quote(items[1].clone()));
            }
            "fn" => {
                let (param, body_term) = desugar_fn_to_unary(&items)?;
                let body_scopes = scopes.with_single_slot(param.clone());
                let body = with_child_path(path, 0, |p| {
                    compile_term_with_site_path(&body_term, p, session, &body_scopes)
                })?;
                return Ok(CExpr::FnUnary {
                    param,
                    body_term,
                    body,
                    capture_plan: OnceLock::new(),
                });
            }
            "if" => {
                if items.len() != 4 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(if c t e) expects exactly 3 arguments",
                    ));
                }
                return Ok(CExpr::If {
                    decision_site: session.decision_site_index(path)?,
                    cond: with_child_path(path, 0, |p| {
                        compile_term_with_site_path(items[1], p, session, scopes)
                    })?,
                    then_expr: with_child_path(path, 1, |p| {
                        compile_term_with_site_path(items[2], p, session, scopes)
                    })?,
                    else_expr: with_child_path(path, 2, |p| {
                        compile_term_with_site_path(items[3], p, session, scopes)
                    })?,
                });
            }
            "begin" => {
                if items.len() < 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(begin ...) expects at least 1 argument",
                    ));
                }
                if items.len() == 2 {
                    return with_child_path(path, 0, |p| {
                        compile_term_inner(items[1], p, session, scopes)
                    });
                }
                let mut xs = Vec::with_capacity(items.len() - 1);
                for (idx, it) in items.iter().skip(1).enumerate() {
                    let child = child_index(idx)?;
                    xs.push(with_child_path(path, child, |p| {
                        compile_term_with_site_path(it, p, session, scopes)
                    })?);
                }
                return Ok(CExpr::Begin(xs));
            }
            "let" => {
                if items.len() < 3 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(let ((x e) ...) body...) expects bindings and body",
                    ));
                }
                let bindings = items[1];
                let Some(bs) = bindings.as_proper_list() else {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(let ...) bindings must be a list",
                    ));
                };
                let mut out_bs = Vec::with_capacity(bs.len());
                let mut body_scopes = scopes.clone();
                for (idx, b) in bs.into_iter().enumerate() {
                    let Some(pair) = b.as_proper_list() else {
                        return Err(KernelError::new(
                            KernelErrorKind::BadForm,
                            "(let ...) binding must be a list (name expr)",
                        ));
                    };
                    if pair.len() != 2 {
                        return Err(KernelError::new(
                            KernelErrorKind::BadForm,
                            "(let ...) binding must have exactly 2 forms",
                        ));
                    }
                    let Term::Symbol(name) = pair[0] else {
                        return Err(KernelError::new(
                            KernelErrorKind::BadForm,
                            "(let ...) binding name must be symbol",
                        ));
                    };
                    let child = child_index(idx)?;
                    out_bs.push((
                        name.clone(),
                        with_child_path(path, child, |p| {
                            compile_term_with_site_path(pair[1], p, session, &body_scopes)
                        })?,
                    ));
                    body_scopes = body_scopes.with_single_slot(name.clone());
                }

                let body_term = if items.len() == 3 {
                    items[2].clone()
                } else {
                    let mut xs = Vec::with_capacity(items.len() - 1);
                    xs.push(Term::Symbol("begin".to_string()));
                    for b in items.iter().skip(2) {
                        xs.push((*b).clone());
                    }
                    Term::list(xs)
                };
                let body_child = child_index(out_bs.len())?;
                return Ok(CExpr::Let(
                    out_bs,
                    with_child_path(path, body_child, |p| {
                        compile_term_with_site_path(&body_term, p, session, &body_scopes)
                    })?,
                ));
            }
            "prim" => {
                if items.len() < 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(prim op ...) expects at least an op",
                    ));
                }
                let Term::Symbol(op) = items[1] else {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(prim ...) op must be a symbol",
                    ));
                };
                let mut args = Vec::with_capacity(items.len().saturating_sub(2));
                for (idx, a) in items.iter().skip(2).enumerate() {
                    let child = child_index(idx)?;
                    args.push(with_child_path(path, child, |p| {
                        compile_term_with_site_path(a, p, session, scopes)
                    })?);
                }
                if let Some(op) = PrimOp::from_str(op) {
                    return Ok(CExpr::Prim { op, args });
                }
                return Ok(CExpr::PrimUnknown {
                    op: op.clone(),
                    args,
                });
            }
            "seal" => {
                return match items.len() {
                    1 => Ok(CExpr::SealNew),
                    3 => Ok(CExpr::Seal(
                        with_child_path(path, 0, |p| {
                            compile_term_with_site_path(items[1], p, session, scopes)
                        })?,
                        with_child_path(path, 1, |p| {
                            compile_term_with_site_path(items[2], p, session, scopes)
                        })?,
                    )),
                    _ => Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(seal) or (seal v tok)",
                    )),
                };
            }
            "unseal" => {
                if items.len() != 3 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(unseal w tok) expects exactly 2 arguments",
                    ));
                }
                return Ok(CExpr::Unseal(
                    with_child_path(path, 0, |p| {
                        compile_term_with_site_path(items[1], p, session, scopes)
                    })?,
                    with_child_path(path, 1, |p| {
                        compile_term_with_site_path(items[2], p, session, scopes)
                    })?,
                ));
            }
            "def" => {
                return Err(KernelError::new(
                    KernelErrorKind::BadForm,
                    "(def ...) is only allowed at module top-level",
                ));
            }
            _ => {}
        }
    }

    let f = with_child_path(path, 0, |p| {
        compile_term_with_site_path(items[0], p, session, scopes)
    })?;
    if items.len() == 1 {
        return with_child_path(path, 0, |p| {
            compile_term_inner(items[0], p, session, scopes)
        });
    }
    let mut args = Vec::with_capacity(items.len().saturating_sub(1));
    for (idx, a) in items.iter().skip(1).enumerate() {
        let child = child_index(idx + 1)?;
        let arg = with_child_path(path, child, |p| {
            compile_term_with_site_path(a, p, session, scopes)
        })?;
        args.push(arg);
    }
    build_application_expr(f, args)
}

fn build_application_expr(callee: Arc<CExpr>, args: Vec<Arc<CExpr>>) -> Result<CExpr, KernelError> {
    if args.is_empty() {
        return Ok((*callee).clone());
    }
    match callee.as_ref() {
        CExpr::App(inner_callee, inner_arg) => {
            let mut all_args = Vec::with_capacity(args.len() + 1);
            all_args.push(inner_arg.clone());
            all_args.extend(args);
            Ok(CExpr::AppN {
                callee: inner_callee.clone(),
                args: all_args.into_boxed_slice(),
                extra_app_ticks: 1,
            })
        }
        CExpr::AppN {
            callee: inner_callee,
            args: inner_args,
            extra_app_ticks,
        } => {
            let mut all_args = Vec::with_capacity(inner_args.len() + args.len());
            all_args.extend(inner_args.iter().cloned());
            all_args.extend(args);
            Ok(CExpr::AppN {
                callee: inner_callee.clone(),
                args: all_args.into_boxed_slice(),
                extra_app_ticks: checked_extra_app_ticks(*extra_app_ticks, 1)?,
            })
        }
        _ if args.len() == 1 => {
            let mut args = args;
            Ok(CExpr::App(callee, args.remove(0)))
        }
        _ => Ok(CExpr::AppN {
            callee,
            args: args.into_boxed_slice(),
            extra_app_ticks: 0,
        }),
    }
}

fn checked_extra_app_ticks(a: u32, b: u32) -> Result<u32, KernelError> {
    a.checked_add(b).ok_or_else(|| {
        KernelError::new(
            KernelErrorKind::Internal,
            "compiled AppN source application tick count exceeds u32 range",
        )
    })
}

fn desugar_fn_to_unary(items: &[&Term]) -> Result<(String, Term), KernelError> {
    if items.len() < 3 {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn (x) body...) expects params and body",
        ));
    }
    let params = items[1];
    let Some(ps) = params.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn ...) params must be a list",
        ));
    };
    if ps.is_empty() {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "(fn ...) requires at least 1 parameter",
        ));
    }
    for p in &ps {
        if !matches!(p, Term::Symbol(_)) {
            return Err(KernelError::new(
                KernelErrorKind::BadForm,
                "(fn ...) params must be symbols",
            ));
        }
    }

    let body_term = if items.len() == 3 {
        items[2].clone()
    } else {
        let mut xs = Vec::with_capacity(items.len() - 1);
        xs.push(Term::Symbol("begin".to_string()));
        for b in items.iter().skip(2) {
            xs.push((*b).clone());
        }
        Term::list(xs)
    };

    let mut out = body_term;
    for p in ps.into_iter().rev() {
        let Term::Symbol(name) = p else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "internal fn desugaring expected symbol parameter",
            ));
        };
        out = Term::list(vec![
            Term::Symbol("fn".to_string()),
            Term::list(vec![Term::Symbol(name.clone())]),
            out,
        ]);
    }

    let Some(items2) = out.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring failed",
        ));
    };
    if items2.len() != 3 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced unexpected shape",
        ));
    }
    let params2 = &items2[1];
    let Some(ps2) = params2.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced bad params",
        ));
    };
    if ps2.len() != 1 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced non-unary params",
        ));
    }
    let Term::Symbol(param) = ps2[0] else {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "internal fn desugaring produced non-symbol param",
        ));
    };
    Ok((param.clone(), items2[2].clone()))
}
