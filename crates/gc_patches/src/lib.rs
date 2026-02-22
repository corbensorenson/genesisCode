use std::collections::BTreeMap;
use std::path::Path;

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, hash_term, parse_module, parse_term,
    print_module, print_term,
};
use gc_kernel::{Apply, EvalCtx, MemLimits, SealId, StepLimit, Value};
use gc_obligations::{
    CoreformFrontend, EvidenceStore, ObligationError, PackageTestResult, coreform_frontend_is_rust,
    default_coreform_frontend, pack, parse_canonicalize_module_source_with_frontend,
    test_package_with_step_limit_and_frontend,
};
use gc_pkg::PackageManifest;
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_with_mode};
use num_traits::ToPrimitive;
use thiserror::Error;

#[path = "patch_parse.rs"]
mod patch_parse;
#[path = "patch_refactor.rs"]
mod patch_refactor;
#[path = "patch_selfhost_toolchain.rs"]
mod patch_selfhost_toolchain;
#[path = "patch_semantic.rs"]
mod patch_semantic;

use patch_selfhost_toolchain::SelfhostPatchToolchain;
use patch_semantic::{hash32_hex, path_steps_to_term, resolve_node_id_path, semantic_node_id};

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("patch parse error: {0}")]
    Parse(String),

    #[error("patch validation error: {0}")]
    Validate(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("obligations error: {0}")]
    Obligations(#[from] ObligationError),
}

#[derive(Debug, Clone)]
pub struct PatchApplyResult {
    pub ok: bool,
    pub patch_artifact: String,
    pub report_artifact: String,
    pub acceptance_artifact: Option<String>,
    pub package_artifact: Option<String>,
}

#[derive(Debug, Clone)]
struct Patch {
    version: u64,
    intent: String,
    provenance: Term,
    ops: Vec<PatchOp>,
}

#[derive(Debug, Clone)]
enum PatchOp {
    ReplaceNode {
        module_path: String,
        path: Vec<PathStep>,
        new_term: Term,
    },
    ReplaceNodeId {
        module_path: String,
        node_id: String,
        new_term: Term,
    },
    AddModule {
        module_path: String,
        content: ModuleContent,
    },
    RemoveModule {
        module_path: String,
    },
    UpdateManifest {
        set: Option<Term>,
        obligations_add: Vec<String>,
        obligations_remove: Vec<String>,
        tests_add: Vec<String>,
        tests_remove: Vec<String>,
        caps_policy: Option<String>,
    },
    RenameSymbol {
        module_path: String,
        from: String,
        to: String,
    },
    MoveModule {
        from_module_path: String,
        to_module_path: String,
    },
    SplitModule {
        from_module_path: String,
        to_module_path: String,
        symbols: Vec<String>,
    },
    RewriteMetaList {
        module_path: String,
        field: MetaListField,
        add: Vec<String>,
        remove: Vec<String>,
        replace: Option<Vec<String>>,
    },
    MigrateContractSignature {
        module_path: String,
        contract_symbol: String,
        from_param: String,
        to_param: String,
    },
}

#[derive(Debug, Clone)]
enum ModuleContent {
    Source(String),
    Forms(Vec<Term>),
}

#[derive(Debug, Clone)]
enum PathStep {
    Form(usize),
    PairCar,
    PairCdr,
    Vec(usize),
    Map(Term),
}

#[derive(Debug, Clone, Copy)]
enum MetaListField {
    Imports,
    Exports,
}

