use super::*;

pub(super) fn obligation_lint(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let mut ctx = mk_eval_ctx(limits);
    let prelude = build_prelude(&mut ctx);
    let env = prelude.env;
    let lint_fn = env.get("core/editor/lint::lint-module").ok_or_else(|| {
        ObligationError::Test("missing prelude binding core/editor/lint::lint-module".to_string())
    })?;

    let mut ok = true;
    let mut errors: Vec<String> = Vec::new();
    let mut module_terms: Vec<Term> = Vec::new();
    let mut autofix_patches: Vec<Term> = Vec::new();

    for m in modules {
        let p = Value::Data(Term::Str(m.entry.path.clone()));
        let forms = Value::Data(Term::Vector(m.forms.clone()));
        let applied = lint_fn
            .clone()
            .apply(&mut ctx, p)
            .map_err(|e| ObligationError::Test(format!("lint apply(path) failed: {e}")))?;
        let out = applied
            .apply(&mut ctx, forms)
            .map_err(|e| ObligationError::Test(format!("lint apply(forms) failed: {e}")))?;
        let term = out.to_term_for_log(ctx.protocol.map(|p| p.error));
        let diags = match term {
            Term::Vector(v) => v,
            other => {
                return Err(ObligationError::Test(format!(
                    "lint result must be vector diagnostics, got {}",
                    print_term(&other)
                )));
            }
        };

        let mut module_has_error = false;
        for d in &diags {
            let Term::Map(dm) = d else { continue };
            let is_error = match dm.get(&TermOrdKey(Term::symbol(":level"))) {
                Some(Term::Symbol(s)) => s == ":error" || s == "error",
                Some(Term::Str(s)) => s == ":error" || s == "error",
                _ => false,
            };
            if is_error {
                module_has_error = true;
                let code = match dm.get(&TermOrdKey(Term::symbol(":code"))) {
                    Some(Term::Str(s)) => s.clone(),
                    Some(Term::Symbol(s)) => s.clone(),
                    _ => "editor/lint/error".to_string(),
                };
                let msg = match dm.get(&TermOrdKey(Term::symbol(":msg"))) {
                    Some(Term::Str(s)) => s.clone(),
                    _ => "lint error".to_string(),
                };
                errors.push(format!("{}: {}: {}", m.entry.path, code, msg));
            }
        }
        if module_has_error {
            ok = false;
        }

        let autofix_patch_h = if let Some((patch_term, reasons)) =
            lint_autofix_patch_for_module(&m.entry.path, &m.forms)
        {
            let patch_h = store.put_term(&patch_term)?;
            autofix_patches.push(Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":path")),
                        Term::Str(m.entry.path.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":patch")),
                        Term::Str(patch_h.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":reasons")),
                        Term::Vector(reasons.into_iter().map(Term::Str).collect()),
                    ),
                ]
                .into_iter()
                .collect(),
            ));
            Some(patch_h)
        } else {
            None
        };

        module_terms.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(m.entry.path.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":diagnostics")),
                    Term::Vector(diags),
                ),
                (
                    TermOrdKey(Term::symbol(":autofix-patch")),
                    autofix_patch_h.map(Term::Str).unwrap_or(Term::Nil),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/lints-v0.2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":obligation")),
                Term::Str("core/obligation::lint".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(module_terms),
            ),
            (
                TermOrdKey(Term::symbol(":autofix-patches")),
                Term::Vector(autofix_patches),
            ),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::lint".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn obligation_ai_style(
    store: &EvidenceStore,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    limits: KernelLimits,
) -> Result<ObligationResult, ObligationError> {
    let lint = obligation_lint(store, manifest, modules, limits)?;
    let lint_artifact = lint.artifact.clone().ok_or_else(|| {
        ObligationError::Test("core/obligation::lint must emit artifact".to_string())
    })?;
    let lint_path = store.path_for(&lint_artifact);
    let lint_src = std::fs::read_to_string(&lint_path).map_err(|e| {
        ObligationError::Store(format!(
            "failed to read lint artifact {}: {}",
            lint_path.display(),
            e
        ))
    })?;
    let lint_term = parse_term(&lint_src)
        .map_err(|e| ObligationError::Test(format!("failed to parse lint artifact term: {e}")))?;
    let Term::Map(lint_report) = lint_term else {
        return Err(ObligationError::Test(
            "lint artifact must be a map".to_string(),
        ));
    };

    fn get_map_str_or_sym(m: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<String> {
        match m.get(&TermOrdKey(Term::symbol(key))) {
            Some(Term::Str(s)) => Some(s.clone()),
            Some(Term::Symbol(s)) => Some(s.clone()),
            _ => None,
        }
    }

    fn get_map_vec<'a>(m: &'a BTreeMap<TermOrdKey, Term>, key: &str) -> Option<&'a Vec<Term>> {
        match m.get(&TermOrdKey(Term::symbol(key))) {
            Some(Term::Vector(v)) => Some(v),
            _ => None,
        }
    }

    fn normalize_level(level: Option<String>) -> String {
        let lv = level.unwrap_or_else(|| ":error".to_string());
        let norm = lv.trim_start_matches(':').to_ascii_lowercase();
        match norm.as_str() {
            "error" => ":error".to_string(),
            "warn" | "warning" => ":warn".to_string(),
            _ => ":info".to_string(),
        }
    }

    let mut autofix_by_path: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    if let Some(autofixes) = get_map_vec(&lint_report, ":autofix-patches") {
        for entry in autofixes {
            let Term::Map(m) = entry else { continue };
            let Some(path) = get_map_str_or_sym(m, ":path") else {
                continue;
            };
            let Some(patch) = get_map_str_or_sym(m, ":patch") else {
                continue;
            };
            let reasons = get_map_vec(m, ":reasons")
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|t| match t {
                    Term::Str(s) => Some(s),
                    _ => None,
                })
                .collect::<Vec<_>>();
            autofix_by_path.insert(path, (patch, reasons));
        }
    }

    let strict_warning_codes: BTreeSet<String> = [
        "editor/lint/missing-meta",
        "editor/lint/malformed-meta",
        "editor/lint/missing-exports",
        "editor/lint/export-not-symbol",
        "editor/lint/missing-types-map",
        "editor/lint/missing-type",
        "editor/lint/missing-intent",
        "editor/lint/intent-not-string",
        "editor/lint/missing-caps",
        "editor/lint/caps-not-vector",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect();

    let mut diagnostics: Vec<Term> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let modules_vec = get_map_vec(&lint_report, ":modules").ok_or_else(|| {
        ObligationError::Test("lint artifact missing :modules diagnostics vector".to_string())
    })?;

    for (module_idx, module_entry) in modules_vec.iter().enumerate() {
        let Term::Map(module_map) = module_entry else {
            continue;
        };
        let path = get_map_str_or_sym(module_map, ":path")
            .unwrap_or_else(|| format!("<module-{}>", module_idx));
        let module_autofix = autofix_by_path.get(&path);
        let module_diags = get_map_vec(module_map, ":diagnostics")
            .cloned()
            .unwrap_or_default();
        for (diag_idx, diag_term) in module_diags.into_iter().enumerate() {
            let Term::Map(diag_map) = diag_term else {
                continue;
            };
            let code = get_map_str_or_sym(&diag_map, ":code")
                .unwrap_or_else(|| "editor/lint/error".to_string());
            let level = normalize_level(get_map_str_or_sym(&diag_map, ":level"));
            let message =
                get_map_str_or_sym(&diag_map, ":msg").unwrap_or_else(|| "lint error".to_string());
            let symbol = diag_map
                .get(&TermOrdKey(Term::symbol(":sym")))
                .cloned()
                .unwrap_or(Term::Nil);

            let mut fixes = Vec::new();
            if let Some((patch_h, reasons)) = module_autofix {
                fixes.push(Term::Map(
                    [
                        (TermOrdKey(Term::symbol(":kind")), Term::symbol(":gcpatch")),
                        (
                            TermOrdKey(Term::symbol(":schema")),
                            Term::Str("genesis/fix-schema-v1".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":patch")),
                            Term::Str(patch_h.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":intent")),
                            Term::Str(format!("apply lint autofix for {code}")),
                        ),
                        (
                            TermOrdKey(Term::symbol(":reasons")),
                            Term::Vector(reasons.iter().cloned().map(Term::Str).collect()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ));
            }

            let fail =
                level == ":error" || (level == ":warn" && strict_warning_codes.contains(&code));
            if fail {
                errors.push(format!("{path}: {code}: {message}"));
            }

            diagnostics.push(Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":id")),
                        Term::Str(format!("{path}#{diag_idx}#{code}")),
                    ),
                    (TermOrdKey(Term::symbol(":code")), Term::Str(code.clone())),
                    (TermOrdKey(Term::symbol(":severity")), Term::symbol(&level)),
                    (
                        TermOrdKey(Term::symbol(":message")),
                        Term::Str(message.clone()),
                    ),
                    (TermOrdKey(Term::symbol(":path")), Term::Str(path.clone())),
                    (TermOrdKey(Term::symbol(":symbol")), symbol),
                    (
                        TermOrdKey(Term::symbol(":module-index")),
                        Term::Int(BigInt::from(module_idx)),
                    ),
                    (
                        TermOrdKey(Term::symbol(":diag-index")),
                        Term::Int(BigInt::from(diag_idx)),
                    ),
                    (TermOrdKey(Term::symbol(":fixes")), Term::Vector(fixes)),
                ]
                .into_iter()
                .collect(),
            ));
        }
    }

    let patch_intents = autofix_by_path
        .into_iter()
        .map(|(path, (patch, reasons))| {
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":path")), Term::Str(path)),
                    (TermOrdKey(Term::symbol(":patch")), Term::Str(patch)),
                    (
                        TermOrdKey(Term::symbol(":schema")),
                        Term::Str("genesis/patch-intent-v1".to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":intent")),
                        Term::Str("apply canonical lint autofix patch".to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":reasons")),
                        Term::Vector(reasons.into_iter().map(Term::Str).collect()),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect::<Vec<_>>();

    let ok = errors.is_empty();
    let report = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/ai-style-v0.1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":schema")),
                Term::Str("genesis/diagnostics-schema-v1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":obligation")),
                Term::Str("core/obligation::ai-style".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Str(manifest.name.clone()),
            ),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":lint-artifact")),
                Term::Str(lint_artifact),
            ),
            (
                TermOrdKey(Term::symbol(":diagnostics")),
                Term::Vector(diagnostics),
            ),
            (
                TermOrdKey(Term::symbol(":patch-intents")),
                Term::Vector(patch_intents),
            ),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );

    let artifact = store.put_term(&report)?;
    Ok(ObligationResult {
        name: "core/obligation::ai-style".to_string(),
        ok,
        artifact: Some(artifact),
        errors,
    })
}

