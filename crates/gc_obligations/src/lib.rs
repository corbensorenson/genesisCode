mod error;
mod frontend;
mod obligation_cache;
mod obligation_eval_helpers;
mod obligation_exec;
mod obligation_gfx;
mod obligation_lint;
mod obligation_stage;
mod registry_policy;
mod signing;
mod store;
mod transparency;
mod verify;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, hash_term, parse_module, parse_term,
    print_term,
};
use gc_effects::{CapsPolicy, EffectLog};
use gc_kernel::{
    Apply, DecisionCoverageCounters, DecisionSample, Env, EvalCtx, MemLimits, StepLimit, Value,
    compile_module_with_site_namespace, compiled_module_coverage_manifest, eval_compiled_module,
    value_hash,
};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

pub use crate::error::ObligationError;
pub use crate::frontend::{
    CoreformFrontend, SelfhostFrontendConfig, coreform_frontend_is_rust, default_coreform_frontend,
    rust_coreform_frontend, set_frontend_runtime_profile_parity_harness,
};
use crate::frontend::{enforce_frontend_allowed, env_truthy, frontend_is_rust};
use crate::obligation_cache::*;
pub(crate) use crate::obligation_eval_helpers::*;
use crate::obligation_exec::*;
use crate::obligation_lint::{obligation_ai_style, obligation_lint};
use crate::obligation_stage::{
    PackageEval, obligation_stage1_validation, obligation_translation_validation,
};
pub use crate::registry_policy::{RegistryPolicy, RegistryPolicyError};
pub use crate::signing::{
    AcceptanceSignature, KeyFile, SigningError, load_signature_set, read_acceptance_hash_from_last,
    sign_acceptance_hash, signatures_file_path, write_signature_set,
};
pub use crate::store::EvidenceStore;
pub use crate::transparency::{
    TransparencyError, TransparencyVerifyResult, append_transparency_entry, verify_transparency_log,
};
pub use crate::verify::{PackageVerifyResult, verify_package, verify_package_with_policy};
pub use gc_pkg::{DepEntry, ModuleEntry, PackageManifest};

include!("obligations/types_api.rs");
include!("obligations/frontend_module_ops.rs");
include!("obligations/manifest_hashing.rs");
include!("obligations/test_exec.rs");

#[cfg(test)]
// Obligation-library contract tests are split out to keep this production unit below policy limits.
#[path = "tests/mod.rs"]
mod tests;
