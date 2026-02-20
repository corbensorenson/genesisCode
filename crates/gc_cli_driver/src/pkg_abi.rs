use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{MemLimits, StepLimit};
use gc_obligations::{
    CoreformFrontend, hash_module_forms_with_frontend,
    parse_canonicalize_module_source_with_frontend,
};
use gc_pkg::PackageManifest;
use gc_types::{ModuleForTypecheck, typecheck_package};

use crate::pkg_workspace_ops::LocalPkgResult;

#[derive(Debug, Default, Clone)]
struct TypeEffectSummary {
    ops: BTreeSet<String>,
    open: bool,
}

#[derive(Debug)]
struct LoadedModuleAbi {
    path: String,
    hash: [u8; 32],
    meta: Option<Term>,
}

pub(crate) fn handle_abi(
    pkg_toml: &Path,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<LocalPkgResult, String> {
    let (manifest, pkg_dir) = PackageManifest::load(pkg_toml).map_err(|e| e.to_string())?;

    let mut loaded_modules = Vec::new();
    let mut typecheck_modules = Vec::new();

    for module in &manifest.modules {
        let abs_path = pkg_dir.join(&module.path);
        let src = std::fs::read_to_string(&abs_path)
            .map_err(|e| format!("read module {}: {e}", abs_path.display()))?;

        let forms =
            parse_canonicalize_module_source_with_frontend(&src, frontend, step_limit, mem_limits)
                .map_err(|e| format!("parse/canonicalize module {}: {e}", module.path))?;
        let hash = hash_module_forms_with_frontend(&forms, frontend, step_limit, mem_limits)
            .map_err(|e| format!("hash module {}: {e}", module.path))?;
        let meta = extract_meta_static(&forms);

        loaded_modules.push(LoadedModuleAbi {
            path: module.path.clone(),
            hash,
            meta: meta.clone(),
        });
        typecheck_modules.push(ModuleForTypecheck {
            path: module.path.clone(),
            forms,
            meta,
        });
    }

    let typecheck = typecheck_package(&typecheck_modules);
    let report_by_path: BTreeMap<String, gc_types::ModuleReport> = typecheck
        .modules
        .iter()
        .cloned()
        .map(|m| (m.path.clone(), m))
        .collect();

    let mut obligations_set: BTreeSet<String> = BTreeSet::new();
    obligations_set.extend(manifest.obligations.iter().cloned());

    let mut required_caps = BTreeSet::new();
    let mut export_index: BTreeMap<String, Vec<Term>> = BTreeMap::new();
    let mut module_terms = Vec::new();
    let mut export_count: usize = 0;

    for module in &loaded_modules {
        let exports = module
            .meta
            .as_ref()
            .map(meta_exports)
            .unwrap_or_default()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let declared_caps = module
            .meta
            .as_ref()
            .map(meta_caps)
            .unwrap_or_default()
            .into_iter()
            .collect::<BTreeSet<_>>();
        let declared_types = module.meta.as_ref().map(meta_types).unwrap_or_default();
        let intent = module
            .meta
            .as_ref()
            .and_then(|meta| meta_optional_str(meta, ":intent"));

        let report = report_by_path.get(&module.path);
        let inferred_ops = report
            .map(|r| r.inferred_effects.ops.clone())
            .unwrap_or_default();
        let unknown_ops = report.map(|r| r.inferred_effects.unknown).unwrap_or(false);

        let mut effect_by_export: BTreeMap<String, TypeEffectSummary> = BTreeMap::new();
        if let Some(r) = report {
            for eff in &r.export_effects {
                effect_by_export.insert(
                    eff.name.clone(),
                    TypeEffectSummary {
                        ops: eff.effects.ops.clone(),
                        open: eff.effects.unknown,
                    },
                );
            }
        }

        let mut type_by_export: BTreeMap<String, (Option<Term>, Term)> = BTreeMap::new();
        if let Some(r) = report {
            for ty in &r.export_types {
                type_by_export.insert(ty.name.clone(), (ty.declared.clone(), ty.inferred.clone()));
            }
        }
        for (name, declared) in &declared_types {
            type_by_export.entry(name.clone()).or_insert_with(|| {
                (
                    Some(declared.clone()),
                    Term::Symbol("?".to_string()), // if typecheck report is absent, stay gradual
                )
            });
        }

        let mut module_export_names: BTreeSet<String> = BTreeSet::new();
        module_export_names.extend(exports.iter().cloned());
        module_export_names.extend(effect_by_export.keys().cloned());
        module_export_names.extend(type_by_export.keys().cloned());

        let mut module_required_caps = declared_caps.clone();
        module_required_caps.extend(inferred_ops.iter().cloned());

        let mut export_entries = Vec::new();
        for export_name in module_export_names {
            let (declared_type, inferred_type) = type_by_export
                .get(&export_name)
                .cloned()
                .unwrap_or_else(|| (None, Term::Symbol("?".to_string())));

            let mut type_effect = TypeEffectSummary::default();
            if let Some(t) = declared_type.as_ref() {
                collect_effect_summary_from_type_term(t, &mut type_effect);
            }
            collect_effect_summary_from_type_term(&inferred_type, &mut type_effect);

            let mut export_effect = effect_by_export
                .get(&export_name)
                .cloned()
                .unwrap_or_default();
            export_effect.ops.extend(type_effect.ops.iter().cloned());
            export_effect.open = export_effect.open || type_effect.open;

            let mut contract_ops = if let Some(t) = declared_type.as_ref() {
                contract_ops_from_type_term(t)
            } else {
                Vec::new()
            };
            if contract_ops.is_empty() {
                contract_ops = contract_ops_from_type_term(&inferred_type);
            }

            module_required_caps.extend(export_effect.ops.iter().cloned());
            required_caps.extend(export_effect.ops.iter().cloned());

            let export_term = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":name")),
                        Term::Symbol(export_name.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":module")),
                        Term::Str(module.path.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":declared-type")),
                        declared_type.clone().unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":inferred-type")),
                        inferred_type.clone(),
                    ),
                    (
                        TermOrdKey(Term::symbol(":effect-signature-ops")),
                        symbol_vec(export_effect.ops.iter().cloned()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":effect-signature-open")),
                        Term::Bool(export_effect.open),
                    ),
                    (
                        TermOrdKey(Term::symbol(":required-caps")),
                        symbol_vec(export_effect.ops.iter().cloned()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":contract-ops")),
                        Term::Vector(contract_ops),
                    ),
                ]
                .into_iter()
                .collect(),
            );

            export_entries.push(export_term.clone());
            export_index
                .entry(export_name)
                .or_default()
                .push(export_term.clone());
            export_count += 1;
        }

        required_caps.extend(module_required_caps.iter().cloned());
        required_caps.extend(declared_caps.iter().cloned());
        required_caps.extend(inferred_ops.iter().cloned());

        let report_ok = report.map(|r| r.ok).unwrap_or(false);
        let report_errors = report
            .map(|r| Term::Vector(r.errors.iter().cloned().map(Term::Str).collect()))
            .unwrap_or_else(|| Term::Vector(Vec::new()));
        let report_warnings = report
            .map(|r| Term::Vector(r.warnings.iter().cloned().map(Term::Str).collect()))
            .unwrap_or_else(|| Term::Vector(Vec::new()));

        let declared_types_term = Term::Map(
            declared_types
                .iter()
                .map(|(name, ty)| (TermOrdKey(Term::Symbol(name.clone())), ty.clone()))
                .collect(),
        );

        module_terms.push(Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(module.path.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":hash")),
                    Term::Str(hex32(&module.hash)),
                ),
                (
                    TermOrdKey(Term::symbol(":intent")),
                    intent.clone().map(Term::Str).unwrap_or_else(|| Term::Nil),
                ),
                (
                    TermOrdKey(Term::symbol(":exports")),
                    symbol_vec(exports.iter().cloned()),
                ),
                (
                    TermOrdKey(Term::symbol(":declared-caps")),
                    symbol_vec(declared_caps.iter().cloned()),
                ),
                (
                    TermOrdKey(Term::symbol(":required-caps")),
                    symbol_vec(module_required_caps.iter().cloned()),
                ),
                (
                    TermOrdKey(Term::symbol(":inferred-ops")),
                    symbol_vec(inferred_ops.iter().cloned()),
                ),
                (
                    TermOrdKey(Term::symbol(":unknown-ops")),
                    Term::Bool(unknown_ops),
                ),
                (
                    TermOrdKey(Term::symbol(":declared-types")),
                    declared_types_term,
                ),
                (
                    TermOrdKey(Term::symbol(":typecheck-ok")),
                    Term::Bool(report_ok),
                ),
                (TermOrdKey(Term::symbol(":typecheck-errors")), report_errors),
                (
                    TermOrdKey(Term::symbol(":typecheck-warnings")),
                    report_warnings,
                ),
                (
                    TermOrdKey(Term::symbol(":exports-abi")),
                    Term::Vector(export_entries),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    }

    let index_term = Term::Map(
        export_index
            .into_iter()
            .map(|(name, entries)| {
                let value = if entries.len() == 1 {
                    entries[0].clone()
                } else {
                    Term::Vector(entries)
                };
                (TermOrdKey(Term::Symbol(name)), value)
            })
            .collect(),
    );

    let package_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":name")),
                Term::Str(manifest.name.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":version")),
                Term::Str(manifest.version.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":manifest")),
                Term::Str(pkg_toml.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":root")),
                Term::Str(pkg_dir.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":caps-policy")),
                manifest
                    .caps_policy
                    .clone()
                    .map(Term::Str)
                    .unwrap_or_else(|| Term::Nil),
            ),
        ]
        .into_iter()
        .collect(),
    );

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":schema")),
                Term::Str("genesis/pkg-abi-v0.1".to_string()),
            ),
            (TermOrdKey(Term::symbol(":package")), package_term),
            (
                TermOrdKey(Term::symbol(":obligations")),
                symbol_vec(obligations_set.iter().cloned()),
            ),
            (
                TermOrdKey(Term::symbol(":required-caps")),
                symbol_vec(required_caps.iter().cloned()),
            ),
            (
                TermOrdKey(Term::symbol(":module-count")),
                Term::Int((loaded_modules.len() as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":export-count")),
                Term::Int((export_count as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":typecheck-ok")),
                Term::Bool(typecheck.ok),
            ),
            (
                TermOrdKey(Term::symbol(":typecheck-errors")),
                Term::Vector(typecheck.errors.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":typecheck-warnings")),
                Term::Vector(typecheck.warnings.iter().cloned().map(Term::Str).collect()),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(module_terms),
            ),
            (TermOrdKey(Term::symbol(":index")), index_term),
        ]
        .into_iter()
        .collect(),
    );

    Ok(LocalPkgResult {
        kind: "genesis/pkg-abi-v0.1",
        log_op: "pkg-abi",
        program_hash: hash_term(&value),
        value,
    })
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

