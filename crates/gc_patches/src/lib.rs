use std::collections::BTreeMap;
use std::path::Path;

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_term, parse_module, parse_term, print_module,
    print_term,
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
    node_id: String,
    path: Vec<PathStep>,
    new_term_hash: String,
}

fn usize_to_int_term(x: usize) -> Result<Term, PatchError> {
    let i = i64::try_from(x).map_err(|_| PatchError::Validate("index out of range".to_string()))?;
    Ok(Term::Int(i.into()))
}

fn path_steps_to_term(path: &[PathStep]) -> Result<Term, PatchError> {
    let mut steps = Vec::with_capacity(path.len());
    for st in path {
        let t = match st {
            PathStep::Form(i) => Term::Vector(vec![Term::symbol(":form"), usize_to_int_term(*i)?]),
            PathStep::PairCar => Term::Vector(vec![Term::symbol(":pair-car")]),
            PathStep::PairCdr => Term::Vector(vec![Term::symbol(":pair-cdr")]),
            PathStep::Vec(i) => Term::Vector(vec![Term::symbol(":vec"), usize_to_int_term(*i)?]),
            PathStep::Map(k) => Term::Vector(vec![Term::symbol(":map"), k.clone()]),
        };
        steps.push(t);
    }
    Ok(Term::Vector(steps))
}

fn hash32_hex(h: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in h {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn semantic_node_id(module_path: &str, path: &[PathStep]) -> Result<String, PatchError> {
    let path_term = path_steps_to_term(path)?;
    let path_repr = print_term(&path_term);
    let mut h = blake3::Hasher::new();
    h.update(b"GCv0.2\0semantic-node-id\0");
    h.update(module_path.as_bytes());
    h.update(b"\0");
    h.update(path_repr.as_bytes());
    Ok(h.finalize().to_hex().to_string())
}

fn term_tag(t: &Term) -> &'static str {
    match t {
        Term::Nil => "nil",
        Term::Bool(_) => "bool",
        Term::Int(_) => "int",
        Term::Str(_) => "str",
        Term::Bytes(_) => "bytes",
        Term::Symbol(_) => "sym",
        Term::Pair(_, _) => "pair",
        Term::Vector(_) => "vec",
        Term::Map(_) => "map",
    }
}

fn collect_term_nodes(
    module_path: &str,
    path: &mut Vec<PathStep>,
    t: &Term,
    out: &mut Vec<SemanticNodeRecord>,
) -> Result<(), PatchError> {
    let node_id = semantic_node_id(module_path, path)?;
    let path_term = path_steps_to_term(path)?;
    let path_repr = print_term(&path_term);
    out.push(SemanticNodeRecord {
        module_path: module_path.to_string(),
        node_id,
        path: path_term,
        path_repr,
        term_tag: term_tag(t).to_string(),
        term_hash: hash32_hex(hash_term(t)),
    });
    match t {
        Term::Pair(a, d) => {
            path.push(PathStep::PairCar);
            collect_term_nodes(module_path, path, a, out)?;
            path.pop();
            path.push(PathStep::PairCdr);
            collect_term_nodes(module_path, path, d, out)?;
            path.pop();
        }
        Term::Vector(xs) => {
            for (i, child) in xs.iter().enumerate() {
                path.push(PathStep::Vec(i));
                collect_term_nodes(module_path, path, child, out)?;
                path.pop();
            }
        }
        Term::Map(m) => {
            for (k, child) in m {
                path.push(PathStep::Map(k.0.clone()));
                collect_term_nodes(module_path, path, child, out)?;
                path.pop();
            }
        }
        _ => {}
    }
    Ok(())
}

fn semantic_node_index_for_forms(
    module_path: &str,
    forms: &[Term],
) -> Result<Vec<SemanticNodeRecord>, PatchError> {
    let mut out = Vec::new();
    for (i, form) in forms.iter().enumerate() {
        let mut path = vec![PathStep::Form(i)];
        collect_term_nodes(module_path, &mut path, form, &mut out)?;
    }
    Ok(out)
}

fn resolve_node_id_path(
    module_path: &str,
    forms: &[Term],
    node_id: &str,
) -> Result<Vec<PathStep>, PatchError> {
    let nodes = semantic_node_index_for_forms(module_path, forms)?;
    let mut matches = nodes
        .into_iter()
        .filter(|n| n.node_id == node_id)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Err(PatchError::Validate(format!(
            "replace-node-id unknown :node-id {node_id} in module {module_path}"
        )));
    }
    if matches.len() > 1 {
        return Err(PatchError::Validate(format!(
            "replace-node-id ambiguous :node-id {node_id} in module {module_path}"
        )));
    }
    parse_path(&matches.remove(0).path)
}