impl MetaListField {
    fn key_symbol(self) -> &'static str {
        match self {
            Self::Imports => ":imports",
            Self::Exports => ":exports",
        }
    }

    fn op_symbol(self) -> &'static str {
        match self {
            Self::Imports => ":rewrite-imports",
            Self::Exports => ":rewrite-exports",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticNodeRecord {
    pub module_path: String,
    pub node_id: String,
    pub path: Term,
    pub path_repr: String,
    pub term_tag: String,
    pub term_hash: String,
}

#[derive(Debug, Clone)]
struct AppliedSemanticEdit {
    op: &'static str,
    module_path: String,
    node_id: Option<String>,
    path: Option<Vec<PathStep>>,
    new_term_hash: Option<String>,
    before_module_hash: Option<String>,
    after_module_hash: Option<String>,
    detail: Option<Term>,
}

pub fn semantic_node_index_for_module_with_frontend(
    module_path: &str,
    src: &str,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<Vec<SemanticNodeRecord>, PatchError> {
    patch_semantic::semantic_node_index_for_module_with_frontend(
        module_path,
        src,
        frontend,
        step_limit,
        mem_limits,
    )
}

fn parse_path(t: &Term) -> Result<Vec<PathStep>, PatchError> {
    patch_parse::parse_path(t)
}

pub fn apply_patch(
    patch_path: &Path,
    pkg_toml: &Path,
    caps_override: Option<&Path>,
) -> Result<PatchApplyResult, PatchError> {
    apply_patch_with_step_limit(
        patch_path,
        pkg_toml,
        caps_override,
        StepLimit::Default,
        MemLimits::default(),
    )
}

pub fn apply_patch_with_step_limit(
    patch_path: &Path,
    pkg_toml: &Path,
    caps_override: Option<&Path>,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<PatchApplyResult, PatchError> {
    apply_patch_with_step_limit_and_frontend(
        patch_path,
        pkg_toml,
        caps_override,
        step_limit,
        mem_limits,
        default_coreform_frontend(),
    )
}

pub fn apply_patch_with_step_limit_and_frontend(
    patch_path: &Path,
    pkg_toml: &Path,
    caps_override: Option<&Path>,
    step_limit: StepLimit,
    mem_limits: MemLimits,
    frontend: CoreformFrontend,
) -> Result<PatchApplyResult, PatchError> {
    let patch_src = std::fs::read_to_string(patch_path)?;
    let patch_term = parse_term(&patch_src).map_err(|e| PatchError::Parse(e.to_string()))?;

    let mut selfhost = if coreform_frontend_is_rust(&frontend) {
        None
    } else {
        let CoreformFrontend::Selfhost(cfg) = &frontend else {
            return Err(PatchError::Validate(
                "invalid frontend dispatch while initializing patch toolchain".to_string(),
            ));
        };
        Some(SelfhostPatchToolchain::init(cfg, mem_limits)?)
    };

    // When running under the selfhost CoreForm frontend, validate patch schema via the
    // self-hosted contract to ensure schema acceptance is controlled by `.gc` semantics.
    if let Some(sh) = selfhost.as_mut() {
        if let Err(err) = sh.validate_patch_term(&patch_term, step_limit) {
            match &err {
                PatchError::Validate(msg) if msg.contains("unknown :op") => {
                    // Allow forward-compatible patch ops when selfhost artifact lags schema source.
                    // Rust-side parser/validator remains authoritative for these ops.
                }
                _ => return Err(err),
            }
        }
    }

    let patch = Patch::from_term(&patch_term)?;
    if patch.version != 1 {
        return Err(PatchError::Validate(format!(
            "unsupported patch :version {}",
            patch.version
        )));
    }

    let (_manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| PatchError::Validate(format!("{e}")))?;
    let store = EvidenceStore::open(&pkg_dir)?;

    // Store the patch artifact itself (as canonical CoreForm bytes).
    let patch_artifact = store.put_term(&patch_term)?;

    // Apply ops.
    let mut semantic_edits = Vec::new();
    for op in &patch.ops {
        if let Some(edit) = apply_one_op(
            &pkg_dir,
            pkg_toml,
            op,
            &frontend,
            step_limit,
            mem_limits,
            selfhost.as_mut(),
        )? {
            semantic_edits.push(edit);
        }
    }

    // Re-pack to compute updated package artifact record and module hashes.
    let package_artifact = Some(pack(pkg_toml)?);

    // Re-run obligations using updated manifest.
    let acceptance = Some(test_package_with_step_limit_and_frontend(
        pkg_toml,
        caps_override,
        step_limit,
        mem_limits,
        frontend,
    )?);

    let ok = acceptance.as_ref().is_some_and(|r| r.ok);

    let report = report_term(
        &patch,
        ok,
        &package_artifact,
        acceptance.as_ref(),
        &semantic_edits,
    );
    let report_artifact = store.put_term(&report)?;

    Ok(PatchApplyResult {
        ok,
        patch_artifact,
        report_artifact,
        acceptance_artifact: acceptance.as_ref().map(|r| r.acceptance_artifact.clone()),
        package_artifact,
    })
}

/// Validate a patch artifact term without performing any I/O.
pub fn validate_patch_term(t: &Term) -> Result<(), PatchError> {
    let _ = Patch::from_term(t)?;
    Ok(())
}

fn report_term(
    patch: &Patch,
    ok: bool,
    package_artifact: &Option<String>,
    acceptance: Option<&PackageTestResult>,
    semantic_edits: &[AppliedSemanticEdit],
) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("genesis/patch-apply-v0.2".to_string()),
    );
    m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
    m.insert(
        TermOrdKey(Term::symbol(":intent")),
        Term::Str(patch.intent.clone()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":provenance")),
        patch.provenance.clone(),
    );
    m.insert(
        TermOrdKey(Term::symbol(":ops-count")),
        Term::Int((patch.ops.len() as i64).into()),
    );
    if let Some(p) = package_artifact {
        m.insert(
            TermOrdKey(Term::symbol(":package-artifact")),
            Term::Str(p.clone()),
        );
    }
    if let Some(a) = acceptance {
        m.insert(
            TermOrdKey(Term::symbol(":acceptance-artifact")),
            Term::Str(a.acceptance_artifact.clone()),
        );
    }
    if !semantic_edits.is_empty() {
        let mut edits = Vec::with_capacity(semantic_edits.len());
        for edit in semantic_edits {
            let mut em = BTreeMap::new();
            em.insert(
                TermOrdKey(Term::symbol(":op")),
                Term::Symbol(edit.op.to_string()),
            );
            em.insert(
                TermOrdKey(Term::symbol(":module-path")),
                Term::Str(edit.module_path.clone()),
            );
            if let Some(node_id) = &edit.node_id {
                em.insert(
                    TermOrdKey(Term::symbol(":node-id")),
                    Term::Str(node_id.clone()),
                );
            }
            if let Some(path) = &edit.path {
                em.insert(
                    TermOrdKey(Term::symbol(":path")),
                    path_steps_to_term(path).unwrap_or(Term::Vector(Vec::new())),
                );
            }
            if let Some(new_term_hash) = &edit.new_term_hash {
                em.insert(
                    TermOrdKey(Term::symbol(":new-term-h")),
                    Term::Str(new_term_hash.clone()),
                );
            }
            if let Some(before_h) = &edit.before_module_hash {
                em.insert(
                    TermOrdKey(Term::symbol(":before-module-h")),
                    Term::Str(before_h.clone()),
                );
            }
            if let Some(after_h) = &edit.after_module_hash {
                em.insert(
                    TermOrdKey(Term::symbol(":after-module-h")),
                    Term::Str(after_h.clone()),
                );
            }
            if let Some(detail) = &edit.detail {
                em.insert(TermOrdKey(Term::symbol(":detail")), detail.clone());
            }
            edits.push(Term::Map(em));
        }
        m.insert(
            TermOrdKey(Term::symbol(":semantic-edits")),
            Term::Vector(edits),
        );
    }
    Term::Map(m)
}

