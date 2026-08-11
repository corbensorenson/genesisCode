fn report_map<'a>(
    term: &'a Term,
    context: &str,
    fields: &[&str],
) -> Result<&'a BTreeMap<TermOrdKey, Term>, ObligationError> {
    let Term::Map(map) = term else {
        return Err(typecheck_error(format!(
            "{context} must be a map, got {}",
            print_term(term)
        )));
    };
    if map.len() != fields.len()
        || fields
            .iter()
            .any(|field| !map.contains_key(&TermOrdKey(Term::symbol(*field))))
    {
        return Err(typecheck_error(format!(
            "{context} must contain exactly fields [{}]",
            fields.join(", ")
        )));
    }
    Ok(map)
}

fn report_field<'a>(map: &'a BTreeMap<TermOrdKey, Term>, field: &str) -> &'a Term {
    // report_map has already established this closed field set.
    &map[&TermOrdKey(Term::symbol(field))]
}

fn report_bool(
    map: &BTreeMap<TermOrdKey, Term>,
    field: &str,
    context: &str,
) -> Result<bool, ObligationError> {
    match report_field(map, field) {
        Term::Bool(value) => Ok(*value),
        value => Err(typecheck_error(format!(
            "{context} {field} must be bool, got {}",
            print_term(value)
        ))),
    }
}

fn report_string(
    map: &BTreeMap<TermOrdKey, Term>,
    field: &str,
    context: &str,
) -> Result<String, ObligationError> {
    match report_field(map, field) {
        Term::Str(value) => Ok(value.clone()),
        value => Err(typecheck_error(format!(
            "{context} {field} must be string, got {}",
            print_term(value)
        ))),
    }
}

fn report_symbol(
    map: &BTreeMap<TermOrdKey, Term>,
    field: &str,
    context: &str,
) -> Result<String, ObligationError> {
    match report_field(map, field) {
        Term::Symbol(value) => Ok(value.clone()),
        value => Err(typecheck_error(format!(
            "{context} {field} must be symbol, got {}",
            print_term(value)
        ))),
    }
}

fn report_strings(term: &Term, context: &str) -> Result<Vec<String>, ObligationError> {
    let Term::Vector(values) = term else {
        return Err(typecheck_error(format!("{context} must be a vector")));
    };
    values
        .iter()
        .map(|value| match value {
            Term::Str(value) => Ok(value.clone()),
            other => Err(typecheck_error(format!(
                "{context} must contain strings, got {}",
                print_term(other)
            ))),
        })
        .collect()
}

fn report_symbols(term: &Term, context: &str) -> Result<BTreeSet<String>, ObligationError> {
    let Term::Vector(values) = term else {
        return Err(typecheck_error(format!("{context} must be a vector")));
    };
    let mut out = BTreeSet::new();
    for value in values {
        let Term::Symbol(symbol) = value else {
            return Err(typecheck_error(format!(
                "{context} must contain symbols, got {}",
                print_term(value)
            )));
        };
        if !out.insert(symbol.clone()) {
            return Err(typecheck_error(format!(
                "{context} contains duplicate symbol {symbol}"
            )));
        }
    }
    Ok(out)
}

fn decode_diagnostic(term: &Term) -> Result<TypecheckDiagnostic, ObligationError> {
    let context = "typecheck diagnostic";
    let map = report_map(
        term,
        context,
        &[
            ":code",
            ":domain",
            ":id",
            ":message",
            ":module",
            ":ordinal",
            ":severity",
        ],
    )?;
    let code = report_symbol(map, ":code", context)?;
    let severity = report_symbol(map, ":severity", context)?;
    let domain = report_symbol(map, ":domain", context)?;
    if domain != "typechecker"
        || !matches!(
            (code.as_str(), severity.as_str()),
            ("typecheck/error", "error") | ("typecheck/warning", "warning")
        )
    {
        return Err(typecheck_error(
            "typecheck diagnostic domain/code/severity identity mismatch",
        ));
    }
    let ordinal = match report_field(map, ":ordinal") {
        Term::Int(value) => value.to_u64().ok_or_else(|| {
            typecheck_error("typecheck diagnostic :ordinal must be a non-negative u64")
        })?,
        _ => {
            return Err(typecheck_error(
                "typecheck diagnostic :ordinal must be an integer",
            ));
        }
    };
    Ok(TypecheckDiagnostic {
        id: report_string(map, ":id", context)?,
        code,
        severity,
        module_path: report_string(map, ":module", context)?,
        ordinal,
        message: report_string(map, ":message", context)?,
    })
}

