#[derive(Clone, Debug, Eq, PartialEq)]
struct LintAutofix {
    path: String,
    hash: String,
    reasons: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LintDiagnostic {
    code: String,
    level: String,
    message: String,
    path: String,
    symbol: Term,
}

fn evidence_hash_term(term: &Term) -> String {
    blake3::hash(print_term(term).as_bytes())
        .to_hex()
        .to_string()
}

fn valid_artifact_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn decode_artifact_transport(
    transport: &Term,
) -> Result<(Term, BTreeMap<String, Term>), ObligationError> {
    let map = exact_map(
        transport,
        "artifact transport",
        &[":artifact-terms", ":final"],
    )?;
    let Some(Term::Vector(rows)) = map_field(map, ":artifact-terms") else {
        return Err(authority_error(
            "artifact transport :artifact-terms must be vector",
        ));
    };
    let mut artifacts = BTreeMap::new();
    for row in rows {
        let row = exact_map(row, "side artifact", &[":hash", ":term"])?;
        let hash = string_field(row, ":hash", "side artifact")?;
        let term = required_field(row, ":term", "side artifact")?.clone();
        if !valid_artifact_hash(&hash) || evidence_hash_term(&term) != hash {
            return Err(authority_error("side artifact hash mismatch"));
        }
        if artifacts.insert(hash, term).is_some() {
            return Err(authority_error("duplicate side artifact hash"));
        }
    }
    Ok((
        required_field(map, ":final", "artifact transport")?.clone(),
        artifacts,
    ))
}

fn lint_diagnostic(term: &Term, module_path: &str) -> Result<LintDiagnostic, ObligationError> {
    let map = exact_map(
        term,
        "lint diagnostic",
        &[":code", ":level", ":msg", ":path", ":sym"],
    )?;
    let path = string_field(map, ":path", "lint diagnostic")?;
    let code = string_field(map, ":code", "lint diagnostic")?;
    let message = string_field(map, ":msg", "lint diagnostic")?;
    let level = match map_field(map, ":level") {
        Some(Term::Symbol(level)) if matches!(level.as_str(), ":error" | ":warn") => level.clone(),
        _ => {
            return Err(authority_error(
                "lint diagnostic :level must be :error or :warn",
            ));
        }
    };
    let symbol = required_field(map, ":sym", "lint diagnostic")?.clone();
    if path != module_path || !matches!(symbol, Term::Nil | Term::Symbol(_)) {
        return Err(authority_error("lint diagnostic observation mismatch"));
    }
    Ok(LintDiagnostic {
        code,
        level,
        message,
        path,
        symbol,
    })
}

fn lint_autofixes(report: &BTreeMap<TermOrdKey, Term>) -> Result<Vec<LintAutofix>, ObligationError> {
    let Some(Term::Vector(rows)) = map_field(report, ":autofix-patches") else {
        return Err(authority_error(
            "lint report :autofix-patches must be vector",
        ));
    };
    let mut paths = BTreeSet::new();
    rows.iter()
        .map(|row| {
            let row = exact_map(row, "lint autofix", &[":patch", ":path", ":reasons"])?;
            let path = string_field(row, ":path", "lint autofix")?;
            let hash = string_field(row, ":patch", "lint autofix")?;
            let reasons = string_vector(
                required_field(row, ":reasons", "lint autofix")?,
                "lint autofix :reasons",
            )?;
            if !paths.insert(path.clone())
                || !valid_artifact_hash(&hash)
                || reasons.is_empty()
                || reasons.iter().any(|reason| {
                    !matches!(
                        reason.as_str(),
                        "editor/lint/missing-types-map" | "editor/lint/missing-type"
                    )
                })
            {
                return Err(authority_error("lint autofix identity mismatch"));
            }
            Ok(LintAutofix {
                path,
                hash,
                reasons,
            })
        })
        .collect()
}

fn expected_lint_patch(
    module: &LoadedModule,
    autofix: &LintAutofix,
) -> Result<Term, ObligationError> {
    let (meta_index, mut meta) = module
        .forms
        .iter()
        .enumerate()
        .find_map(|(index, form)| {
            let (name, expression) = parse_def(form)?;
            if name != "::meta" {
                return None;
            }
            let quoted = expression.as_proper_list()?;
            match quoted.as_slice() {
                [Term::Symbol(head), Term::Map(meta)] if head == "quote" => {
                    Some((index, meta.clone()))
                }
                _ => None,
            }
        })
        .ok_or_else(|| authority_error("lint autofix has no canonical metadata target"))?;
    let exports = match meta.get(&TermOrdKey(Term::symbol(":exports"))) {
        Some(Term::Vector(exports)) => exports
            .iter()
            .filter_map(|export| match export {
                Term::Symbol(symbol) => Some(symbol.clone()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        _ => return Err(authority_error("lint autofix has no exports vector")),
    };
    let mut reasons = Vec::new();
    let mut types = match meta.get(&TermOrdKey(Term::symbol(":types"))) {
        Some(Term::Map(types)) => types.clone(),
        _ => {
            reasons.push("editor/lint/missing-types-map".to_string());
            BTreeMap::new()
        }
    };
    let mut added = false;
    for export in exports {
        if let std::collections::btree_map::Entry::Vacant(entry) =
            types.entry(TermOrdKey(Term::Symbol(export)))
        {
            entry.insert(Term::symbol("?"));
            added = true;
        }
    }
    if added {
        reasons.push("editor/lint/missing-type".to_string());
    }
    if reasons.is_empty() || reasons != autofix.reasons {
        return Err(authority_error("lint autofix reasons contradict module metadata"));
    }
    meta.insert(
        TermOrdKey(Term::symbol(":types")),
        Term::Map(types),
    );
    let new_form = Term::list(vec![
        Term::symbol("def"),
        Term::symbol("::meta"),
        Term::list(vec![Term::symbol("quote"), Term::Map(meta)]),
    ]);
    let operation = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":module-path")),
                Term::Str(autofix.path.clone()),
            ),
            (TermOrdKey(Term::symbol(":new")), new_form),
            (
                TermOrdKey(Term::symbol(":op")),
                Term::symbol(":replace-node"),
            ),
            (
                TermOrdKey(Term::symbol(":path")),
                Term::Vector(vec![Term::Vector(vec![
                    Term::symbol(":form"),
                    Term::Int(BigInt::from(meta_index)),
                ])]),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok(Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":intent")),
                Term::Str("lint/autofix-types".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":ops")),
                Term::Vector(vec![operation]),
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
                            Term::Str(autofix.path.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":reasons")),
                            Term::Vector(
                                reasons.into_iter().map(Term::Str).collect(),
                            ),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (TermOrdKey(Term::symbol(":version")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    ))
}

