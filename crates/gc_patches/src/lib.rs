use std::collections::BTreeMap;
use std::path::Path;

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, hash_term, parse_term, print_module,
    print_term,
};
use gc_kernel::{Apply, EvalCtx, MemLimits, SealId, StepLimit, Value};
use gc_obligations::{
    CoreformFrontend, EvidenceStore, ObligationError, PackageTestResult, coreform_frontend_is_rust,
    default_coreform_frontend, pack, test_package_with_step_limit_and_frontend,
};
use gc_pkg::PackageManifest;
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_with_mode};
use num_traits::ToPrimitive;
use thiserror::Error;

pub const SEMANTIC_PATCH_PROFILE_ID: &str = "genesis/patch-profile/v0.2";
pub const SEMANTIC_PATCH_VERSION: u64 = 1;

#[path = "patch_apply.rs"]
mod patch_apply;
#[path = "patch_manifest.rs"]
mod patch_manifest;
#[path = "patch_parse.rs"]
mod patch_parse;
#[path = "patch_refactor.rs"]
mod patch_refactor;
#[path = "patch_replace.rs"]
mod patch_replace;
#[path = "patch_selfhost_toolchain.rs"]
mod patch_selfhost_toolchain;
#[path = "patch_semantic.rs"]
mod patch_semantic;

use patch_manifest::{
    apply_manifest_set, coreform_to_toml, parse_canonicalize_module_src,
    patch_manifest_add_module_path, patch_manifest_move_module_path, patch_string_vec_field,
    toml_to_coreform, update_manifest_op_to_term,
};
use patch_replace::apply_replace;
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
    patch_apply::apply_patch_with_step_limit_and_frontend(
        patch_path,
        pkg_toml,
        caps_override,
        step_limit,
        mem_limits,
        frontend,
    )
}

/// Validate a patch term using the selected frontend.
///
/// Production callers should prefer this API so patch-schema acceptance follows
/// selfhost `.gc` semantics when running with the selfhost frontend.
pub fn validate_patch_term_with_frontend(
    t: &Term,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<(), PatchError> {
    if coreform_frontend_is_rust(frontend) {
        let _ = Patch::from_term(t)?;
        return Ok(());
    }
    let CoreformFrontend::Selfhost(cfg) = frontend else {
        return Err(PatchError::Validate(
            "invalid frontend dispatch while validating patch".to_string(),
        ));
    };
    let mut sh = SelfhostPatchToolchain::init(cfg, mem_limits)?;
    sh.validate_patch_term(t, step_limit)
}