fn decode_export_effect(term: &Term) -> Result<TypecheckExportEffectReport, ObligationError> {
    let context = "typecheck export effect";
    let map = report_map(term, context, &[":name", ":ops", ":unknown"])?;
    Ok(TypecheckExportEffectReport {
        name: report_symbol(map, ":name", context)?,
        ops: report_symbols(report_field(map, ":ops"), "typecheck export effect :ops")?,
        unknown: report_bool(map, ":unknown", context)?,
    })
}

fn decode_export_type(term: &Term) -> Result<TypecheckExportTypeReport, ObligationError> {
    let context = "typecheck export type";
    let map = report_map(
        term,
        context,
        &[
            ":declared",
            ":errors",
            ":inferred",
            ":name",
            ":ok",
            ":warnings",
        ],
    )?;
    let declared = match report_field(map, ":declared") {
        Term::Nil => None,
        value => Some(value.clone()),
    };
    let errors = report_strings(
        report_field(map, ":errors"),
        "typecheck export type :errors",
    )?;
    let warnings = report_strings(
        report_field(map, ":warnings"),
        "typecheck export type :warnings",
    )?;
    let ok = report_bool(map, ":ok", context)?;
    if ok != errors.is_empty() {
        return Err(typecheck_error(
            "typecheck export type :ok disagrees with :errors",
        ));
    }
    Ok(TypecheckExportTypeReport {
        name: report_symbol(map, ":name", context)?,
        declared,
        inferred: report_field(map, ":inferred").clone(),
        ok,
        errors,
        warnings,
    })
}

fn decode_module_report(term: &Term) -> Result<TypecheckModuleReport, ObligationError> {
    let context = "typecheck module report";
    let map = report_map(
        term,
        context,
        &[
            ":errors",
            ":exports",
            ":inferred-ops",
            ":ok",
            ":path",
            ":types",
            ":unknown-ops",
            ":warnings",
        ],
    )?;
    let errors = report_strings(report_field(map, ":errors"), "typecheck module :errors")?;
    let warnings = report_strings(report_field(map, ":warnings"), "typecheck module :warnings")?;
    let ok = report_bool(map, ":ok", context)?;
    if ok != errors.is_empty() {
        return Err(typecheck_error(
            "typecheck module :ok disagrees with :errors",
        ));
    }
    let Term::Vector(export_terms) = report_field(map, ":exports") else {
        return Err(typecheck_error(
            "typecheck module :exports must be a vector",
        ));
    };
    let Term::Vector(type_terms) = report_field(map, ":types") else {
        return Err(typecheck_error("typecheck module :types must be a vector"));
    };
    Ok(TypecheckModuleReport {
        path: report_string(map, ":path", context)?,
        ok,
        errors,
        warnings,
        inferred_ops: report_symbols(
            report_field(map, ":inferred-ops"),
            "typecheck module :inferred-ops",
        )?,
        unknown_ops: report_bool(map, ":unknown-ops", context)?,
        export_effects: export_terms
            .iter()
            .map(decode_export_effect)
            .collect::<Result<_, _>>()?,
        export_types: type_terms
            .iter()
            .map(decode_export_type)
            .collect::<Result<_, _>>()?,
    })
}