pub fn semantic_node_index_for_module_with_frontend(
    module_path: &str,
    src: &str,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<Vec<SemanticNodeRecord>, PatchError> {
    let forms = parse_canonicalize_module_src(src, frontend, step_limit, mem_limits)?;
    semantic_node_index_for_forms(module_path, &forms)
}

struct SelfhostPatchToolchain {
    ctx: EvalCtx,
    error_token: SealId,
    validate_patch: Value,
    apply_replace_node: Value,
    print_module_forms: Value,
    print_module_from_content: Value,
    manifest_apply_add_module: Value,
    manifest_apply_remove_module: Value,
    manifest_apply_update_manifest_op: Value,
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
        sh.validate_patch_term(&patch_term, step_limit)?;
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

fn summarize_protocol_error_payload(payload: &Value) -> String {
    let Some(t) = payload.as_data() else {
        return payload.debug_repr();
    };
    match t {
        Term::Map(m) => {
            let code = m
                .get(&TermOrdKey(Term::symbol(":error/code")))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("core/error");
            let msg = m
                .get(&TermOrdKey(Term::symbol(":error/message")))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("error");
            format!("{code}: {msg}")
        }
        _ => print_term(t),
    }
}

fn extract_protocol_error(out: &Value, error_token: SealId) -> Option<String> {
    match out {
        Value::Sealed { token, payload } if *token == error_token => {
            Some(summarize_protocol_error_payload(payload))
        }
        _ => None,
    }
}

impl SelfhostPatchToolchain {
    fn init(
        cfg: &gc_obligations::SelfhostFrontendConfig,
        mem_limits: MemLimits,
    ) -> Result<Self, PatchError> {
        // Toolchain bootstrap is trusted and therefore uncharged.
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(mem_limits);
        let prelude = build_prelude(&mut ctx);
        let error_token = prelude.protocol.error;
        let mut env = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            cfg.bootstrap_mode,
            cfg.artifact.as_deref(),
        )
        .map_err(|e| PatchError::Validate(format!("selfhost/init: {e}")))?;

        let validate_patch = env.get("core/cli::validate-patch").ok_or_else(|| {
            PatchError::Validate("missing binding core/cli::validate-patch".to_string())
        })?;
        let apply_replace_node = env.get("core/cli::apply-replace-node").ok_or_else(|| {
            PatchError::Validate("missing binding core/cli::apply-replace-node".to_string())
        })?;
        let print_module_forms = env.get("core/cli::print-module-forms").ok_or_else(|| {
            PatchError::Validate("missing binding core/cli::print-module-forms".to_string())
        })?;
        let print_module_from_content =
            env.get("core/cli::print-module-from-content")
                .ok_or_else(|| {
                    PatchError::Validate(
                        "missing binding core/cli::print-module-from-content".to_string(),
                    )
                })?;
        let manifest_apply_add_module =
            env.get("core/cli::manifest-apply-add-module")
                .ok_or_else(|| {
                    PatchError::Validate(
                        "missing binding core/cli::manifest-apply-add-module".to_string(),
                    )
                })?;
        let manifest_apply_remove_module = env
            .get("core/cli::manifest-apply-remove-module")
            .ok_or_else(|| {
                PatchError::Validate(
                    "missing binding core/cli::manifest-apply-remove-module".to_string(),
                )
            })?;
        let manifest_apply_update_manifest_op = env
            .get("core/cli::manifest-apply-update-manifest-op")
            .ok_or_else(|| {
                PatchError::Validate(
                    "missing binding core/cli::manifest-apply-update-manifest-op".to_string(),
                )
            })?;

        Ok(SelfhostPatchToolchain {
            ctx,
            error_token,
            validate_patch,
            apply_replace_node,
            print_module_forms,
            print_module_from_content,
            manifest_apply_add_module,
            manifest_apply_remove_module,
            manifest_apply_update_manifest_op,
        })
    }

    fn with_limits(&mut self, step_limit: StepLimit) {
        self.ctx.steps = 0;
        self.ctx.step_limit = step_limit.resolve();
    }

    fn validate_patch_term(
        &mut self,
        patch_term: &Term,
        step_limit: StepLimit,
    ) -> Result<(), PatchError> {
        self.with_limits(step_limit);
        let out = self
            .validate_patch
            .clone()
            .apply(&mut self.ctx, Value::Data(patch_term.clone()))
            .map_err(|e| PatchError::Validate(format!("selfhost validate-patch apply: {e}")))?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli validate-patch failed: {e}"
            )));
        }
        Ok(())
    }

    fn apply_replace_node_term(
        &mut self,
        forms: &[Term],
        path: &Term,
        new_term: &Term,
        step_limit: StepLimit,
    ) -> Result<Vec<Term>, PatchError> {
        self.with_limits(step_limit);
        let mut req = BTreeMap::new();
        req.insert(
            TermOrdKey(Term::symbol(":forms")),
            Term::Vector(forms.to_vec()),
        );
        req.insert(TermOrdKey(Term::symbol(":path")), path.clone());
        req.insert(TermOrdKey(Term::symbol(":new")), new_term.clone());
        let req = Term::Map(req);

        let out = self
            .apply_replace_node
            .clone()
            .apply(&mut self.ctx, Value::Data(req))
            .map_err(|e| PatchError::Validate(format!("selfhost apply-replace-node apply: {e}")))?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli apply-replace-node failed: {e}"
            )));
        }
        let Value::Data(Term::Vector(forms)) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli apply-replace-node must return vector of forms, got {}",
                out.debug_repr()
            )));
        };
        Ok(forms)
    }

    fn print_module_forms_term(
        &mut self,
        forms: &[Term],
        step_limit: StepLimit,
    ) -> Result<String, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .print_module_forms
            .clone()
            .apply(&mut self.ctx, Value::Data(Term::Vector(forms.to_vec())))
            .map_err(|e| PatchError::Validate(format!("selfhost print-module-forms apply: {e}")))?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli print-module-forms failed: {e}"
            )));
        }
        let Value::Data(Term::Str(s)) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli print-module-forms must return string, got {}",
                out.debug_repr()
            )));
        };
        Ok(s)
    }

    fn print_module_from_content_term(
        &mut self,
        content: &Term,
        step_limit: StepLimit,
    ) -> Result<String, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .print_module_from_content
            .clone()
            .apply(&mut self.ctx, Value::Data(content.clone()))
            .map_err(|e| {
                PatchError::Validate(format!("selfhost print-module-from-content apply: {e}"))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli print-module-from-content failed: {e}"
            )));
        }
        let Value::Data(Term::Str(s)) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli print-module-from-content must return string, got {}",
                out.debug_repr()
            )));
        };
        Ok(s)
    }

    fn manifest_apply_add_module_term(
        &mut self,
        manifest: &Term,
        module_path: &str,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .manifest_apply_add_module
            .clone()
            .apply(&mut self.ctx, Value::Data(manifest.clone()))
            .and_then(|f| {
                f.apply(
                    &mut self.ctx,
                    Value::Data(Term::Str(module_path.to_string())),
                )
            })
            .map_err(|e| {
                PatchError::Validate(format!("selfhost manifest-apply-add-module apply: {e}"))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-add-module failed: {e}"
            )));
        }
        let Value::Data(t) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-add-module must return data term, got {}",
                out.debug_repr()
            )));
        };
        Ok(t)
    }

    fn manifest_apply_remove_module_term(
        &mut self,
        manifest: &Term,
        module_path: &str,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .manifest_apply_remove_module
            .clone()
            .apply(&mut self.ctx, Value::Data(manifest.clone()))
            .and_then(|f| {
                f.apply(
                    &mut self.ctx,
                    Value::Data(Term::Str(module_path.to_string())),
                )
            })
            .map_err(|e| {
                PatchError::Validate(format!("selfhost manifest-apply-remove-module apply: {e}"))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-remove-module failed: {e}"
            )));
        }
        let Value::Data(t) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-remove-module must return data term, got {}",
                out.debug_repr()
            )));
        };
        Ok(t)
    }

    fn manifest_apply_update_manifest_op_term(
        &mut self,
        manifest: &Term,
        op: &Term,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .manifest_apply_update_manifest_op
            .clone()
            .apply(&mut self.ctx, Value::Data(manifest.clone()))
            .and_then(|f| f.apply(&mut self.ctx, Value::Data(op.clone())))
            .map_err(|e| {
                PatchError::Validate(format!(
                    "selfhost manifest-apply-update-manifest-op apply: {e}"
                ))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-update-manifest-op failed: {e}"
            )));
        }
        let Value::Data(t) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-update-manifest-op must return data term, got {}",
                out.debug_repr()
            )));
        };
        Ok(t)
    }
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
            em.insert(
                TermOrdKey(Term::symbol(":node-id")),
                Term::Str(edit.node_id.clone()),
            );
            em.insert(
                TermOrdKey(Term::symbol(":path")),
                path_steps_to_term(&edit.path).unwrap_or(Term::Vector(Vec::new())),
            );
            em.insert(
                TermOrdKey(Term::symbol(":new-term-h")),
                Term::Str(edit.new_term_hash.clone()),
            );
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
                node_id: semantic_node_id(module_path, path)?,
                path: path.clone(),
                new_term_hash: hash32_hex(hash_term(new_term)),
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
                node_id: node_id.clone(),
                path,
                new_term_hash: hash32_hex(hash_term(new_term)),
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

