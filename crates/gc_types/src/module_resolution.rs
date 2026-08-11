use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{
    COREFORM_PROFILE_ID, HASH_PROFILE_ID, LANGUAGE_PROFILE_ID, SpecialForm, Term, TermOrdKey,
    hash_module, hash_term,
};
use unicode_normalization::is_nfc;

use crate::ModuleForTypecheck;

pub const MODULE_RESOLUTION_PROFILE_ID: &str = "genesis/module-resolution-profile-v0.1";

const REQUIRED_PROFILE_BINDINGS: [(&str, &str); 4] = [
    ("genesis/coreform-profile", COREFORM_PROFILE_ID),
    ("genesis/hash-profile", HASH_PROFILE_ID),
    ("genesis/language-profile", LANGUAGE_PROFILE_ID),
    (
        "genesis/module-resolution-profile",
        MODULE_RESOLUTION_PROFILE_ID,
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleResolutionReport {
    pub active: bool,
    pub ok: bool,
    pub errors_by_module: BTreeMap<String, Vec<String>>,
    pub module_identities: BTreeMap<String, [u8; 32]>,
    pub resolution_order: Vec<String>,
    pub resolution_identity: Option<[u8; 32]>,
}

#[derive(Debug, Default)]
struct Descriptor {
    imports: BTreeSet<String>,
    exports: BTreeSet<String>,
    definitions: BTreeSet<String>,
    required_profiles: BTreeMap<String, String>,
}

/// Validate the opt-in v0.1 module-resolution profile.
///
/// The supplied module order is the package manifest order and is semantic: an
/// import may resolve only to a public export of an earlier module. This is the
/// same visibility boundary used by package evaluation and rejects cycles
/// without relying on host traversal order.
pub fn resolve_module_profile(modules: &[ModuleForTypecheck]) -> ModuleResolutionReport {
    let active = modules.iter().any(has_profile_field);
    let mut errors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut identities = BTreeMap::new();
    let order = modules
        .iter()
        .map(|module| module.path.clone())
        .collect::<Vec<_>>();

    if !active {
        return ModuleResolutionReport {
            active: false,
            ok: true,
            errors_by_module: BTreeMap::new(),
            module_identities: modules
                .iter()
                .map(|module| (module.path.clone(), hash_module(&module.forms)))
                .collect(),
            resolution_order: order,
            resolution_identity: None,
        };
    }

    let mut paths = BTreeMap::<String, Vec<usize>>::new();
    for (index, module) in modules.iter().enumerate() {
        paths.entry(module.path.clone()).or_default().push(index);
        identities.insert(module.path.clone(), hash_module(&module.forms));
        if let Err(message) = validate_portable_module_path(&module.path) {
            push_error(&mut errors, &module.path, message);
        }
    }
    for (path, indices) in &paths {
        if indices.len() > 1 {
            push_error(
                &mut errors,
                path,
                format!("duplicate module path {path} at manifest indices {indices:?}"),
            );
        }
    }

    let mut descriptors = Vec::with_capacity(modules.len());
    for module in modules {
        descriptors.push(parse_descriptor(module, &mut errors));
    }

    let mut definition_owners: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    for (index, descriptor) in descriptors.iter().enumerate() {
        for definition in &descriptor.definitions {
            definition_owners
                .entry(definition.clone())
                .or_default()
                .insert(index);
        }
        for export in &descriptor.exports {
            if !descriptor.definitions.contains(export) {
                push_error(
                    &mut errors,
                    &modules[index].path,
                    format!("export {export} is not defined by this module"),
                );
            }
        }
    }

    for (symbol, owners) in &definition_owners {
        if owners.len() > 1 {
            let owner_paths = owners
                .iter()
                .map(|index| modules[*index].path.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            for owner in owners {
                push_error(
                    &mut errors,
                    &modules[*owner].path,
                    format!("definition {symbol} has multiple module owners [{owner_paths}]"),
                );
            }
        }
    }

    for (index, descriptor) in descriptors.iter().enumerate() {
        for import in &descriptor.imports {
            validate_import(
                modules,
                &descriptors,
                &definition_owners,
                index,
                import,
                &mut errors,
            );
        }

        let mut references = BTreeSet::new();
        collect_module_references(
            &modules[index].forms,
            &descriptor.definitions,
            &mut references,
        );
        for reference in references {
            let Some(owners) = definition_owners.get(&reference) else {
                continue;
            };
            if owners.contains(&index) || owners.len() != 1 {
                continue;
            }
            let Some(&owner) = owners.iter().next() else {
                continue;
            };
            if !descriptors[owner].exports.contains(&reference) {
                push_error(
                    &mut errors,
                    &modules[index].path,
                    format!(
                        "reference {reference} crosses the private boundary of module {}",
                        modules[owner].path
                    ),
                );
            } else if !descriptor.imports.contains(&reference) {
                push_error(
                    &mut errors,
                    &modules[index].path,
                    format!("reference {reference} is not declared in :imports"),
                );
            }
        }
    }

    let errors_by_module = errors
        .into_iter()
        .map(|(path, messages)| (path, messages.into_iter().collect::<Vec<_>>()))
        .collect::<BTreeMap<_, _>>();
    let ok = errors_by_module.is_empty();
    let resolution_identity = ok.then(|| {
        hash_term(&resolution_identity_term(
            modules,
            &descriptors,
            &identities,
        ))
    });

    ModuleResolutionReport {
        active,
        ok,
        errors_by_module,
        module_identities: identities,
        resolution_order: order,
        resolution_identity,
    }
}

fn parse_descriptor(
    module: &ModuleForTypecheck,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) -> Descriptor {
    let path = module.path.as_str();
    let Some(Term::Map(meta)) = module.meta.as_ref() else {
        push_error(
            errors,
            path,
            "module-resolution profile requires map-shaped ::meta".to_string(),
        );
        return Descriptor::default();
    };

    match meta.get(&TermOrdKey(Term::symbol(":module-profile"))) {
        Some(Term::Symbol(profile)) if profile == MODULE_RESOLUTION_PROFILE_ID => {}
        Some(other) => push_error(
            errors,
            path,
            format!(
                ":module-profile must be exact symbol {MODULE_RESOLUTION_PROFILE_ID}, got {}",
                gc_coreform::print_term(other)
            ),
        ),
        None => push_error(
            errors,
            path,
            format!(
                "every module in the resolution closure must declare :module-profile {MODULE_RESOLUTION_PROFILE_ID}"
            ),
        ),
    }

    let imports = parse_unique_symbol_vector(meta, ":imports", path, errors);
    let exports = parse_unique_symbol_vector(meta, ":exports", path, errors);
    let required_profiles = parse_required_profiles(meta, path, errors);
    let definitions = module
        .forms
        .iter()
        .filter_map(parse_def_name)
        .filter(|name| name != "::meta")
        .collect();

    Descriptor {
        imports,
        exports,
        definitions,
        required_profiles,
    }
}

fn parse_unique_symbol_vector(
    meta: &BTreeMap<TermOrdKey, Term>,
    field: &str,
    path: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let Some(value) = meta.get(&TermOrdKey(Term::symbol(field))) else {
        push_error(
            errors,
            path,
            format!("module-resolution profile requires {field}"),
        );
        return BTreeSet::new();
    };
    let Term::Vector(items) = value else {
        push_error(errors, path, format!("{field} must be a vector of symbols"));
        return BTreeSet::new();
    };
    let mut output = BTreeSet::new();
    for item in items {
        let Term::Symbol(symbol) = item else {
            push_error(errors, path, format!("{field} entries must be symbols"));
            continue;
        };
        if !output.insert(symbol.clone()) {
            push_error(errors, path, format!("duplicate {field} entry {symbol}"));
        }
        if let Err(message) = validate_qualified_symbol(symbol) {
            push_error(
                errors,
                path,
                format!("invalid {field} entry {symbol}: {message}"),
            );
        }
    }
    output
}

fn parse_required_profiles(
    meta: &BTreeMap<TermOrdKey, Term>,
    path: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<String, String> {
    let Some(value) = meta.get(&TermOrdKey(Term::symbol(":requires-profiles"))) else {
        push_error(
            errors,
            path,
            "module-resolution profile requires :requires-profiles".to_string(),
        );
        return BTreeMap::new();
    };
    let Term::Map(entries) = value else {
        push_error(
            errors,
            path,
            ":requires-profiles must be a symbol-to-symbol map".to_string(),
        );
        return BTreeMap::new();
    };

    let mut parsed = BTreeMap::new();
    for (key, value) in entries {
        let (Term::Symbol(key), Term::Symbol(value)) = (&key.0, value) else {
            push_error(
                errors,
                path,
                ":requires-profiles must be a symbol-to-symbol map".to_string(),
            );
            continue;
        };
        parsed.insert(key.clone(), value.clone());
    }
    let expected = REQUIRED_PROFILE_BINDINGS
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect::<BTreeMap<_, _>>();
    if parsed != expected {
        push_error(
            errors,
            path,
            format!(
                ":requires-profiles must exactly bind [{}]",
                REQUIRED_PROFILE_BINDINGS
                    .iter()
                    .map(|(key, value)| format!("{key} {value}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
    parsed
}

fn validate_import(
    modules: &[ModuleForTypecheck],
    descriptors: &[Descriptor],
    owners: &BTreeMap<String, BTreeSet<usize>>,
    consumer: usize,
    import: &str,
    errors: &mut BTreeMap<String, BTreeSet<String>>,
) {
    let path = modules[consumer].path.as_str();
    let Some(import_owners) = owners.get(import) else {
        push_error(
            errors,
            path,
            format!("import {import} has no owner in the supplied package closure"),
        );
        return;
    };
    if import_owners.len() != 1 {
        push_error(
            errors,
            path,
            format!("import {import} has ambiguous ownership"),
        );
        return;
    }
    let Some(&owner) = import_owners.iter().next() else {
        push_error(errors, path, format!("import {import} has no owner"));
        return;
    };
    if owner == consumer {
        push_error(
            errors,
            path,
            format!("local symbol {import} must not be listed in :imports"),
        );
    } else if !descriptors[owner].exports.contains(import) {
        push_error(
            errors,
            path,
            format!(
                "import {import} is private to module {}",
                modules[owner].path
            ),
        );
    } else if owner > consumer {
        push_error(
            errors,
            path,
            format!(
                "import {import} resolves to later module {}; imports must target earlier manifest entries (cycles and forward imports are forbidden)",
                modules[owner].path
            ),
        );
    }
}

fn collect_module_references(
    forms: &[Term],
    local_definitions: &BTreeSet<String>,
    output: &mut BTreeSet<String>,
) {
    let bound = BTreeSet::new();
    for form in forms {
        if let Some(items) = form.as_proper_list()
            && items.len() == 3
            && matches!(&items[0], Term::Symbol(symbol) if SpecialForm::from_symbol(symbol) == Some(SpecialForm::Def))
        {
            collect_references(items[2], local_definitions, &bound, output);
        } else {
            collect_references(form, local_definitions, &bound, output);
        }
    }
}

fn collect_references(
    term: &Term,
    local_definitions: &BTreeSet<String>,
    bound: &BTreeSet<String>,
    output: &mut BTreeSet<String>,
) {
    match term {
        Term::Symbol(symbol) => {
            if !symbol.starts_with(':')
                && !local_definitions.contains(symbol)
                && !bound.contains(symbol)
            {
                output.insert(symbol.clone());
            }
        }
        Term::Pair(_, _) => {
            let Some(items) = term.as_proper_list() else {
                collect_pair_terms(term, local_definitions, bound, output);
                return;
            };
            if items.is_empty() {
                return;
            }
            let special = match &items[0] {
                Term::Symbol(symbol) => SpecialForm::from_symbol(symbol),
                _ => None,
            };
            match special {
                Some(SpecialForm::Quote) => {}
                Some(SpecialForm::Fn) if items.len() >= 3 => {
                    let mut body_bound = bound.clone();
                    if let Some(parameters) = items[1].as_proper_list() {
                        for parameter in parameters {
                            if let Term::Symbol(symbol) = parameter {
                                body_bound.insert(symbol.clone());
                            }
                        }
                    }
                    for body in &items[2..] {
                        collect_references(body, local_definitions, &body_bound, output);
                    }
                }
                Some(SpecialForm::Let) if items.len() >= 3 => {
                    let mut body_bound = bound.clone();
                    if let Some(bindings) = items[1].as_proper_list() {
                        for binding in bindings {
                            if let Some(pair) = binding.as_proper_list()
                                && pair.len() == 2
                            {
                                collect_references(pair[1], local_definitions, bound, output);
                                if let Term::Symbol(symbol) = &pair[0] {
                                    body_bound.insert(symbol.clone());
                                }
                            }
                        }
                    }
                    for body in &items[2..] {
                        collect_references(body, local_definitions, &body_bound, output);
                    }
                }
                Some(SpecialForm::Prim) => {
                    for argument in items.iter().skip(2) {
                        collect_references(argument, local_definitions, bound, output);
                    }
                }
                Some(SpecialForm::Def) if items.len() == 3 => {
                    collect_references(items[2], local_definitions, bound, output);
                }
                _ => {
                    for item in items {
                        collect_references(item, local_definitions, bound, output);
                    }
                }
            }
        }
        Term::Vector(items) => {
            for item in items {
                collect_references(item, local_definitions, bound, output);
            }
        }
        Term::Map(entries) => {
            for value in entries.values() {
                collect_references(value, local_definitions, bound, output);
            }
        }
        Term::Nil | Term::Bool(_) | Term::Int(_) | Term::Str(_) | Term::Bytes(_) => {}
    }
}

fn collect_pair_terms(
    term: &Term,
    local_definitions: &BTreeSet<String>,
    bound: &BTreeSet<String>,
    output: &mut BTreeSet<String>,
) {
    if let Term::Pair(car, cdr) = term {
        collect_references(car, local_definitions, bound, output);
        collect_references(cdr, local_definitions, bound, output);
    }
}

fn resolution_identity_term(
    modules: &[ModuleForTypecheck],
    descriptors: &[Descriptor],
    identities: &BTreeMap<String, [u8; 32]>,
) -> Term {
    let entries = modules
        .iter()
        .zip(descriptors)
        .map(|(module, descriptor)| {
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":content-h")),
                        Term::Bytes(identities[&module.path].to_vec().into()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":exports")),
                        Term::Vector(
                            descriptor
                                .exports
                                .iter()
                                .cloned()
                                .map(Term::Symbol)
                                .collect(),
                        ),
                    ),
                    (
                        TermOrdKey(Term::symbol(":imports")),
                        Term::Vector(
                            descriptor
                                .imports
                                .iter()
                                .cloned()
                                .map(Term::Symbol)
                                .collect(),
                        ),
                    ),
                    (
                        TermOrdKey(Term::symbol(":path")),
                        Term::Str(module.path.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":requires-profiles")),
                        Term::Map(
                            descriptor
                                .required_profiles
                                .iter()
                                .map(|(key, value)| {
                                    (
                                        TermOrdKey(Term::Symbol(key.clone())),
                                        Term::Symbol(value.clone()),
                                    )
                                })
                                .collect(),
                        ),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Symbol(MODULE_RESOLUTION_PROFILE_ID.to_string()),
            ),
            (TermOrdKey(Term::symbol(":modules")), Term::Vector(entries)),
        ]
        .into_iter()
        .collect(),
    )
}

fn has_profile_field(module: &ModuleForTypecheck) -> bool {
    matches!(
        module.meta.as_ref(),
        Some(Term::Map(meta))
            if meta.contains_key(&TermOrdKey(Term::symbol(":module-profile")))
    )
}

fn parse_def_name(term: &Term) -> Option<String> {
    let items = term.as_proper_list()?;
    if items.len() != 3
        || !matches!(&items[0], Term::Symbol(symbol) if SpecialForm::from_symbol(symbol) == Some(SpecialForm::Def))
    {
        return None;
    }
    match &items[1] {
        Term::Symbol(name) => Some(name.clone()),
        _ => None,
    }
}

fn validate_portable_module_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("module path must be non-empty".to_string());
    }
    if !is_nfc(path) {
        return Err("module path must be Unicode NFC".to_string());
    }
    if path.starts_with('/') || path.contains('\\') {
        return Err("module path must be base-relative and use '/' separators".to_string());
    }
    let components = path.split('/').collect::<Vec<_>>();
    if components
        .iter()
        .any(|component| component.is_empty() || *component == "." || *component == "..")
    {
        return Err("module path must not contain empty, '.', or '..' components".to_string());
    }
    if components[0].ends_with(':') {
        return Err("module path must not contain a drive prefix".to_string());
    }
    Ok(())
}

fn validate_qualified_symbol(symbol: &str) -> Result<(), &'static str> {
    let Some((namespace, name)) = symbol.split_once("::") else {
        return Err("expected namespace::name");
    };
    if namespace.is_empty()
        || name.is_empty()
        || name.contains("::")
        || symbol.chars().any(char::is_whitespace)
    {
        return Err("namespace and name must be non-empty with exactly one '::'");
    }
    if namespace.contains('\\')
        || namespace.starts_with('/')
        || namespace
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err("namespace must be a portable slash-separated path");
    }
    Ok(())
}

fn push_error(errors: &mut BTreeMap<String, BTreeSet<String>>, path: &str, message: String) {
    errors.entry(path.to_string()).or_default().insert(message);
}