fn requested_export_names(module: &TypecheckModuleInput) -> Result<Vec<String>, ObligationError> {
    let Some(Term::Map(meta)) = module.meta.as_ref() else {
        return Ok(Vec::new());
    };
    let Some(Term::Vector(exports)) = meta.get(&TermOrdKey(Term::symbol(":exports"))) else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    let mut unique = BTreeSet::new();
    for export in exports {
        let Term::Symbol(name) = export else {
            continue;
        };
        if !unique.insert(name.clone()) {
            return Err(typecheck_error(format!(
                "typecheck request module {} contains duplicate export {name}",
                module.path
            )));
        }
        names.push(name.clone());
    }
    Ok(names)
}

fn requested_declared_types(module: &TypecheckModuleInput) -> BTreeMap<String, Term> {
    let Some(Term::Map(meta)) = module.meta.as_ref() else {
        return BTreeMap::new();
    };
    let Some(Term::Map(types)) = meta.get(&TermOrdKey(Term::symbol(":types"))) else {
        return BTreeMap::new();
    };
    types
        .iter()
        .filter_map(|(name, declared)| match &name.0 {
            Term::Symbol(name) => Some((name.clone(), declared.clone())),
            _ => None,
        })
        .collect()
}

fn validate_typecheck_report_binding(
    requested: &[TypecheckModuleInput],
    reported: &[TypecheckModuleReport],
) -> Result<(), ObligationError> {
    if requested.len() != reported.len() {
        return Err(typecheck_error(format!(
            "typecheck report module count mismatch: requested {}, reported {}",
            requested.len(),
            reported.len()
        )));
    }

    let mut requested_paths = BTreeSet::new();
    for (index, (request, report)) in requested.iter().zip(reported).enumerate() {
        if !requested_paths.insert(request.path.clone()) {
            return Err(typecheck_error(format!(
                "typecheck request contains duplicate module path {}",
                request.path
            )));
        }
        if report.path != request.path {
            return Err(typecheck_error(format!(
                "typecheck report module {index} path mismatch: requested {}, reported {}",
                request.path, report.path
            )));
        }

        let expected_exports = requested_export_names(request)?;
        let effect_exports = report
            .export_effects
            .iter()
            .map(|export| export.name.clone())
            .collect::<Vec<_>>();
        let type_exports = report
            .export_types
            .iter()
            .map(|export| export.name.clone())
            .collect::<Vec<_>>();
        if effect_exports != expected_exports || type_exports != expected_exports {
            return Err(typecheck_error(format!(
                "typecheck report export inventory mismatch for {}: requested [{}], effects [{}], types [{}]",
                request.path,
                expected_exports.join(","),
                effect_exports.join(","),
                type_exports.join(",")
            )));
        }

        let declared_types = requested_declared_types(request);
        for export in &report.export_types {
            let expected = declared_types.get(&export.name).cloned();
            if export.declared != expected {
                return Err(typecheck_error(format!(
                    "typecheck report declared type mismatch for {}::{}",
                    request.path, export.name
                )));
            }
        }
    }
    Ok(())
}

fn request_uses_profile_negotiation(modules: &[TypecheckModuleInput]) -> bool {
    modules.iter().any(|module| {
        matches!(
            module.meta.as_ref(),
            Some(Term::Map(meta))
                if meta.contains_key(&TermOrdKey(Term::symbol(":profile-negotiation")))
                    || meta.contains_key(&TermOrdKey(Term::symbol(
                        ":package-profile-requirements"
                    )))
        )
    })
}