fn validate_lint_patch(
    term: &Term,
    autofix: &LintAutofix,
    module: &LoadedModule,
) -> Result<(), ObligationError> {
    if term != &expected_lint_patch(module, autofix)? {
        return Err(authority_error("lint patch contradicts canonical module metadata"));
    }
    Ok(())
}

fn validate_lint_report(
    report: &Term,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    outer_ok: bool,
    outer_errors: &[String],
    side_artifacts: &BTreeMap<String, Term>,
) -> Result<(), ObligationError> {
    let report = exact_map(
        report,
        "lint report",
        &[
            ":autofix-patches",
            ":errors",
            ":kind",
            ":modules",
            ":obligation",
            ":ok",
            ":package",
        ],
    )?;
    let errors = string_vector(
        required_field(report, ":errors", "lint report")?,
        "lint report :errors",
    )?;
    if string_field(report, ":kind", "lint report")? != "genesis/lints-v0.2"
        || string_field(report, ":obligation", "lint report")? != "core/obligation::lint"
        || string_field(report, ":package", "lint report")? != manifest.name
        || bool_field(report, ":ok", "lint report")? != outer_ok
        || errors != outer_errors
    {
        return Err(authority_error("lint report identity mismatch"));
    }
    let autofixes = lint_autofixes(report)?;
    let autofix_by_path = autofixes
        .iter()
        .map(|autofix| (autofix.path.as_str(), autofix))
        .collect::<BTreeMap<_, _>>();
    let Some(Term::Vector(rows)) = map_field(report, ":modules") else {
        return Err(authority_error("lint report :modules must be vector"));
    };
    if rows.len() != modules.len() {
        return Err(authority_error("lint report module inventory mismatch"));
    }
    let mut derived_errors = Vec::new();
    for (row, module) in rows.iter().zip(modules) {
        let row = exact_map(
            row,
            "lint module",
            &[":autofix-patch", ":diagnostics", ":path"],
        )?;
        let path = string_field(row, ":path", "lint module")?;
        if path != module.entry.path {
            return Err(authority_error("lint module path mismatch"));
        }
        let expected_autofix = autofix_by_path.get(path.as_str()).map(|row| row.hash.as_str());
        match map_field(row, ":autofix-patch") {
            Some(Term::Nil) if expected_autofix.is_none() => {}
            Some(Term::Str(hash)) if Some(hash.as_str()) == expected_autofix => {}
            _ => return Err(authority_error("lint module autofix mismatch")),
        }
        let Some(Term::Vector(diagnostics)) = map_field(row, ":diagnostics") else {
            return Err(authority_error("lint module diagnostics must be vector"));
        };
        for diagnostic in diagnostics {
            let diagnostic = lint_diagnostic(diagnostic, &path)?;
            if diagnostic.level == ":error" {
                derived_errors.push(format!(
                    "{}: {}: {}",
                    path, diagnostic.code, diagnostic.message
                ));
            }
        }
    }
    if derived_errors != errors || outer_ok != errors.is_empty() {
        return Err(authority_error("lint report aggregate mismatch"));
    }
    let expected_hashes = autofixes
        .iter()
        .map(|autofix| autofix.hash.as_str())
        .collect::<BTreeSet<_>>();
    if expected_hashes.len() != side_artifacts.len()
        || side_artifacts
            .keys()
            .any(|hash| !expected_hashes.contains(hash.as_str()))
    {
        return Err(authority_error("lint side artifact inventory mismatch"));
    }
    for autofix in &autofixes {
        let module = modules
            .iter()
            .find(|module| module.entry.path == autofix.path)
            .ok_or_else(|| authority_error("lint autofix references unknown module"))?;
        validate_lint_patch(
            side_artifacts
                .get(&autofix.hash)
                .ok_or_else(|| authority_error("missing lint patch side artifact"))?,
            autofix,
            module,
        )?;
    }
    Ok(())
}

