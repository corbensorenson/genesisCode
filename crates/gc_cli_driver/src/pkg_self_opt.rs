use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_module, print_module};
use gc_kernel::{MemLimits, StepLimit};
use gc_obligations::{
    CoreformFrontend, EvidenceStore, PackageManifest, pack_with_frontend,
    parse_canonicalize_module_source_with_frontend, test_package_with_step_limit_and_frontend,
};

use crate::pkg_workspace_ops::LocalPkgResult;

#[derive(Debug, Clone)]
struct CandidateRewrite {
    path: String,
    abs: PathBuf,
    old_src: String,
    new_src: String,
    old_hash: [u8; 32],
    new_hash: [u8; 32],
    stats: gc_opt::OptimizeStats,
}

pub(crate) fn handle_self_optimize(
    pkg_toml: &Path,
    caps_override: Option<&Path>,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
    dry_run: bool,
) -> Result<LocalPkgResult, String> {
    let (manifest, pkg_dir) = PackageManifest::load(pkg_toml).map_err(|e| e.to_string())?;
    let store = EvidenceStore::open(&pkg_dir).map_err(|e| e.to_string())?;
    let original_pkg_toml = std::fs::read(pkg_toml).map_err(|e| e.to_string())?;

    let mut rewrites = Vec::new();
    for m in &manifest.modules {
        let abs = pkg_dir.join(&m.path);
        let old_src = std::fs::read_to_string(&abs)
            .map_err(|e| format!("read module {}: {e}", abs.display()))?;
        let old_forms = parse_canonicalize_module_source_with_frontend(
            &old_src, frontend, step_limit, mem_limits,
        )
        .map_err(|e| format!("parse/canonicalize {}: {e}", m.path))?;
        let old_hash = hash_module(&old_forms);
        let (opt_forms_raw, opt_report) = gc_opt::optimize_module_with_report(&old_forms);
        let opt_forms = gc_coreform::canonicalize_module(opt_forms_raw)
            .map_err(|e| format!("canonicalize optimized {}: {e}", m.path))?;
        let new_hash = hash_module(&opt_forms);
        if new_hash != old_hash {
            rewrites.push(CandidateRewrite {
                path: m.path.clone(),
                abs,
                old_src,
                new_src: print_module(&opt_forms),
                old_hash,
                new_hash,
                stats: opt_report.stats,
            });
        }
    }

    let proposed_count = rewrites.len() as i64;
    if rewrites.is_empty() {
        let value = Term::Map(
            [
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":pkg")),
                    Term::Str(pkg_toml.display().to_string()),
                ),
                (TermOrdKey(Term::symbol(":dry-run")), Term::Bool(dry_run)),
                (
                    TermOrdKey(Term::symbol(":proposed-count")),
                    Term::Int(proposed_count.into()),
                ),
                (TermOrdKey(Term::symbol(":promotable")), Term::Bool(true)),
                (TermOrdKey(Term::symbol(":promoted")), Term::Bool(false)),
                (
                    TermOrdKey(Term::symbol(":message")),
                    Term::Str("no optimizer rewrite candidates".to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        return Ok(LocalPkgResult {
            kind: "genesis/pkg-self-optimize-v0.1",
            log_op: "pkg-self-optimize",
            program_hash: gc_coreform::hash_term(&value),
            value,
        });
    }

    let mut errors: Vec<String> = Vec::new();
    let mut translation_obligation_added = false;
    let mut package_artifact: Option<String> = None;
    let mut acceptance_artifact: Option<String> = None;
    let mut translation_artifact: Option<String> = None;
    let mut promotable = false;
    let mut promoted = false;

    // Apply candidate rewrite set + translation-validation obligation in-place, then either
    // promote (keep) or roll back atomically by restoring original bytes.
    let apply_result = (|| -> Result<(), String> {
        translation_obligation_added = ensure_translation_obligation(pkg_toml)?;

        for rw in &rewrites {
            std::fs::write(&rw.abs, &rw.new_src)
                .map_err(|e| format!("write optimized module {}: {e}", rw.abs.display()))?;
        }

        package_artifact = Some(
            pack_with_frontend(pkg_toml, frontend.clone())
                .map_err(|e| format!("pack optimized package: {e}"))?,
        );
        let test = test_package_with_step_limit_and_frontend(
            pkg_toml,
            caps_override,
            step_limit,
            mem_limits,
            frontend.clone(),
        )
        .map_err(|e| format!("obligation run after optimize: {e}"))?;
        acceptance_artifact = Some(test.acceptance_artifact.clone());
        let translation = test
            .obligation_results
            .iter()
            .find(|o| o.name == "core/obligation::translation-validation")
            .cloned();
        match translation {
            Some(ob) => {
                if let Some(a) = ob.artifact.clone() {
                    translation_artifact = Some(a);
                }
                if !ob.ok {
                    errors.extend(ob.errors.clone());
                }
                promotable = test.ok && ob.ok;
            }
            None => {
                promotable = false;
                errors.push(
                    "missing core/obligation::translation-validation result after optimize"
                        .to_string(),
                );
            }
        }
        Ok(())
    })();

    if let Err(e) = apply_result {
        errors.push(e);
    }

    if promotable && !dry_run {
        promoted = true;
    } else {
        if let Err(e) = std::fs::write(pkg_toml, &original_pkg_toml) {
            errors.push(format!(
                "restore package manifest {}: {e}",
                pkg_toml.display()
            ));
        }
        for rw in &rewrites {
            if let Err(e) = std::fs::write(&rw.abs, &rw.old_src) {
                errors.push(format!("restore module {}: {e}", rw.abs.display()));
            }
        }
    }

    let proposed_term = Term::Vector(rewrites.iter().map(candidate_term).collect());
    let report_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/self-optimize-report-v0.1".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":pkg")),
                Term::Str(pkg_toml.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":dry-run")), Term::Bool(dry_run)),
            (
                TermOrdKey(Term::symbol(":proposed-count")),
                Term::Int(proposed_count.into()),
            ),
            (
                TermOrdKey(Term::symbol(":translation-obligation-added")),
                Term::Bool(translation_obligation_added),
            ),
            (
                TermOrdKey(Term::symbol(":promotable")),
                Term::Bool(promotable),
            ),
            (TermOrdKey(Term::symbol(":promoted")), Term::Bool(promoted)),
            (
                TermOrdKey(Term::symbol(":package-artifact")),
                package_artifact.clone().map(Term::Str).unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":acceptance-artifact")),
                acceptance_artifact
                    .clone()
                    .map(Term::Str)
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":translation-artifact")),
                translation_artifact
                    .clone()
                    .map(Term::Str)
                    .unwrap_or(Term::Nil),
            ),
            (TermOrdKey(Term::symbol(":proposed")), proposed_term.clone()),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let report_artifact = store.put_term(&report_term).map_err(|e| e.to_string())?;

    let ok = if dry_run { promotable } else { promoted };
    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (
                TermOrdKey(Term::symbol(":pkg")),
                Term::Str(pkg_toml.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":dry-run")), Term::Bool(dry_run)),
            (
                TermOrdKey(Term::symbol(":proposed-count")),
                Term::Int(proposed_count.into()),
            ),
            (
                TermOrdKey(Term::symbol(":promotable")),
                Term::Bool(promotable),
            ),
            (TermOrdKey(Term::symbol(":promoted")), Term::Bool(promoted)),
            (
                TermOrdKey(Term::symbol(":translation-obligation-added")),
                Term::Bool(translation_obligation_added),
            ),
            (
                TermOrdKey(Term::symbol(":package-artifact")),
                package_artifact.map(Term::Str).unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":acceptance-artifact")),
                acceptance_artifact.map(Term::Str).unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":translation-artifact")),
                translation_artifact.map(Term::Str).unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":report-artifact")),
                Term::Str(report_artifact),
            ),
            (TermOrdKey(Term::symbol(":proposed")), proposed_term),
            (
                TermOrdKey(Term::symbol(":errors")),
                Term::Vector(errors.iter().cloned().map(Term::Str).collect()),
            ),
        ]
        .into_iter()
        .collect(),
    );

    Ok(LocalPkgResult {
        kind: "genesis/pkg-self-optimize-v0.1",
        log_op: "pkg-self-optimize",
        program_hash: gc_coreform::hash_term(&value),
        value,
    })
}

