use super::*;
use crate::pkg_lock_read_authority::{PkgLockReadAuthority, PkgSnapshotDecision};
use crate::runner_io_ops::payload_pkg_path;

#[derive(Debug, Clone)]
struct CanonicalModule {
    forms: Vec<Term>,
    module_hash: [u8; 32],
}

#[derive(Debug, Clone)]
enum ModuleSemanticError {
    Parse { message: String },
    Canon { message: String },
    BadPinnedHash { path: String },
    HashMismatch { path: String },
}

fn parse_canonical_module_source(
    src: &str,
    module_path: &str,
    pinned_hash_hex: Option<&str>,
) -> Result<CanonicalModule, ModuleSemanticError> {
    let forms = gc_coreform::parse_module(src).map_err(|e| ModuleSemanticError::Parse {
        message: e.to_string(),
    })?;
    let forms =
        gc_coreform::canonicalize_module(forms).map_err(|e| ModuleSemanticError::Canon {
            message: e.to_string(),
        })?;
    let module_hash = gc_coreform::hash_module(&forms);
    if let Some(want_hex) = pinned_hash_hex {
        if want_hex.len() != 64 || !want_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ModuleSemanticError::BadPinnedHash {
                path: module_path.to_string(),
            });
        }
        let got_hex = blake3::Hash::from_bytes(module_hash).to_hex().to_string();
        if got_hex != want_hex {
            return Err(ModuleSemanticError::HashMismatch {
                path: module_path.to_string(),
            });
        }
    }
    Ok(CanonicalModule { forms, module_hash })
}

fn module_semantics_error(e: ModuleSemanticError, error_tok: SealId, op: &str) -> Value {
    match e {
        ModuleSemanticError::Parse { message } => {
            mk_error(error_tok, "core/pkg/parse-error", message, Some(op))
        }
        ModuleSemanticError::Canon { message } => {
            mk_error(error_tok, "core/pkg/canon-error", message, Some(op))
        }
        ModuleSemanticError::BadPinnedHash { path } => mk_error(
            error_tok,
            "core/pkg/bad-hash",
            format!("manifest module hash is not 64-hex: {path}"),
            Some(op),
        ),
        ModuleSemanticError::HashMismatch { path } => mk_error(
            error_tok,
            "core/pkg/hash-mismatch",
            format!("module hash mismatch: {path}"),
            Some(op),
        ),
    }
}