impl Patch {
    fn from_term(t: &Term) -> Result<Self, PatchError> {
        let Term::Map(m) = t else {
            return Err(PatchError::Validate("patch must be a map".to_string()));
        };
        let version = get_int(m, ":version")?.unwrap_or(1);
        let intent = get_str(m, ":intent")?.unwrap_or_else(|| "".to_string());
        let provenance = m
            .get(&TermOrdKey(Term::Symbol(":provenance".to_string())))
            .cloned()
            .unwrap_or(Term::Map(BTreeMap::new()));
        let ops_t = m
            .get(&TermOrdKey(Term::Symbol(":ops".to_string())))
            .ok_or_else(|| PatchError::Validate("missing :ops".to_string()))?;
        let Term::Vector(ops) = ops_t else {
            return Err(PatchError::Validate(":ops must be a vector".to_string()));
        };
        let mut parsed = Vec::new();
        for op in ops {
            parsed.push(parse_op(op)?);
        }
        Ok(Self {
            version,
            intent,
            provenance,
            ops: parsed,
        })
    }
}

fn parse_op(t: &Term) -> Result<PatchOp, PatchError> {
    let Term::Map(m) = t else {
        return Err(PatchError::Validate("op must be a map".to_string()));
    };
    let op = match m.get(&TermOrdKey(Term::Symbol(":op".to_string()))) {
        Some(Term::Symbol(s)) => s.as_str(),
        Some(x) => {
            return Err(PatchError::Validate(format!(
                ":op must be symbol, got {}",
                print_term(x)
            )));
        }
        None => return Err(PatchError::Validate("missing :op".to_string())),
    };
    match op {
        ":replace-node" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("replace-node missing :module-path".to_string())
            })?;
            let path_t = m
                .get(&TermOrdKey(Term::Symbol(":path".to_string())))
                .ok_or_else(|| PatchError::Validate("replace-node missing :path".to_string()))?;
            let path = parse_path(path_t)?;
            let new_term = m
                .get(&TermOrdKey(Term::Symbol(":new".to_string())))
                .ok_or_else(|| PatchError::Validate("replace-node missing :new".to_string()))?
                .clone();
            Ok(PatchOp::ReplaceNode {
                module_path,
                path,
                new_term,
            })
        }
        ":replace-node-id" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("replace-node-id missing :module-path".to_string())
            })?;
            let node_id = get_str(m, ":node-id")?.ok_or_else(|| {
                PatchError::Validate("replace-node-id missing :node-id".to_string())
            })?;
            if node_id.trim().is_empty() {
                return Err(PatchError::Validate(
                    "replace-node-id :node-id must be non-empty".to_string(),
                ));
            }
            let new_term = m
                .get(&TermOrdKey(Term::Symbol(":new".to_string())))
                .ok_or_else(|| PatchError::Validate("replace-node-id missing :new".to_string()))?
                .clone();
            Ok(PatchOp::ReplaceNodeId {
                module_path,
                node_id,
                new_term,
            })
        }
        ":add-module" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("add-module missing :module-path".to_string())
            })?;
            let content_t = m
                .get(&TermOrdKey(Term::Symbol(":content".to_string())))
                .ok_or_else(|| PatchError::Validate("add-module missing :content".to_string()))?;
            let content = match content_t {
                Term::Str(s) => ModuleContent::Source(s.clone()),
                Term::Vector(xs) => ModuleContent::Forms(xs.clone()),
                _ => {
                    return Err(PatchError::Validate(
                        ":content must be string or vector".to_string(),
                    ));
                }
            };
            Ok(PatchOp::AddModule {
                module_path,
                content,
            })
        }
        ":remove-module" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("remove-module missing :module-path".to_string())
            })?;
            Ok(PatchOp::RemoveModule { module_path })
        }
        ":update-manifest" => {
            let set = m
                .get(&TermOrdKey(Term::Symbol(":set".to_string())))
                .cloned();
            let obligations_add = get_sym_vec(m, ":obligations-add")?;
            let obligations_remove = get_sym_vec(m, ":obligations-remove")?;
            let tests_add = get_sym_vec(m, ":tests-add")?;
            let tests_remove = get_sym_vec(m, ":tests-remove")?;
            let caps_policy = get_str(m, ":caps-policy")?;
            Ok(PatchOp::UpdateManifest {
                set,
                obligations_add,
                obligations_remove,
                tests_add,
                tests_remove,
                caps_policy,
            })
        }
        other => Err(PatchError::Validate(format!("unknown op {other}"))),
    }
}

