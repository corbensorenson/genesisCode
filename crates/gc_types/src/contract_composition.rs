use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};

use crate::ModuleForTypecheck;
use crate::module_resolution::MODULE_RESOLUTION_PROFILE_ID;
use crate::ty::{RowTail, Ty, parse_type_term};
use crate::type_compatibility::validate_effect_row_declaration;

mod analysis;

use analysis::{TypeAnalysis, analyze_type};

pub const CONTRACT_COMPOSITION_PROFILE_ID: &str = "genesis/contract-composition-profile-v0.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameAssignment {
    pub provider: String,
    pub consumer: String,
    pub boundary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptimizationPreconditions {
    pub concrete: bool,
    pub closed_shapes: bool,
    pub closed_effects: bool,
    pub pure: bool,
    pub refinement_free: bool,
    pub contract_free: bool,
    pub monomorphic: bool,
    pub eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedExport {
    pub module_path: String,
    pub symbol: String,
    pub declared_type: Term,
    pub shape_identity: [u8; 32],
    pub refinement_identity: [u8; 32],
    pub interface_identity: [u8; 32],
    pub effect_row_variables: Vec<String>,
    pub blame: BlameAssignment,
    pub optimization: OptimizationPreconditions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractCompositionReport {
    pub active: bool,
    pub ok: bool,
    pub errors_by_module: BTreeMap<String, Vec<String>>,
    pub exports: BTreeMap<String, ComposedExport>,
    pub profile_identity: Option<[u8; 32]>,
}

/// Validate and identify the opt-in v0.1 static contract-composition profile.
///
/// This report identifies declared interfaces. A caller must additionally require
/// the enclosing `TypecheckReport::ok` before treating an interface as accepted.
pub fn compose_contract_profile(modules: &[ModuleForTypecheck]) -> ContractCompositionReport {
    let active = modules.iter().any(has_profile_field);
    if !active {
        return ContractCompositionReport {
            active: false,
            ok: true,
            errors_by_module: BTreeMap::new(),
            exports: BTreeMap::new(),
            profile_identity: None,
        };
    }

    let mut errors = BTreeMap::<String, BTreeSet<String>>::new();
    let mut exports = BTreeMap::<String, ComposedExport>::new();
    let mut owners = BTreeMap::<String, BTreeSet<String>>::new();

    for module in modules {
        let path = module.path.as_str();
        let Some(Term::Map(meta)) = module.meta.as_ref() else {
            push_error(
                &mut errors,
                path,
                "[blame=boundary] contract-composition profile requires map-shaped ::meta",
            );
            continue;
        };

        require_exact_profile(meta, path, &mut errors);
        require_true(meta, ":strict-shapes", path, &mut errors);
        require_true(meta, ":strict-effects", path, &mut errors);

        let module_exports = parse_exports(meta, path, &mut errors);
        let types = parse_symbol_map(meta, ":types", path, &mut errors);
        let refinements = parse_refinements(meta, path, &mut errors);
        check_exact_keys(
            &module_exports,
            types.keys().cloned().collect(),
            ":types",
            path,
            &mut errors,
        );
        check_exact_keys(
            &module_exports,
            refinements.keys().cloned().collect(),
            ":refinements",
            path,
            &mut errors,
        );

        for symbol in module_exports {
            owners
                .entry(symbol.clone())
                .or_default()
                .insert(path.to_string());
            let Some(type_term) = types.get(&symbol) else {
                continue;
            };
            let ty = match parse_type_term(type_term) {
                Ok(ty) => ty,
                Err(message) => {
                    push_error(
                        &mut errors,
                        path,
                        format!("[blame=boundary] {symbol} has invalid contract type: {message}"),
                    );
                    continue;
                }
            };

            if contains_gradual_type(&ty) {
                push_error(
                    &mut errors,
                    path,
                    format!(
                        "[blame=provider] {symbol} contract type contains unsupported gradual `?`"
                    ),
                );
            }
            if contains_anonymous_row(&ty) {
                push_error(
                    &mut errors,
                    path,
                    format!(
                        "[blame=provider] {symbol} contract type contains unsupported anonymous row tail"
                    ),
                );
            }
            if let Err(message) = validate_effect_row_declaration(&ty, true) {
                push_error(
                    &mut errors,
                    path,
                    format!("[blame=provider] {symbol}: {message}"),
                );
            }
            validate_contract_methods(&ty, &symbol, path, &mut errors);

            let refinement_set = refinements.get(&symbol).cloned().unwrap_or_default();
            for refinement in &refinement_set {
                push_error(
                    &mut errors,
                    path,
                    format!(
                        "[blame=boundary] {symbol} refinement {refinement} is unsupported in {CONTRACT_COMPOSITION_PROFILE_ID}"
                    ),
                );
            }

            let TypeAnalysis {
                normalized_type,
                effect_row_variables,
                optimization,
            } = analyze_type(&ty, refinement_set.is_empty());
            let shape_identity = hash_term(&Term::list(vec![
                Term::symbol(CONTRACT_COMPOSITION_PROFILE_ID),
                Term::symbol("static-shape"),
                normalized_type.clone(),
            ]));
            let refinement_term =
                Term::Vector(refinement_set.iter().cloned().map(Term::Symbol).collect());
            let refinement_identity = hash_term(&Term::list(vec![
                Term::symbol(CONTRACT_COMPOSITION_PROFILE_ID),
                Term::symbol("refinements"),
                refinement_term,
            ]));
            let interface_identity = hash_term(&Term::list(vec![
                Term::symbol(CONTRACT_COMPOSITION_PROFILE_ID),
                Term::symbol("interface"),
                Term::Str(path.to_string()),
                Term::symbol(symbol.clone()),
                Term::Bytes(shape_identity.to_vec().into()),
                Term::Bytes(refinement_identity.to_vec().into()),
            ]));
            exports.insert(
                symbol.clone(),
                ComposedExport {
                    module_path: path.to_string(),
                    symbol: symbol.clone(),
                    declared_type: normalized_type,
                    shape_identity,
                    refinement_identity,
                    interface_identity,
                    effect_row_variables,
                    blame: BlameAssignment {
                        provider: format!("{path}#{symbol}"),
                        consumer: "import-or-call-site".to_string(),
                        boundary: CONTRACT_COMPOSITION_PROFILE_ID.to_string(),
                    },
                    optimization,
                },
            );
        }
    }

    for (symbol, paths) in owners {
        if paths.len() > 1 {
            let joined = paths.iter().cloned().collect::<Vec<_>>().join(", ");
            for path in paths {
                push_error(
                    &mut errors,
                    &path,
                    format!(
                        "[blame=boundary] duplicate contract export {symbol} is owned by modules [{joined}]"
                    ),
                );
            }
            exports.remove(&symbol);
        }
    }

    let errors_by_module = errors
        .into_iter()
        .map(|(path, messages)| (path, messages.into_iter().collect()))
        .collect::<BTreeMap<_, _>>();
    let ok = errors_by_module.is_empty();
    let profile_identity = ok.then(|| profile_identity(modules, &exports));

    ContractCompositionReport {
        active,
        ok,
        errors_by_module,
        exports,
        profile_identity,
    }
}

fn has_profile_field(module: &ModuleForTypecheck) -> bool {
    matches!(
        module.meta.as_ref(),
        Some(Term::Map(meta))
            if meta.contains_key(&TermOrdKey(Term::symbol(":contract-composition-profile")))
    )
}

fn require_exact_profile(
    meta: &BTreeMap<TermOrdKey, Term>,
    path: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) {
    match meta.get(&TermOrdKey(Term::symbol(":contract-composition-profile"))) {
        Some(Term::Symbol(profile)) if profile == CONTRACT_COMPOSITION_PROFILE_ID => {}
        Some(other) => push_error(
            errors,
            path,
            format!(
                "[blame=boundary] :contract-composition-profile must be exact symbol {CONTRACT_COMPOSITION_PROFILE_ID}, got {}",
                print_term(other)
            ),
        ),
        None => push_error(
            errors,
            path,
            format!(
                "[blame=boundary] every module in the contract closure must declare :contract-composition-profile {CONTRACT_COMPOSITION_PROFILE_ID}"
            ),
        ),
    }
    match meta.get(&TermOrdKey(Term::symbol(":module-profile"))) {
        Some(Term::Symbol(profile)) if profile == MODULE_RESOLUTION_PROFILE_ID => {}
        _ => push_error(
            errors,
            path,
            format!(
                "[blame=boundary] {CONTRACT_COMPOSITION_PROFILE_ID} requires :module-profile {MODULE_RESOLUTION_PROFILE_ID}"
            ),
        ),
    }
}

fn require_true(
    meta: &BTreeMap<TermOrdKey, Term>,
    field: &str,
    path: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) {
    if !matches!(
        meta.get(&TermOrdKey(Term::symbol(field))),
        Some(Term::Bool(true))
    ) {
        push_error(
            errors,
            path,
            format!("[blame=boundary] contract-composition profile requires {field} true"),
        );
    }
}

fn parse_exports(
    meta: &BTreeMap<TermOrdKey, Term>,
    path: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let Some(Term::Vector(values)) = meta.get(&TermOrdKey(Term::symbol(":exports"))) else {
        push_error(
            errors,
            path,
            "[blame=boundary] contract-composition profile requires :exports symbol vector",
        );
        return BTreeSet::new();
    };
    let mut out = BTreeSet::new();
    for value in values {
        let Term::Symbol(symbol) = value else {
            push_error(
                errors,
                path,
                format!(
                    "[blame=boundary] :exports entry must be symbol, got {}",
                    print_term(value)
                ),
            );
            continue;
        };
        if !symbol.contains("::") {
            push_error(
                errors,
                path,
                format!("[blame=boundary] contract export {symbol} must be qualified"),
            );
        }
        if !out.insert(symbol.clone()) {
            push_error(
                errors,
                path,
                format!("[blame=boundary] duplicate :exports entry {symbol}"),
            );
        }
    }
    out
}

fn parse_symbol_map(
    meta: &BTreeMap<TermOrdKey, Term>,
    field: &str,
    path: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<String, Term> {
    let Some(Term::Map(values)) = meta.get(&TermOrdKey(Term::symbol(field))) else {
        push_error(
            errors,
            path,
            format!("[blame=boundary] contract-composition profile requires {field} map"),
        );
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (key, value) in values {
        let Term::Symbol(symbol) = &key.0 else {
            push_error(
                errors,
                path,
                format!(
                    "[blame=boundary] {field} key must be symbol, got {}",
                    print_term(&key.0)
                ),
            );
            continue;
        };
        out.insert(symbol.clone(), value.clone());
    }
    out
}

fn parse_refinements(
    meta: &BTreeMap<TermOrdKey, Term>,
    path: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<String, BTreeSet<String>> {
    let raw = parse_symbol_map(meta, ":refinements", path, errors);
    let mut out = BTreeMap::new();
    for (symbol, value) in raw {
        let Term::Vector(items) = value else {
            push_error(
                errors,
                path,
                format!("[blame=boundary] refinement set for {symbol} must be a symbol vector"),
            );
            continue;
        };
        let mut set = BTreeSet::new();
        for item in items {
            let Term::Symbol(refinement) = item else {
                push_error(
                    errors,
                    path,
                    format!("[blame=boundary] refinement for {symbol} must be a symbol"),
                );
                continue;
            };
            if !refinement.contains("::") {
                push_error(
                    errors,
                    path,
                    format!(
                        "[blame=boundary] refinement {refinement} for {symbol} must be qualified"
                    ),
                );
            }
            if !set.insert(refinement.clone()) {
                push_error(
                    errors,
                    path,
                    format!("[blame=boundary] duplicate refinement {refinement} for {symbol}"),
                );
            }
        }
        out.insert(symbol, set);
    }
    out
}

fn check_exact_keys(
    expected: &BTreeSet<String>,
    actual: BTreeSet<String>,
    field: &str,
    path: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) {
    for missing in expected.difference(&actual) {
        push_error(
            errors,
            path,
            format!("[blame=boundary] {field} missing export {missing}"),
        );
    }
    for extra in actual.difference(expected) {
        push_error(
            errors,
            path,
            format!("[blame=boundary] {field} contains non-export {extra}"),
        );
    }
}

fn contains_gradual_type(ty: &Ty) -> bool {
    match ty {
        Ty::Any => true,
        Ty::Msg { payload, .. } => contains_gradual_type(payload),
        Ty::Fn { param, ret, .. } => contains_gradual_type(param) || contains_gradual_type(ret),
        Ty::Prog { ret, .. } => contains_gradual_type(ret),
        Ty::Rec { fields, .. } => fields.values().any(contains_gradual_type),
        Ty::Contract { methods, .. } => methods.values().any(contains_gradual_type),
        Ty::Int | Ty::Dec | Ty::Bool | Ty::Nil | Ty::Str | Ty::Bytes | Ty::Symbol => false,
    }
}

fn contains_anonymous_row(ty: &Ty) -> bool {
    match ty {
        Ty::Msg { payload, .. } => contains_anonymous_row(payload),
        Ty::Fn { param, ret, eff } => {
            matches!(eff.tail, RowTail::Any)
                || contains_anonymous_row(param)
                || contains_anonymous_row(ret)
        }
        Ty::Prog { ret, eff } => matches!(eff.tail, RowTail::Any) || contains_anonymous_row(ret),
        Ty::Rec { fields, tail } => {
            matches!(tail, RowTail::Any) || fields.values().any(contains_anonymous_row)
        }
        Ty::Contract { methods, tail } => {
            matches!(tail, RowTail::Any) || methods.values().any(contains_anonymous_row)
        }
        Ty::Any | Ty::Int | Ty::Dec | Ty::Bool | Ty::Nil | Ty::Str | Ty::Bytes | Ty::Symbol => {
            false
        }
    }
}

fn validate_contract_methods(
    ty: &Ty,
    export: &str,
    path: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) {
    match ty {
        Ty::Contract { methods, .. } => {
            for (operation, method) in methods {
                if !matches!(method, Ty::Fn { .. }) {
                    push_error(
                        errors,
                        path,
                        format!(
                            "[blame=provider] {export} contract method {operation} must have Fn type"
                        ),
                    );
                }
                validate_contract_methods(method, export, path, errors);
            }
        }
        Ty::Fn { param, ret, .. } => {
            validate_contract_methods(param, export, path, errors);
            validate_contract_methods(ret, export, path, errors);
        }
        Ty::Prog { ret, .. } | Ty::Msg { payload: ret, .. } => {
            validate_contract_methods(ret, export, path, errors)
        }
        Ty::Rec { fields, .. } => {
            for field in fields.values() {
                validate_contract_methods(field, export, path, errors);
            }
        }
        Ty::Any | Ty::Int | Ty::Dec | Ty::Bool | Ty::Nil | Ty::Str | Ty::Bytes | Ty::Symbol => {}
    }
}

fn profile_identity(
    modules: &[ModuleForTypecheck],
    exports: &BTreeMap<String, ComposedExport>,
) -> [u8; 32] {
    let module_terms = modules
        .iter()
        .map(|module| {
            let interface_terms = exports
                .values()
                .filter(|export| export.module_path == module.path)
                .map(|export| {
                    Term::Vector(vec![
                        Term::symbol(export.symbol.clone()),
                        Term::Bytes(export.interface_identity.to_vec().into()),
                    ])
                })
                .collect();
            Term::Vector(vec![
                Term::Str(module.path.clone()),
                Term::Vector(interface_terms),
            ])
        })
        .collect();
    hash_term(&Term::list(vec![
        Term::symbol(CONTRACT_COMPOSITION_PROFILE_ID),
        Term::symbol("profile"),
        Term::Vector(module_terms),
    ]))
}

fn push_error(
    errors: &mut BTreeMap<String, BTreeSet<String>>,
    path: &str,
    message: impl Into<String>,
) {
    errors
        .entry(path.to_string())
        .or_default()
        .insert(message.into());
}