pub(super) fn lint_autofix_patch_for_module(
    module_path: &str,
    forms: &[Term],
) -> Option<(Term, Vec<String>)> {
    let mut meta_idx = None;
    let mut meta_map = None;
    for (i, f) in forms.iter().enumerate() {
        let Some((name, expr)) = parse_def(f) else {
            continue;
        };
        if name != "::meta" {
            continue;
        }
        let items = expr.as_proper_list()?;
        if items.len() != 2 || !matches!(items[0], Term::Symbol(s) if s == "quote") {
            return None;
        }
        let Term::Map(m) = &items[1] else {
            return None;
        };
        meta_idx = Some(i);
        meta_map = Some(m.clone());
        break;
    }
    let (meta_idx, mut meta_map) = (meta_idx?, meta_map?);

    let exports = match meta_map.get(&TermOrdKey(Term::symbol(":exports"))) {
        Some(Term::Vector(xs)) => xs
            .iter()
            .filter_map(|x| match x {
                Term::Symbol(s) => Some(s.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        _ => return None,
    };

    let mut reasons = Vec::new();
    let mut types = match meta_map.get(&TermOrdKey(Term::symbol(":types"))) {
        Some(Term::Map(m)) => m.clone(),
        _ => {
            reasons.push("editor/lint/missing-types-map".to_string());
            BTreeMap::new()
        }
    };

    let mut added_missing = false;
    for sym in exports {
        let key = TermOrdKey(Term::Symbol(sym));
        if let std::collections::btree_map::Entry::Vacant(e) = types.entry(key) {
            e.insert(Term::Symbol("?".to_string()));
            added_missing = true;
        }
    }
    if added_missing {
        reasons.push("editor/lint/missing-type".to_string());
    }
    if reasons.is_empty() {
        return None;
    }

    meta_map.insert(TermOrdKey(Term::symbol(":types")), Term::Map(types));
    let new_form = Term::list(vec![
        Term::symbol("def"),
        Term::symbol("::meta"),
        Term::list(vec![Term::symbol("quote"), Term::Map(meta_map)]),
    ]);
    let path = Term::Vector(vec![Term::Vector(vec![
        Term::symbol(":form"),
        Term::Int((meta_idx as i64).into()),
    ])]);
    let op = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":op")),
                Term::symbol(":replace-node"),
            ),
            (
                TermOrdKey(Term::symbol(":module-path")),
                Term::Str(module_path.to_string()),
            ),
            (TermOrdKey(Term::symbol(":path")), path),
            (TermOrdKey(Term::symbol(":new")), new_form),
        ]
        .into_iter()
        .collect(),
    );
    let patch = Term::Map(
        [
            (TermOrdKey(Term::symbol(":version")), Term::Int(1i64.into())),
            (
                TermOrdKey(Term::symbol(":intent")),
                Term::Str("lint/autofix-types".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":generated-by")),
                            Term::Str("core/obligation::lint".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":module-path")),
                            Term::Str(module_path.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":reasons")),
                            Term::Vector(reasons.iter().cloned().map(Term::Str).collect()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":ops")), Term::Vector(vec![op])),
        ]
        .into_iter()
        .collect(),
    );
    Some((patch, reasons))
}