fn validate_profile_negotiation(
    term: &Term,
    requested_modules: &[TypecheckModuleInput],
) -> Result<bool, ObligationError> {
    let context = "typecheck profile negotiation";
    let map = report_map(
        term,
        context,
        &[
            ":active",
            ":errors",
            ":identity",
            ":kind",
            ":ok",
            ":requirements",
            ":selected",
        ],
    )?;
    if report_symbol(map, ":kind", context)? != "genesis/profile-negotiation-v0.1" {
        return Err(typecheck_error(
            "typecheck profile negotiation :kind mismatch",
        ));
    }
    let active = report_bool(map, ":active", context)?;
    if active != request_uses_profile_negotiation(requested_modules) {
        return Err(typecheck_error(
            "typecheck profile negotiation :active disagrees with the request",
        ));
    }

    let Term::Map(errors) = report_field(map, ":errors") else {
        return Err(typecheck_error(
            "typecheck profile negotiation :errors must be a map",
        ));
    };
    let requested_paths = requested_modules
        .iter()
        .map(|module| module.path.as_str())
        .collect::<BTreeSet<_>>();
    for (path, messages) in errors {
        let Term::Str(path) = &path.0 else {
            return Err(typecheck_error(
                "typecheck profile negotiation :errors keys must be module-path strings",
            ));
        };
        if !requested_paths.contains(path.as_str()) {
            return Err(typecheck_error(format!(
                "typecheck profile negotiation reports errors for unrequested module {path}"
            )));
        }
        let messages = report_strings(messages, "typecheck profile negotiation module errors")?;
        if messages.is_empty() {
            return Err(typecheck_error(
                "typecheck profile negotiation error entries must not be empty",
            ));
        }
    }

    let Term::Map(requirements) = report_field(map, ":requirements") else {
        return Err(typecheck_error(
            "typecheck profile negotiation :requirements must be a map",
        ));
    };
    for (family, requirement) in requirements {
        if !matches!(&family.0, Term::Symbol(_)) {
            return Err(typecheck_error(
                "typecheck profile negotiation requirement families must be symbols",
            ));
        }
        let requirement = report_map(
            requirement,
            "typecheck profile negotiation requirement",
            &[":mode", ":profile"],
        )?;
        let mode = report_symbol(
            requirement,
            ":mode",
            "typecheck profile negotiation requirement",
        )?;
        if !matches!(mode.as_str(), "exact" | "minimum") {
            return Err(typecheck_error(
                "typecheck profile negotiation requirement :mode must be exact or minimum",
            ));
        }
        let _ = report_symbol(
            requirement,
            ":profile",
            "typecheck profile negotiation requirement",
        )?;
    }

    let Term::Map(selected) = report_field(map, ":selected") else {
        return Err(typecheck_error(
            "typecheck profile negotiation :selected must be a map",
        ));
    };
    for (family, profile) in selected {
        if !matches!((&family.0, profile), (Term::Symbol(_), Term::Symbol(_))) {
            return Err(typecheck_error(
                "typecheck profile negotiation selections must map symbol families to symbol profiles",
            ));
        }
    }

    let identity_valid = match report_field(map, ":identity") {
        Term::Nil => false,
        Term::Bytes(bytes) if bytes.len() == 32 => true,
        Term::Bytes(_) => {
            return Err(typecheck_error(
                "typecheck profile negotiation :identity bytes must be exactly 32 bytes",
            ));
        }
        _ => {
            return Err(typecheck_error(
                "typecheck profile negotiation :identity must be nil or 32 bytes",
            ));
        }
    };
    let ok = report_bool(map, ":ok", context)?;
    if identity_valid != (active && ok) {
        return Err(typecheck_error(
            "typecheck profile negotiation :identity presence disagrees with :active/:ok",
        ));
    }
    if !active {
        if !ok || !errors.is_empty() || !requirements.is_empty() || !selected.is_empty() {
            return Err(typecheck_error(
                "inactive typecheck profile negotiation must be the exact successful empty report",
            ));
        }
    } else if ok {
        let requirement_families = requirements.keys().collect::<BTreeSet<_>>();
        let selected_families = selected.keys().collect::<BTreeSet<_>>();
        if !errors.is_empty()
            || requirements.len() != 4
            || selected.len() != 4
            || requirement_families != selected_families
        {
            return Err(typecheck_error(
                "successful active typecheck profile negotiation is incomplete or contradictory",
            ));
        }
    } else if errors.is_empty() {
        return Err(typecheck_error(
            "failed active typecheck profile negotiation must report module errors",
        ));
    }
    Ok(ok)
}