fn apply_one_op(
    pkg_dir: &Path,
    pkg_toml: &Path,
    op: &PatchOp,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
    mut selfhost: Option<&mut SelfhostPatchToolchain>,
) -> Result<Option<AppliedSemanticEdit>, PatchError> {
    match op {
        PatchOp::ReplaceNode {
            module_path,
            path,
            new_term,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;

            if let Some(sh) = selfhost.as_deref_mut() {
                let path_term = path_steps_to_term(path)?;
                let next_forms =
                    sh.apply_replace_node_term(&forms, &path_term, new_term, step_limit)?;
                let out = sh.print_module_forms_term(&next_forms, step_limit)?;
                std::fs::write(&abs, out)?;
            } else {
                let mut forms = forms;
                apply_replace(&mut forms, path, new_term.clone())?;
                let forms =
                    canonicalize_module(forms).map_err(|e| PatchError::Validate(e.to_string()))?;
                let out = print_module(&forms);
                std::fs::write(&abs, out)?;
            }
            Ok(Some(AppliedSemanticEdit {
                op: ":replace-node",
                module_path: module_path.clone(),
                node_id: Some(semantic_node_id(module_path, path)?),
                path: Some(path.clone()),
                new_term_hash: Some(hash32_hex(hash_term(new_term))),
                before_module_hash: None,
                after_module_hash: None,
                detail: None,
            }))
        }
        PatchOp::ReplaceNodeId {
            module_path,
            node_id,
            new_term,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let path = resolve_node_id_path(module_path, &forms, node_id)?;

            if let Some(sh) = selfhost.as_deref_mut() {
                let path_term = path_steps_to_term(&path)?;
                let next_forms =
                    sh.apply_replace_node_term(&forms, &path_term, new_term, step_limit)?;
                let out = sh.print_module_forms_term(&next_forms, step_limit)?;
                std::fs::write(&abs, out)?;
            } else {
                let mut forms = forms;
                apply_replace(&mut forms, &path, new_term.clone())?;
                let forms =
                    canonicalize_module(forms).map_err(|e| PatchError::Validate(e.to_string()))?;
                let out = print_module(&forms);
                std::fs::write(&abs, out)?;
            }
            Ok(Some(AppliedSemanticEdit {
                op: ":replace-node-id",
                module_path: module_path.clone(),
                node_id: Some(node_id.clone()),
                path: Some(path),
                new_term_hash: Some(hash32_hex(hash_term(new_term))),
                before_module_hash: None,
                after_module_hash: None,
                detail: None,
            }))
        }
        PatchOp::AddModule {
            module_path,
            content,
        } => {
            let abs = pkg_dir.join(module_path);
            if abs.exists() {
                return Err(PatchError::Validate(format!(
                    "add-module target already exists: {}",
                    abs.display()
                )));
            }
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let out = if let Some(sh) = selfhost.as_deref_mut() {
                let content_term = match content {
                    ModuleContent::Source(s) => Term::Str(s.clone()),
                    ModuleContent::Forms(fs) => Term::Vector(fs.clone()),
                };
                sh.print_module_from_content_term(&content_term, step_limit)?
            } else {
                let forms = match content {
                    ModuleContent::Source(s) => {
                        parse_canonicalize_module_src(s, frontend, step_limit, mem_limits)?
                    }
                    ModuleContent::Forms(fs) => canonicalize_module(fs.clone())
                        .map_err(|e| PatchError::Validate(e.to_string()))?,
                };
                print_module(&forms)
            };
            std::fs::write(&abs, out)?;

            // Update manifest modules list by appending; pack will pin hashes.
            let mut s = std::fs::read_to_string(pkg_toml)?;
            let v0: toml::Value =
                toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
            let v = if let Some(sh) = selfhost.as_deref_mut() {
                let manifest_term = toml_to_coreform(&v0)?;
                let out_term =
                    sh.manifest_apply_add_module_term(&manifest_term, module_path, step_limit)?;
                coreform_to_toml(&out_term)?
            } else {
                let mut v = v0;
                let mods = v
                    .get_mut("modules")
                    .and_then(|x| x.as_array_mut())
                    .ok_or_else(|| {
                        PatchError::Validate("manifest missing modules array".to_string())
                    })?;
                mods.push(toml::Value::Table(
                    [
                        ("path".to_string(), toml::Value::String(module_path.clone())),
                        ("hash".to_string(), toml::Value::String("".to_string())),
                    ]
                    .into_iter()
                    .collect(),
                ));
                v
            };
            s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
            std::fs::write(pkg_toml, s)?;
            Ok(None)
        }
        PatchOp::RemoveModule { module_path } => {
            let abs = pkg_dir.join(module_path);
            if abs.exists() {
                std::fs::remove_file(&abs)?;
            }
            // Remove from manifest modules array.
            let mut s = std::fs::read_to_string(pkg_toml)?;
            let v0: toml::Value =
                toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
            let v = if let Some(sh) = selfhost.as_deref_mut() {
                let manifest_term = toml_to_coreform(&v0)?;
                let out_term =
                    sh.manifest_apply_remove_module_term(&manifest_term, module_path, step_limit)?;
                coreform_to_toml(&out_term)?
            } else {
                let mut v = v0;
                let mods = v
                    .get_mut("modules")
                    .and_then(|x| x.as_array_mut())
                    .ok_or_else(|| {
                        PatchError::Validate("manifest missing modules array".to_string())
                    })?;
                mods.retain(|m| {
                    m.get("path").and_then(|p| p.as_str()) != Some(module_path.as_str())
                });
                v
            };
            s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
            std::fs::write(pkg_toml, s)?;
            Ok(None)
        }
        PatchOp::UpdateManifest {
            set,
            obligations_add,
            obligations_remove,
            tests_add,
            tests_remove,
            caps_policy,
        } => {
            let mut s = std::fs::read_to_string(pkg_toml)?;
            let v0: toml::Value =
                toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
            let v = if let Some(sh) = selfhost {
                let manifest_term = toml_to_coreform(&v0)?;
                let op_term = update_manifest_op_to_term(
                    set.as_ref(),
                    obligations_add,
                    obligations_remove,
                    tests_add,
                    tests_remove,
                    caps_policy.as_deref(),
                )?;
                let out_term = sh.manifest_apply_update_manifest_op_term(
                    &manifest_term,
                    &op_term,
                    step_limit,
                )?;
                coreform_to_toml(&out_term)?
            } else {
                let mut v = v0;
                if let Some(set) = set {
                    apply_manifest_set(&mut v, set)?;
                }
                if !obligations_add.is_empty() || !obligations_remove.is_empty() {
                    patch_string_vec_field(
                        &mut v,
                        "obligations",
                        obligations_add,
                        obligations_remove,
                    )?;
                }
                if !tests_add.is_empty() || !tests_remove.is_empty() {
                    patch_string_vec_field(&mut v, "tests", tests_add, tests_remove)?;
                }
                if let Some(p) = caps_policy {
                    v.as_table_mut()
                        .ok_or_else(|| {
                            PatchError::Validate("manifest must be a table".to_string())
                        })?
                        .insert("caps_policy".to_string(), toml::Value::String(p.clone()));
                }
                v
            };
            s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
            std::fs::write(pkg_toml, s)?;
            Ok(None)
        }
        PatchOp::RenameSymbol {
            module_path,
            from,
            to,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let before_module_hash = hash32_hex(hash_module(&forms));
            let (next, rewrites) = patch_refactor::rename_symbol_in_forms(forms, from, to)?;
            if rewrites == 0 {
                return Err(PatchError::Validate(format!(
                    "rename-symbol found no `{from}` references in {module_path}"
                )));
            }
            let next =
                canonicalize_module(next).map_err(|e| PatchError::Validate(e.to_string()))?;
            let after_module_hash = hash32_hex(hash_module(&next));
            if let Some(sh) = selfhost.as_deref_mut() {
                let out = sh.print_module_forms_term(&next, step_limit)?;
                std::fs::write(&abs, out)?;
            } else {
                std::fs::write(&abs, print_module(&next))?;
            }
            let mut detail = BTreeMap::new();
            detail.insert(
                TermOrdKey(Term::symbol(":from")),
                Term::symbol(from.clone()),
            );
            detail.insert(TermOrdKey(Term::symbol(":to")), Term::symbol(to.clone()));
            detail.insert(
                TermOrdKey(Term::symbol(":rewrite-count")),
                Term::Int((rewrites as i64).into()),
            );
            Ok(Some(AppliedSemanticEdit {
                op: ":rename-symbol",
                module_path: module_path.clone(),
                node_id: None,
                path: None,
                new_term_hash: None,
                before_module_hash: Some(before_module_hash),
                after_module_hash: Some(after_module_hash),
                detail: Some(Term::Map(detail)),
            }))
        }
        PatchOp::MoveModule {
            from_module_path,
            to_module_path,
        } => {
            let from_abs = pkg_dir.join(from_module_path);
            let to_abs = pkg_dir.join(to_module_path);
            if !from_abs.exists() {
                return Err(PatchError::Validate(format!(
                    "move-module source does not exist: {}",
                    from_abs.display()
                )));
            }
            if to_abs.exists() {
                return Err(PatchError::Validate(format!(
                    "move-module target already exists: {}",
                    to_abs.display()
                )));
            }
            let src = std::fs::read_to_string(&from_abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let module_hash = hash32_hex(hash_module(&forms));
            if let Some(parent) = to_abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&from_abs, &to_abs)?;
            patch_manifest_move_module_path(pkg_toml, from_module_path, to_module_path)?;
            let mut detail = BTreeMap::new();
            detail.insert(
                TermOrdKey(Term::symbol(":from-module-path")),
                Term::Str(from_module_path.clone()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":to-module-path")),
                Term::Str(to_module_path.clone()),
            );
            Ok(Some(AppliedSemanticEdit {
                op: ":move-module",
                module_path: to_module_path.clone(),
                node_id: None,
                path: None,
                new_term_hash: None,
                before_module_hash: Some(module_hash.clone()),
                after_module_hash: Some(module_hash),
                detail: Some(Term::Map(detail)),
            }))
        }
        PatchOp::SplitModule {
            from_module_path,
            to_module_path,
            symbols,
        } => {
            let from_abs = pkg_dir.join(from_module_path);
            let to_abs = pkg_dir.join(to_module_path);
            if !from_abs.exists() {
                return Err(PatchError::Validate(format!(
                    "split-module source does not exist: {}",
                    from_abs.display()
                )));
            }
            if to_abs.exists() {
                return Err(PatchError::Validate(format!(
                    "split-module target already exists: {}",
                    to_abs.display()
                )));
            }
            let src = std::fs::read_to_string(&from_abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let before_module_hash = hash32_hex(hash_module(&forms));
            let symbol_set = symbols
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            let (keep, extracted, moved) = patch_refactor::split_module_forms(forms, &symbol_set)?;
            let keep =
                canonicalize_module(keep).map_err(|e| PatchError::Validate(e.to_string()))?;
            let extracted =
                canonicalize_module(extracted).map_err(|e| PatchError::Validate(e.to_string()))?;
            let after_module_hash = hash32_hex(hash_module(&keep));
            let extracted_module_hash = hash32_hex(hash_module(&extracted));
            if let Some(parent) = to_abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if let Some(sh) = selfhost.as_deref_mut() {
                let keep_out = sh.print_module_forms_term(&keep, step_limit)?;
                std::fs::write(&from_abs, keep_out)?;
                let extracted_out = sh.print_module_forms_term(&extracted, step_limit)?;
                std::fs::write(&to_abs, extracted_out)?;
            } else {
                std::fs::write(&from_abs, print_module(&keep))?;
                std::fs::write(&to_abs, print_module(&extracted))?;
            }
            patch_manifest_add_module_path(pkg_toml, to_module_path)?;
            let mut detail = BTreeMap::new();
            detail.insert(
                TermOrdKey(Term::symbol(":to-module-path")),
                Term::Str(to_module_path.clone()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":moved-def-count")),
                Term::Int((moved as i64).into()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":extracted-module-h")),
                Term::Str(extracted_module_hash),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":symbols")),
                Term::Vector(symbols.iter().cloned().map(Term::symbol).collect()),
            );
            Ok(Some(AppliedSemanticEdit {
                op: ":split-module",
                module_path: from_module_path.clone(),
                node_id: None,
                path: None,
                new_term_hash: None,
                before_module_hash: Some(before_module_hash),
                after_module_hash: Some(after_module_hash),
                detail: Some(Term::Map(detail)),
            }))
        }
        PatchOp::RewriteMetaList {
            module_path,
            field,
            add,
            remove,
            replace,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let before_module_hash = hash32_hex(hash_module(&forms));
            let (next, changed_entries) =
                patch_refactor::rewrite_meta_list(forms, *field, add, remove, replace.as_deref())?;
            let next =
                canonicalize_module(next).map_err(|e| PatchError::Validate(e.to_string()))?;
            let after_module_hash = hash32_hex(hash_module(&next));
            if let Some(sh) = selfhost.as_deref_mut() {
                let out = sh.print_module_forms_term(&next, step_limit)?;
                std::fs::write(&abs, out)?;
            } else {
                std::fs::write(&abs, print_module(&next))?;
            }
            let mut detail = BTreeMap::new();
            detail.insert(
                TermOrdKey(Term::symbol(":field")),
                Term::symbol(field.key_symbol()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":changed-entries")),
                Term::Int((changed_entries as i64).into()),
            );
            if !add.is_empty() {
                detail.insert(
                    TermOrdKey(Term::symbol(":add")),
                    Term::Vector(add.iter().cloned().map(Term::symbol).collect()),
                );
            }
            if !remove.is_empty() {
                detail.insert(
                    TermOrdKey(Term::symbol(":remove")),
                    Term::Vector(remove.iter().cloned().map(Term::symbol).collect()),
                );
            }
            if let Some(replace) = replace {
                detail.insert(
                    TermOrdKey(Term::symbol(":replace")),
                    Term::Vector(replace.iter().cloned().map(Term::symbol).collect()),
                );
            }
            Ok(Some(AppliedSemanticEdit {
                op: field.op_symbol(),
                module_path: module_path.clone(),
                node_id: None,
                path: None,
                new_term_hash: None,
                before_module_hash: Some(before_module_hash),
                after_module_hash: Some(after_module_hash),
                detail: Some(Term::Map(detail)),
            }))
        }
        PatchOp::MigrateContractSignature {
            module_path,
            contract_symbol,
            from_param,
            to_param,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let before_module_hash = hash32_hex(hash_module(&forms));
            let (next, changed_entries) = patch_refactor::migrate_contract_signature(
                forms,
                contract_symbol,
                from_param,
                to_param,
            )?;
            let next =
                canonicalize_module(next).map_err(|e| PatchError::Validate(e.to_string()))?;
            let after_module_hash = hash32_hex(hash_module(&next));
            if let Some(sh) = selfhost.as_deref_mut() {
                let out = sh.print_module_forms_term(&next, step_limit)?;
                std::fs::write(&abs, out)?;
            } else {
                std::fs::write(&abs, print_module(&next))?;
            }
            let mut detail = BTreeMap::new();
            detail.insert(
                TermOrdKey(Term::symbol(":contract-symbol")),
                Term::symbol(contract_symbol.clone()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":from-param")),
                Term::symbol(from_param.clone()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":to-param")),
                Term::symbol(to_param.clone()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":changed-entries")),
                Term::Int((changed_entries as i64).into()),
            );
            Ok(Some(AppliedSemanticEdit {
                op: ":migrate-contract-signature",
                module_path: module_path.clone(),
                node_id: None,
                path: None,
                new_term_hash: None,
                before_module_hash: Some(before_module_hash),
                after_module_hash: Some(after_module_hash),
                detail: Some(Term::Map(detail)),
            }))
        }
    }
}