pub(super) fn handle_load_package(
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
    op: &str,
) -> Result<Value, EffectsError> {
    let pkg_path_s = match payload_pkg_path(payload) {
        Ok(s) => s,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/pkg/bad-payload",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    let base_dir = effective_base_dir(pol)?;
    let pkg_path = sandbox_path_read(&base_dir, &pkg_path_s)?;

    let (manifest, pkg_dir) = match gc_pkg::PackageManifest::load(&pkg_path) {
        Ok(x) => x,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/pkg/manifest-error",
                format!("{e}"),
                Some(op),
            ));
        }
    };

    let base = std::fs::canonicalize(&base_dir)?;
    let pkg_dir_resolved = std::fs::canonicalize(&pkg_dir)?;
    if !pkg_dir_resolved.starts_with(&base) {
        return Ok(mk_error(
            error_tok,
            "core/caps/path-escape",
            "package directory escapes base dir".to_string(),
            Some(op),
        ));
    }

    let mut modules_out: Vec<Term> = Vec::new();
    for me in &manifest.modules {
        let module_fs_path = pkg_dir.join(&me.path);
        let resolved = match std::fs::canonicalize(&module_fs_path) {
            Ok(p) => p,
            Err(e) => {
                return Ok(mk_error_with_ctx(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                    Term::Map(
                        [(
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str(me.path.clone()),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                ));
            }
        };
        if !resolved.starts_with(&base) {
            return Ok(mk_error(
                error_tok,
                "core/caps/path-escape",
                format!("module escapes base dir: {}", me.path),
                Some(op),
            ));
        }
        let src = match std::fs::read_to_string(&resolved) {
            Ok(s) => s,
            Err(e) => {
                return Ok(mk_error_with_ctx(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                    Term::Map(
                        [(
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str(me.path.clone()),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                ));
            }
        };
        let parsed = match parse_canonical_module_source(&src, &me.path, me.hash.as_deref()) {
            Ok(v) => v,
            Err(e) => return Ok(module_semantics_error(e, error_tok, op)),
        };
        let forms = parsed.forms;
        let module_h = parsed.module_hash;

        let mut mm = BTreeMap::new();
        mm.insert(
            TermOrdKey(Term::symbol(":path")),
            Term::Str(me.path.clone()),
        );
        mm.insert(TermOrdKey(Term::symbol(":module")), Term::Vector(forms));
        mm.insert(
            TermOrdKey(Term::symbol(":module-h")),
            Term::Bytes(module_h.to_vec().into()),
        );
        modules_out.push(Term::Map(mm));
    }

    let mut out = BTreeMap::new();
    out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
    out.insert(TermOrdKey(Term::symbol(":pkg")), Term::Str(pkg_path_s));
    out.insert(
        TermOrdKey(Term::symbol(":name")),
        Term::Str(manifest.name.clone()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":version")),
        Term::Str(manifest.version.clone()),
    );
    out.insert(
        TermOrdKey(Term::symbol(":obligations")),
        Term::Vector(
            manifest
                .obligations
                .iter()
                .cloned()
                .map(Term::Symbol)
                .collect(),
        ),
    );
    out.insert(
        TermOrdKey(Term::symbol(":modules")),
        Term::Vector(modules_out),
    );
    Ok(Value::data(Term::Map(out)))
}

#[expect(
    clippy::too_many_arguments,
    reason = "snapshot dispatch keeps payload, policy, store, authority, budget, and sealed-error boundaries explicit"
)]
pub(super) fn handle_snapshot(
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: &ArtifactStore,
    snapshot_authority: Option<&mut PkgLockReadAuthority>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
) -> Result<Value, EffectsError> {
    let snapshot_authority = snapshot_authority.ok_or_else(|| {
        EffectsError::Log("missing selfhost package snapshot authority".to_string())
    })?;
    let pkg_path_s = match payload_pkg_path(payload) {
        Ok(s) => s,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/pkg/bad-payload",
                format!("{e}"),
                Some(op),
            ));
        }
    };
    let base_dir = effective_base_dir(pol)?;
    let pkg_path = sandbox_path_read(&base_dir, &pkg_path_s)?;

    let (manifest, pkg_dir) = match gc_pkg::PackageManifest::load(&pkg_path) {
        Ok(x) => x,
        Err(e) => {
            return Ok(mk_error(
                error_tok,
                "core/pkg/manifest-error",
                format!("{e}"),
                Some(op),
            ));
        }
    };

    let base = std::fs::canonicalize(&base_dir)?;
    let pkg_dir_resolved = std::fs::canonicalize(&pkg_dir)?;
    if !pkg_dir_resolved.starts_with(&base) {
        return Ok(mk_error(
            error_tok,
            "core/caps/path-escape",
            "package directory escapes base dir".to_string(),
            Some(op),
        ));
    }

    let mut module_facts: Vec<Term> = Vec::new();
    for me in &manifest.modules {
        let module_fs_path = pkg_dir.join(&me.path);
        let resolved = match std::fs::canonicalize(&module_fs_path) {
            Ok(p) => p,
            Err(e) => {
                return Ok(mk_error_with_ctx(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                    Term::Map(
                        [(
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str(me.path.clone()),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                ));
            }
        };
        if !resolved.starts_with(&base) {
            return Ok(mk_error(
                error_tok,
                "core/caps/path-escape",
                format!("module escapes base dir: {}", me.path),
                Some(op),
            ));
        }
        let src = match std::fs::read_to_string(&resolved) {
            Ok(s) => s,
            Err(e) => {
                return Ok(mk_error_with_ctx(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                    Term::Map(
                        [(
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str(me.path.clone()),
                        )]
                        .into_iter()
                        .collect(),
                    ),
                ));
            }
        };
        let parsed = match parse_canonical_module_source(&src, &me.path, me.hash.as_deref()) {
            Ok(v) => v,
            Err(e) => return Ok(module_semantics_error(e, error_tok, op)),
        };
        let forms = parsed.forms;
        let module_h = parsed.module_hash;

        let mut mm = BTreeMap::new();
        mm.insert(
            TermOrdKey(Term::Symbol(":path".to_string())),
            Term::Str(me.path.clone()),
        );
        mm.insert(
            TermOrdKey(Term::Symbol(":module".to_string())),
            Term::Vector(forms),
        );
        mm.insert(
            TermOrdKey(Term::Symbol(":module-h".to_string())),
            Term::Bytes(module_h.to_vec().into()),
        );
        module_facts.push(Term::Map(mm));
    }

    let facts = Term::Map(
        [
            (
                TermOrdKey(Term::Symbol(":modules".to_string())),
                Term::Vector(module_facts),
            ),
            (
                TermOrdKey(Term::Symbol(":name".to_string())),
                Term::Str(manifest.name.clone()),
            ),
            (
                TermOrdKey(Term::Symbol(":obligations".to_string())),
                Term::Vector(
                    manifest
                        .obligations
                        .iter()
                        .cloned()
                        .map(Term::Symbol)
                        .collect(),
                ),
            ),
            (
                TermOrdKey(Term::Symbol(":pkg".to_string())),
                Term::Str(pkg_path_s),
            ),
            (
                TermOrdKey(Term::Symbol(":version".to_string())),
                Term::Str(manifest.version.clone()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let plan = match snapshot_authority.construct_snapshot(facts)? {
        PkgSnapshotDecision::Accept(plan) => plan,
        PkgSnapshotDecision::Error { code, message } => {
            return Ok(mk_error(error_tok, &code, message, Some(op)));
        }
    };
    for artifact in &plan.artifacts {
        let stored =
            match store_put_with_budget(store, &artifact.bytes, policy, budget, error_tok, op) {
                Ok(hash) => hash,
                Err(value) => return Ok(value),
            };
        if stored != artifact.hash {
            return Err(EffectsError::Log(
                "selfhost package snapshot authority/store identity contradiction".to_string(),
            ));
        }
    }

    let mut out = BTreeMap::new();
    out.insert(
        TermOrdKey(Term::Symbol(":snapshot".to_string())),
        Term::Str(plan.snapshot),
    );
    out.insert(
        TermOrdKey(Term::Symbol(":modules".to_string())),
        Term::Vector(plan.modules),
    );
    Ok(Value::data(Term::Map(out)))
}