fn expected_diagnostics(modules: &[TypecheckModuleReport]) -> Vec<TypecheckDiagnostic> {
    let mut out = Vec::new();
    for module in modules {
        for (ordinal, message) in module.errors.iter().enumerate() {
            out.push(TypecheckDiagnostic {
                id: format!("{}#error#{ordinal}", module.path),
                code: "typecheck/error".to_string(),
                severity: "error".to_string(),
                module_path: module.path.clone(),
                ordinal: u64::try_from(ordinal).unwrap_or(u64::MAX),
                message: message.clone(),
            });
        }
        for (ordinal, message) in module.warnings.iter().enumerate() {
            out.push(TypecheckDiagnostic {
                id: format!("{}#warning#{ordinal}", module.path),
                code: "typecheck/warning".to_string(),
                severity: "warning".to_string(),
                module_path: module.path.clone(),
                ordinal: u64::try_from(ordinal).unwrap_or(u64::MAX),
                message: message.clone(),
            });
        }
    }
    out
}

fn decode_typecheck_report(
    term: Term,
    requested_modules: &[TypecheckModuleInput],
) -> Result<AuthoritativeTypecheckReport, ObligationError> {
    let context = "typecheck report";
    let map = report_map(
        &term,
        context,
        &[
            ":diagnostics",
            ":errors",
            ":kind",
            ":modules",
            ":ok",
            ":profile-negotiation",
            ":warnings",
        ],
    )?;
    if report_string(map, ":kind", context)? != "genesis/typecheck-v0.2" {
        return Err(typecheck_error("typecheck report :kind mismatch"));
    }
    let Term::Vector(module_terms) = report_field(map, ":modules") else {
        return Err(typecheck_error(
            "typecheck report :modules must be a vector",
        ));
    };
    let modules = module_terms
        .iter()
        .map(decode_module_report)
        .collect::<Result<Vec<_>, _>>()?;
    validate_typecheck_report_binding(requested_modules, &modules)?;
    let Term::Vector(diagnostic_terms) = report_field(map, ":diagnostics") else {
        return Err(typecheck_error(
            "typecheck report :diagnostics must be a vector",
        ));
    };
    let diagnostics = diagnostic_terms
        .iter()
        .map(decode_diagnostic)
        .collect::<Result<Vec<_>, _>>()?;
    if diagnostics != expected_diagnostics(&modules) {
        return Err(typecheck_error(
            "typecheck report diagnostics disagree with module diagnostics",
        ));
    }
    let errors = report_strings(report_field(map, ":errors"), "typecheck report :errors")?;
    let warnings = report_strings(report_field(map, ":warnings"), "typecheck report :warnings")?;
    let expected_errors: Vec<String> = modules
        .iter()
        .flat_map(|module| module.errors.iter().cloned())
        .collect();
    let expected_warnings: Vec<String> = modules
        .iter()
        .flat_map(|module| module.warnings.iter().cloned())
        .collect();
    if errors != expected_errors || warnings != expected_warnings {
        return Err(typecheck_error(
            "typecheck report aggregate messages disagree with module reports",
        ));
    }
    let profile_ok =
        validate_profile_negotiation(report_field(map, ":profile-negotiation"), requested_modules)?;
    let ok = report_bool(map, ":ok", context)?;
    if ok != (profile_ok && modules.iter().all(|module| module.ok)) {
        return Err(typecheck_error(
            "typecheck report :ok disagrees with module/profile reports",
        ));
    }
    Ok(AuthoritativeTypecheckReport {
        ok,
        errors,
        warnings,
        diagnostics,
        modules,
        term,
    })
}