fn normalized_style_level(level: &str) -> &'static str {
    match level {
        ":error" | "error" => ":error",
        ":warn" | "warn" | ":warning" | "warning" => ":warn",
        _ => ":info",
    }
}

fn strict_style_code(code: &str) -> bool {
    matches!(
        code,
        "editor/lint/missing-meta"
            | "editor/lint/malformed-meta"
            | "editor/lint/missing-exports"
            | "editor/lint/export-not-symbol"
            | "editor/lint/missing-types-map"
            | "editor/lint/missing-type"
            | "editor/lint/missing-intent"
            | "editor/lint/intent-not-string"
            | "editor/lint/missing-caps"
            | "editor/lint/caps-not-vector"
    )
}

fn validate_ai_style_report(
    report: &Term,
    manifest: &PackageManifest,
    modules: &[LoadedModule],
    outer_ok: bool,
    outer_errors: &[String],
    side_artifacts: &BTreeMap<String, Term>,
) -> Result<(), ObligationError> {
    let report = exact_map(
        report,
        "AI-style report",
        &[
            ":diagnostics",
            ":errors",
            ":kind",
            ":lint-artifact",
            ":obligation",
            ":ok",
            ":package",
            ":patch-intents",
            ":schema",
        ],
    )?;
    let errors = string_vector(
        required_field(report, ":errors", "AI-style report")?,
        "AI-style report :errors",
    )?;
    let lint_hash = string_field(report, ":lint-artifact", "AI-style report")?;
    if string_field(report, ":kind", "AI-style report")? != "genesis/ai-style-v0.1"
        || string_field(report, ":schema", "AI-style report")?
            != "genesis/diagnostics-schema-v1"
        || string_field(report, ":obligation", "AI-style report")?
            != "core/obligation::ai-style"
        || string_field(report, ":package", "AI-style report")? != manifest.name
        || bool_field(report, ":ok", "AI-style report")? != outer_ok
        || errors != outer_errors
    {
        return Err(authority_error("AI-style report identity mismatch"));
    }
    let lint_term = side_artifacts
        .get(&lint_hash)
        .ok_or_else(|| authority_error("AI-style lint artifact is missing"))?;
    let lint_map = exact_map(
        lint_term,
        "AI-style lint report",
        &[
            ":autofix-patches",
            ":errors",
            ":kind",
            ":modules",
            ":obligation",
            ":ok",
            ":package",
        ],
    )?;
    let lint_errors = string_vector(
        required_field(lint_map, ":errors", "AI-style lint report")?,
        "AI-style lint report :errors",
    )?;
    let lint_ok = bool_field(lint_map, ":ok", "AI-style lint report")?;
    let patch_artifacts = side_artifacts
        .iter()
        .filter(|(hash, _)| *hash != &lint_hash)
        .map(|(hash, term)| (hash.clone(), term.clone()))
        .collect();
    validate_lint_report(
        lint_term,
        manifest,
        modules,
        lint_ok,
        &lint_errors,
        &patch_artifacts,
    )?;
    let autofixes = lint_autofixes(lint_map)?;
    let autofix_by_path = autofixes
        .iter()
        .map(|autofix| (autofix.path.as_str(), autofix))
        .collect::<BTreeMap<_, _>>();
    let Some(Term::Vector(lint_modules)) = map_field(lint_map, ":modules") else {
        return Err(authority_error("AI-style lint modules must be vector"));
    };
    let Some(Term::Vector(style_rows)) = map_field(report, ":diagnostics") else {
        return Err(authority_error("AI-style diagnostics must be vector"));
    };
    let mut expected = Vec::new();
    for (module_index, module) in lint_modules.iter().enumerate() {
        let module = exact_map(
            module,
            "AI-style lint module",
            &[":autofix-patch", ":diagnostics", ":path"],
        )?;
        let path = string_field(module, ":path", "AI-style lint module")?;
        let Some(Term::Vector(diagnostics)) = map_field(module, ":diagnostics") else {
            return Err(authority_error("AI-style lint diagnostics must be vector"));
        };
        for (diagnostic_index, diagnostic) in diagnostics.iter().enumerate() {
            expected.push((
                module_index,
                diagnostic_index,
                lint_diagnostic(diagnostic, &path)?,
            ));
        }
    }
    if expected.len() != style_rows.len() {
        return Err(authority_error("AI-style diagnostic inventory mismatch"));
    }
    let mut derived_errors = Vec::new();
    for (row, (module_index, diagnostic_index, diagnostic)) in
        style_rows.iter().zip(expected.iter())
    {
        let row = exact_map(
            row,
            "AI-style diagnostic",
            &[
                ":code",
                ":diag-index",
                ":fixes",
                ":id",
                ":message",
                ":module-index",
                ":path",
                ":severity",
                ":symbol",
            ],
        )?;
        let severity = normalized_style_level(&diagnostic.level);
        let expected_id = format!(
            "{}#{}#{}",
            diagnostic.path, diagnostic_index, diagnostic.code
        );
        if string_field(row, ":code", "AI-style diagnostic")? != diagnostic.code
            || string_field(row, ":id", "AI-style diagnostic")? != expected_id
            || string_field(row, ":message", "AI-style diagnostic")? != diagnostic.message
            || string_field(row, ":path", "AI-style diagnostic")? != diagnostic.path
            || !matches!(map_field(row, ":module-index"), Some(Term::Int(value)) if value == &BigInt::from(*module_index))
            || !matches!(map_field(row, ":diag-index"), Some(Term::Int(value)) if value == &BigInt::from(*diagnostic_index))
            || !matches!(map_field(row, ":severity"), Some(Term::Symbol(value)) if value == severity)
            || map_field(row, ":symbol") != Some(&diagnostic.symbol)
        {
            return Err(authority_error("AI-style diagnostic observation mismatch"));
        }
        let autofix = autofix_by_path.get(diagnostic.path.as_str());
        let Some(Term::Vector(fixes)) = map_field(row, ":fixes") else {
            return Err(authority_error("AI-style diagnostic fixes must be vector"));
        };
        if fixes.len() != usize::from(autofix.is_some()) {
            return Err(authority_error("AI-style diagnostic fix inventory mismatch"));
        }
        if let Some(autofix) = autofix {
            let fix = exact_map(
                &fixes[0],
                "AI-style fix",
                &[":intent", ":kind", ":patch", ":reasons", ":schema"],
            )?;
            if string_field(fix, ":intent", "AI-style fix")?
                != format!("apply lint autofix for {}", diagnostic.code)
                || !matches!(map_field(fix, ":kind"), Some(Term::Symbol(value)) if value == ":gcpatch")
                || string_field(fix, ":patch", "AI-style fix")? != autofix.hash
                || string_field(fix, ":schema", "AI-style fix")? != "genesis/fix-schema-v1"
                || string_vector(
                    required_field(fix, ":reasons", "AI-style fix")?,
                    "AI-style fix :reasons",
                )? != autofix.reasons
            {
                return Err(authority_error("AI-style fix mismatch"));
            }
        }
        if severity == ":error" || (severity == ":warn" && strict_style_code(&diagnostic.code)) {
            derived_errors.push(format!(
                "{}: {}: {}",
                diagnostic.path, diagnostic.code, diagnostic.message
            ));
        }
    }
    let Some(Term::Vector(intents)) = map_field(report, ":patch-intents") else {
        return Err(authority_error("AI-style patch intents must be vector"));
    };
    if intents.len() != autofixes.len() {
        return Err(authority_error("AI-style patch intent inventory mismatch"));
    }
    for (intent, autofix) in intents.iter().zip(&autofixes) {
        let intent = exact_map(
            intent,
            "AI-style patch intent",
            &[":intent", ":patch", ":path", ":reasons", ":schema"],
        )?;
        if string_field(intent, ":intent", "AI-style patch intent")?
            != "apply canonical lint autofix patch"
            || string_field(intent, ":patch", "AI-style patch intent")? != autofix.hash
            || string_field(intent, ":path", "AI-style patch intent")? != autofix.path
            || string_field(intent, ":schema", "AI-style patch intent")?
                != "genesis/patch-intent-v1"
            || string_vector(
                required_field(intent, ":reasons", "AI-style patch intent")?,
                "AI-style patch intent :reasons",
            )? != autofix.reasons
        {
            return Err(authority_error("AI-style patch intent mismatch"));
        }
    }
    if derived_errors != errors || outer_ok != errors.is_empty() {
        return Err(authority_error("AI-style aggregate mismatch"));
    }
    Ok(())
}