fn parse_path(t: &Term) -> Result<Vec<PathStep>, PatchError> {
    let Term::Vector(steps) = t else {
        return Err(PatchError::Validate(":path must be a vector".to_string()));
    };
    let mut out = Vec::new();
    for s in steps {
        let Term::Vector(items) = s else {
            return Err(PatchError::Validate(
                "path step must be a vector".to_string(),
            ));
        };
        if items.is_empty() {
            return Err(PatchError::Validate("empty path step".to_string()));
        }
        let tag = match &items[0] {
            Term::Symbol(x) => x.as_str(),
            other => {
                return Err(PatchError::Validate(format!(
                    "bad path tag {}",
                    print_term(other)
                )));
            }
        };
        match tag {
            ":form" => {
                if items.len() != 2 {
                    return Err(PatchError::Validate(":form expects 1 arg".to_string()));
                }
                let idx = term_to_usize(&items[1])?;
                out.push(PathStep::Form(idx));
            }
            ":pair-car" => out.push(PathStep::PairCar),
            ":pair-cdr" => out.push(PathStep::PairCdr),
            ":vec" => {
                if items.len() != 2 {
                    return Err(PatchError::Validate(":vec expects 1 arg".to_string()));
                }
                out.push(PathStep::Vec(term_to_usize(&items[1])?));
            }
            ":map" => {
                if items.len() != 2 {
                    return Err(PatchError::Validate(":map expects 1 arg".to_string()));
                }
                out.push(PathStep::Map(items[1].clone()));
            }
            other => return Err(PatchError::Validate(format!("unknown path step {other}"))),
        }
    }
    Ok(out)
}

fn term_to_usize(t: &Term) -> Result<usize, PatchError> {
    match t {
        Term::Int(i) => i
            .to_usize()
            .ok_or_else(|| PatchError::Validate("index out of range".to_string())),
        _ => Err(PatchError::Validate("index must be int".to_string())),
    }
}

fn get_int(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Option<u64>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(None),
        Some(Term::Int(i)) => {
            Ok(Some(i.to_u64().ok_or_else(|| {
                PatchError::Validate(format!("{k} out of range"))
            })?))
        }
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be int, got {}",
            print_term(x)
        ))),
    }
}

fn get_str(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Option<String>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be string, got {}",
            print_term(x)
        ))),
    }
}

fn get_sym_vec(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Vec<String>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(Vec::new()),
        Some(Term::Vector(xs)) => Ok(xs
            .iter()
            .filter_map(|t| match t {
                Term::Symbol(s) => Some(s.clone()),
                _ => None,
            })
            .collect()),
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be vector, got {}",
            print_term(x)
        ))),
    }
}