fn extract_meta_static(forms: &[Term]) -> Option<Term> {
    for f in forms {
        let Some((name, expr)) = parse_def(f) else {
            continue;
        };
        if name != "::meta" {
            continue;
        }
        let Some(items) = expr.as_proper_list() else {
            continue;
        };
        if items.len() == 2
            && matches!(items[0], Term::Symbol(s) if s == "quote")
            && let Term::Map(m) = items[1]
        {
            return Some(Term::Map(m.clone()));
        }
    }
    None
}

fn meta_exports(meta: &Term) -> Vec<String> {
    let Term::Map(m) = meta else {
        return Vec::new();
    };
    let Some(Term::Vector(xs)) = m.get(&TermOrdKey(Term::symbol(":exports"))) else {
        return Vec::new();
    };
    xs.iter()
        .filter_map(|x| match x {
            Term::Symbol(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn meta_caps(meta: &Term) -> Vec<String> {
    let Term::Map(m) = meta else {
        return Vec::new();
    };
    let Some(Term::Vector(xs)) = m.get(&TermOrdKey(Term::symbol(":caps"))) else {
        return Vec::new();
    };
    xs.iter()
        .filter_map(|x| match x {
            Term::Symbol(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn meta_types(meta: &Term) -> BTreeMap<String, Term> {
    let Term::Map(m) = meta else {
        return BTreeMap::new();
    };
    let Some(Term::Map(mm)) = m.get(&TermOrdKey(Term::symbol(":types"))) else {
        return BTreeMap::new();
    };
    mm.iter()
        .filter_map(|(k, v)| match &k.0 {
            Term::Symbol(name) => Some((name.clone(), v.clone())),
            _ => None,
        })
        .collect()
}

fn meta_optional_str(meta: &Term, key: &str) -> Option<String> {
    let Term::Map(m) = meta else { return None };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn collect_effect_summary_from_type_term(t: &Term, out: &mut TypeEffectSummary) {
    if let Some(items) = t.as_proper_list()
        && !items.is_empty()
    {
        if let Term::Symbol(head) = items[0]
            && head == "Eff"
            && items.len() == 3
        {
            match items[1] {
                Term::Vector(xs) => {
                    for op in xs {
                        match op {
                            Term::Symbol(s) => {
                                out.ops.insert(s.clone());
                            }
                            _ => out.open = true,
                        }
                    }
                }
                _ => out.open = true,
            }
            match items[2] {
                Term::Nil => {}
                Term::Symbol(_) => out.open = true,
                _ => out.open = true,
            }
            return;
        }
        for item in items {
            collect_effect_summary_from_type_term(item, out);
        }
        return;
    }

    match t {
        Term::Vector(xs) => {
            for x in xs {
                collect_effect_summary_from_type_term(x, out);
            }
        }
        Term::Map(m) => {
            for v in m.values() {
                collect_effect_summary_from_type_term(v, out);
            }
        }
        _ => {}
    }
}

fn contract_ops_from_type_term(ty: &Term) -> Vec<Term> {
    let Some(items) = ty.as_proper_list() else {
        return Vec::new();
    };
    if items.len() != 3 || !matches!(items[0], Term::Symbol(s) if s == "Contract") {
        return Vec::new();
    }
    let Term::Vector(methods) = items[1] else {
        return Vec::new();
    };

    let mut out: BTreeMap<String, Term> = BTreeMap::new();
    for method in methods {
        let Term::Vector(pair) = method else {
            continue;
        };
        if pair.len() != 2 {
            continue;
        }
        let Term::Symbol(op) = &pair[0] else {
            continue;
        };
        let mut eff = TypeEffectSummary::default();
        collect_effect_summary_from_type_term(&pair[1], &mut eff);
        let entry = Term::Map(
            [
                (TermOrdKey(Term::symbol(":op")), Term::Symbol(op.clone())),
                (TermOrdKey(Term::symbol(":type")), pair[1].clone()),
                (
                    TermOrdKey(Term::symbol(":effect-signature-ops")),
                    symbol_vec(eff.ops.into_iter()),
                ),
                (
                    TermOrdKey(Term::symbol(":effect-signature-open")),
                    Term::Bool(eff.open),
                ),
            ]
            .into_iter()
            .collect(),
        );
        out.insert(op.clone(), entry);
    }
    out.into_values().collect()
}

fn symbol_vec<I>(iter: I) -> Term
where
    I: IntoIterator<Item = String>,
{
    Term::Vector(iter.into_iter().map(Term::Symbol).collect())
}

fn hex32(bytes: &[u8; 32]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}