fn candidate_term(rw: &CandidateRewrite) -> Term {
    let rewrites = Term::Map(
        rw.stats
            .rewrites_applied
            .iter()
            .map(|(k, v)| {
                (
                    TermOrdKey(Term::Str(k.clone())),
                    Term::Int((*v as i64).into()),
                )
            })
            .collect::<BTreeMap<_, _>>(),
    );
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":path")),
                Term::Str(rw.path.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":before-h")),
                Term::Str(hex32(rw.old_hash)),
            ),
            (
                TermOrdKey(Term::symbol(":after-h")),
                Term::Str(hex32(rw.new_hash)),
            ),
            (
                TermOrdKey(Term::symbol(":egg-runs")),
                Term::Int((rw.stats.egg_runs as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":egg-iterations")),
                Term::Int((rw.stats.iterations as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":egg-eclasses")),
                Term::Int((rw.stats.eclasses as i64).into()),
            ),
            (
                TermOrdKey(Term::symbol(":egg-enodes")),
                Term::Int((rw.stats.enodes as i64).into()),
            ),
            (TermOrdKey(Term::symbol(":rewrites-applied")), rewrites),
        ]
        .into_iter()
        .collect(),
    )
}

fn ensure_translation_obligation(pkg_toml: &Path) -> Result<bool, String> {
    let src = std::fs::read_to_string(pkg_toml)
        .map_err(|e| format!("read manifest {}: {e}", pkg_toml.display()))?;
    let mut v: toml::Value =
        toml::from_str(&src).map_err(|e| format!("parse manifest {}: {e}", pkg_toml.display()))?;
    let tbl = v
        .as_table_mut()
        .ok_or_else(|| format!("manifest {} root must be table", pkg_toml.display()))?;
    let obligations = tbl
        .entry("obligations".to_string())
        .or_insert_with(|| toml::Value::Array(Vec::new()));
    let arr = obligations
        .as_array_mut()
        .ok_or_else(|| "manifest obligations must be array".to_string())?;
    let needle = "core/obligation::translation-validation";
    let present = arr.iter().any(|x| x.as_str() == Some(needle));
    if !present {
        arr.push(toml::Value::String(needle.to_string()));
        let out =
            toml::to_string_pretty(&v).map_err(|e| format!("serialize manifest update: {e}"))?;
        std::fs::write(pkg_toml, out)
            .map_err(|e| format!("write manifest {}: {e}", pkg_toml.display()))?;
    }
    Ok(!present)
}

fn hex32(h: [u8; 32]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in h {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}