fn parse_canonicalize_module_src(
    src: &str,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<Vec<Term>, PatchError> {
    if coreform_frontend_is_rust(frontend) {
        let forms = parse_module(src).map_err(|e| PatchError::Parse(e.to_string()))?;
        canonicalize_module(forms).map_err(|e| PatchError::Validate(e.to_string()))
    } else {
        parse_canonicalize_module_source_with_frontend(src, frontend, step_limit, mem_limits)
            .map_err(|e| PatchError::Parse(e.to_string()))
    }
}

fn patch_string_vec_field(
    v: &mut toml::Value,
    field: &str,
    add: &[String],
    remove: &[String],
) -> Result<(), PatchError> {
    let tbl = v
        .as_table_mut()
        .ok_or_else(|| PatchError::Validate("manifest must be a table".to_string()))?;
    let arr = tbl
        .entry(field.to_string())
        .or_insert_with(|| toml::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PatchError::Validate(format!("manifest field {field} must be array")))?;
    let mut set: std::collections::BTreeSet<String> = arr
        .iter()
        .filter_map(|x| x.as_str().map(|s| s.to_string()))
        .collect();
    for r in remove {
        set.remove(r);
    }
    for a in add {
        set.insert(a.clone());
    }
    *arr = set.into_iter().map(toml::Value::String).collect();
    Ok(())
}

fn patch_manifest_add_module_path(pkg_toml: &Path, module_path: &str) -> Result<(), PatchError> {
    let mut s = std::fs::read_to_string(pkg_toml)?;
    let mut v: toml::Value = toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
    let mods = v
        .get_mut("modules")
        .and_then(|x| x.as_array_mut())
        .ok_or_else(|| PatchError::Validate("manifest missing modules array".to_string()))?;
    let exists = mods
        .iter()
        .any(|m| m.get("path").and_then(|p| p.as_str()) == Some(module_path));
    if exists {
        return Err(PatchError::Validate(format!(
            "manifest already contains module path `{module_path}`"
        )));
    }
    mods.push(toml::Value::Table(
        [
            (
                "path".to_string(),
                toml::Value::String(module_path.to_string()),
            ),
            ("hash".to_string(), toml::Value::String("".to_string())),
        ]
        .into_iter()
        .collect(),
    ));
    s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
    std::fs::write(pkg_toml, s)?;
    Ok(())
}

fn patch_manifest_move_module_path(
    pkg_toml: &Path,
    from_module_path: &str,
    to_module_path: &str,
) -> Result<(), PatchError> {
    let mut s = std::fs::read_to_string(pkg_toml)?;
    let mut v: toml::Value = toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
    let mods = v
        .get_mut("modules")
        .and_then(|x| x.as_array_mut())
        .ok_or_else(|| PatchError::Validate("manifest missing modules array".to_string()))?;
    let mut moved = false;
    for m in mods.iter_mut() {
        if m.get("path").and_then(|p| p.as_str()) == Some(from_module_path) {
            m.as_table_mut()
                .ok_or_else(|| {
                    PatchError::Validate("manifest module entry must be table".to_string())
                })?
                .insert(
                    "path".to_string(),
                    toml::Value::String(to_module_path.to_string()),
                );
            moved = true;
            break;
        }
    }
    if !moved {
        return Err(PatchError::Validate(format!(
            "manifest missing module path `{from_module_path}`"
        )));
    }
    s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
    std::fs::write(pkg_toml, s)?;
    Ok(())
}

fn apply_manifest_set(v: &mut toml::Value, set: &Term) -> Result<(), PatchError> {
    let Term::Map(m) = set else {
        return Err(PatchError::Validate(":set must be a map".to_string()));
    };
    let tbl = v
        .as_table_mut()
        .ok_or_else(|| PatchError::Validate("manifest must be a table".to_string()))?;
    for (k, vv) in m {
        let Term::Symbol(key) = &k.0 else {
            return Err(PatchError::Validate(
                "manifest :set keys must be symbols".to_string(),
            ));
        };
        // Strip leading ':' for convenience.
        let key = key.strip_prefix(':').unwrap_or(key.as_str()).to_string();
        tbl.insert(key, coreform_to_toml(vv)?);
    }
    Ok(())
}

fn coreform_to_toml(t: &Term) -> Result<toml::Value, PatchError> {
    use base64::Engine as _;
    match t {
        Term::Nil => Ok(toml::Value::String("nil".to_string())),
        Term::Bool(b) => Ok(toml::Value::Boolean(*b)),
        Term::Int(i) => {
            Ok(toml::Value::Integer(i.to_i64().ok_or_else(|| {
                PatchError::Validate("int out of range".to_string())
            })?))
        }
        Term::Str(s) => Ok(toml::Value::String(s.clone())),
        Term::Bytes(b) => Ok(toml::Value::String(
            base64::engine::general_purpose::STANDARD.encode(b),
        )),
        Term::Symbol(s) => Ok(toml::Value::String(s.clone())),
        Term::Vector(xs) => Ok(toml::Value::Array(
            xs.iter().map(coreform_to_toml).collect::<Result<_, _>>()?,
        )),
        Term::Map(m) => Ok(toml::Value::Table(
            m.iter()
                .map(|(k, v)| {
                    let kk = match &k.0 {
                        Term::Symbol(s) => s.clone(),
                        Term::Str(s) => s.clone(),
                        other => print_term(other),
                    };
                    Ok((kk, coreform_to_toml(v)?))
                })
                .collect::<Result<_, PatchError>>()?,
        )),
        Term::Pair(_, _) => Err(PatchError::Validate(
            "cannot convert list to TOML in :set".to_string(),
        )),
    }
}

fn toml_to_coreform(v: &toml::Value) -> Result<Term, PatchError> {
    match v {
        toml::Value::String(s) => Ok(Term::Str(s.clone())),
        toml::Value::Integer(i) => Ok(Term::Int((*i).into())),
        toml::Value::Boolean(b) => Ok(Term::Bool(*b)),
        toml::Value::Float(f) => Ok(Term::Str(f.to_string())),
        toml::Value::Datetime(dt) => Ok(Term::Str(dt.to_string())),
        toml::Value::Array(xs) => Ok(Term::Vector(
            xs.iter().map(toml_to_coreform).collect::<Result<_, _>>()?,
        )),
        toml::Value::Table(m) => Ok(Term::Map(
            m.iter()
                .map(|(k, v)| Ok((TermOrdKey(Term::Str(k.clone())), toml_to_coreform(v)?)))
                .collect::<Result<_, PatchError>>()?,
        )),
    }
}

fn update_manifest_op_to_term(
    set: Option<&Term>,
    obligations_add: &[String],
    obligations_remove: &[String],
    tests_add: &[String],
    tests_remove: &[String],
    caps_policy: Option<&str>,
) -> Result<Term, PatchError> {
    let mut m = BTreeMap::new();
    if let Some(set) = set {
        m.insert(TermOrdKey(Term::symbol(":set")), set.clone());
    }
    if !obligations_add.is_empty() {
        m.insert(
            TermOrdKey(Term::symbol(":obligations-add")),
            Term::Vector(
                obligations_add
                    .iter()
                    .map(|s| Term::Str(s.clone()))
                    .collect(),
            ),
        );
    }
    if !obligations_remove.is_empty() {
        m.insert(
            TermOrdKey(Term::symbol(":obligations-remove")),
            Term::Vector(
                obligations_remove
                    .iter()
                    .map(|s| Term::Str(s.clone()))
                    .collect(),
            ),
        );
    }
    if !tests_add.is_empty() {
        m.insert(
            TermOrdKey(Term::symbol(":tests-add")),
            Term::Vector(tests_add.iter().map(|s| Term::Str(s.clone())).collect()),
        );
    }
    if !tests_remove.is_empty() {
        m.insert(
            TermOrdKey(Term::symbol(":tests-remove")),
            Term::Vector(tests_remove.iter().map(|s| Term::Str(s.clone())).collect()),
        );
    }
    if let Some(p) = caps_policy {
        m.insert(
            TermOrdKey(Term::symbol(":caps-policy")),
            Term::Str(p.to_string()),
        );
    }
    Ok(Term::Map(m))
}

fn apply_replace(forms: &mut [Term], path: &[PathStep], new_term: Term) -> Result<(), PatchError> {
    if path.is_empty() {
        return Err(PatchError::Validate("empty path".to_string()));
    }
    let mut cur: ReplaceTarget = ReplaceTarget::Module(forms);
    for (i, step) in path.iter().enumerate() {
        let is_last = i + 1 == path.len();
        cur = cur.step(step, is_last, new_term.clone())?;
    }
    Ok(())
}

enum ReplaceTarget<'a> {
    Module(&'a mut [Term]),
    Term(&'a mut Term),
}

impl<'a> ReplaceTarget<'a> {
    fn step(
        self,
        s: &PathStep,
        is_last: bool,
        new_term: Term,
    ) -> Result<ReplaceTarget<'a>, PatchError> {
        match self {
            ReplaceTarget::Module(forms) => match s {
                PathStep::Form(idx) => {
                    let t = forms.get_mut(*idx).ok_or_else(|| {
                        PatchError::Validate(format!("form index out of range: {idx}"))
                    })?;
                    if is_last {
                        *t = new_term;
                        Ok(ReplaceTarget::Term(t))
                    } else {
                        Ok(ReplaceTarget::Term(t))
                    }
                }
                _ => Err(PatchError::Validate(
                    "path must start with [:form i]".to_string(),
                )),
            },
            ReplaceTarget::Term(t) => {
                if is_last {
                    // Replace at this node with new_term; but only allowed if step is identity.
                }
                match s {
                    PathStep::PairCar => {
                        let Term::Pair(a, _) = t else {
                            return Err(PatchError::Validate("expected pair".to_string()));
                        };
                        if is_last {
                            **a = new_term;
                        }
                        Ok(ReplaceTarget::Term(a))
                    }
                    PathStep::PairCdr => {
                        let Term::Pair(_, d) = t else {
                            return Err(PatchError::Validate("expected pair".to_string()));
                        };
                        if is_last {
                            **d = new_term;
                        }
                        Ok(ReplaceTarget::Term(d))
                    }
                    PathStep::Vec(idx) => {
                        let Term::Vector(xs) = t else {
                            return Err(PatchError::Validate("expected vector".to_string()));
                        };
                        let elt = xs.get_mut(*idx).ok_or_else(|| {
                            PatchError::Validate(format!("vector index out of range: {idx}"))
                        })?;
                        if is_last {
                            *elt = new_term;
                        }
                        Ok(ReplaceTarget::Term(elt))
                    }
                    PathStep::Map(key) => {
                        let Term::Map(m) = t else {
                            return Err(PatchError::Validate("expected map".to_string()));
                        };
                        let elt = m.get_mut(&TermOrdKey(key.clone())).ok_or_else(|| {
                            PatchError::Validate(format!("missing map key {}", print_term(key)))
                        })?;
                        if is_last {
                            *elt = new_term;
                        }
                        Ok(ReplaceTarget::Term(elt))
                    }
                    PathStep::Form(_) => {
                        Err(PatchError::Validate("unexpected :form step".to_string()))
                    }
                }
            }
        }
    }
}
