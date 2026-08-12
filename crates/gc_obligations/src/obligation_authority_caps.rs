fn module_observations(modules: &[LoadedModule]) -> Vec<Term> {
    modules
        .iter()
        .map(|module| {
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":forms")),
                        Term::Vector(module.forms.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":path")),
                        Term::Str(module.entry.path.clone()),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect()
}

fn capability_inputs(modules: &[LoadedModule], tests: &[TestRun]) -> Term {
    let test_observations = tests
        .iter()
        .filter_map(|test| {
            let log = test.effect_log.as_ref()?;
            let used_ops = log
                .entries
                .iter()
                .map(|entry| entry.op.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(Term::symbol)
                .collect();
            Some(Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":name")),
                        Term::Str(test.id.test_name.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":suite")),
                        Term::symbol(test.id.suite_sym.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":used-ops")),
                        Term::Vector(used_ops),
                    ),
                ]
                .into_iter()
                .collect(),
            ))
        })
        .collect();
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(module_observations(modules)),
            ),
            (
                TermOrdKey(Term::symbol(":tests")),
                Term::Vector(test_observations),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn typecheck_inputs(modules: &[LoadedModule]) -> Term {
    Term::Map(
        [(
            TermOrdKey(Term::symbol(":modules")),
            Term::Vector(module_observations(modules)),
        )]
        .into_iter()
        .collect(),
    )
}

fn typecheck_module_inputs(modules: &[LoadedModule]) -> Vec<TypecheckModuleInput> {
    modules
        .iter()
        .map(|module| TypecheckModuleInput {
            path: module.entry.path.clone(),
            forms: module.forms.clone(),
            meta: extract_meta_static(&module.forms),
        })
        .collect()
}

fn validate_typecheck_obligation_report(
    report: &Term,
    modules: &[LoadedModule],
    outer_ok: bool,
    outer_errors: &[String],
) -> Result<(), ObligationError> {
    let decoded = decode_typecheck_report(report.clone(), &typecheck_module_inputs(modules))?;
    if decoded.ok != outer_ok || decoded.errors != outer_errors {
        return Err(authority_error(
            "typecheck report disagrees with the obligation result",
        ));
    }
    Ok(())
}

fn validate_capabilities_report(
    report: &Term,
    manifest: &PackageManifest,
    outer_ok: bool,
    outer_errors: &[String],
) -> Result<(), ObligationError> {
    let map = exact_map(
        report,
        "capabilities-declared report",
        &[":errors", ":kind", ":ok", ":package"],
    )?;
    let errors = string_vector(
        required_field(map, ":errors", "capabilities-declared report")?,
        "capabilities-declared report :errors",
    )?;
    if string_field(map, ":kind", "capabilities-declared report")?
        != "genesis/caps-declared-v0.2"
        || string_field(map, ":package", "capabilities-declared report")? != manifest.name
        || bool_field(map, ":ok", "capabilities-declared report")? != outer_ok
        || errors != outer_errors
        || outer_ok != outer_errors.is_empty()
    {
        return Err(authority_error(
            "capabilities-declared report identity or aggregate mismatch",
        ));
    }
    Ok(())
}
