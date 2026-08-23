mod lock;
mod manifest;
mod manifest_authority;
mod workspace;

pub const PACKAGE_PROFILE_ID: &str = "genesis/package-profile/v0.2";

pub use crate::manifest::{
    Budgets, DepEntry, Limits, ManifestError, ModuleEntry, PACKAGE_MANIFEST_SCHEMA_VERSION,
    PackageManifest, PropertyConfig,
};
pub use crate::manifest_authority::{
    PACKAGE_MANIFEST_AUTHORITY_BINDING, PACKAGE_MANIFEST_AUTHORITY_REQUEST_KIND,
    PACKAGE_MANIFEST_AUTHORITY_RESULT_KIND, PackageManifestTransport,
    decode_authorized_package_manifest, package_manifest_authority_request,
    read_package_manifest_transport,
};

pub use crate::lock::{
    GENESIS_LOCK_CURRENT_VERSION, GENESIS_LOCK_LEGACY_VERSION, GenesisLock, LockError, LockedEntry,
    Requirement, ResolutionStrategy, SelectorKind, UpdatePolicy, classify_selector,
    default_lock_path, infer_strategy,
};
pub use crate::workspace::{
    GENESIS_WORKSPACE_VERSION, RUNTIME_BACKEND_BACKEND, RUNTIME_BACKEND_GFX, RUNTIME_BACKEND_GPU,
    RUNTIME_BACKEND_HEADLESS, WorkspaceConfig, WorkspaceDefaults, WorkspaceError, WorkspaceMember,
    WorkspaceProfile, WorkspaceTask, normalize_runtime_backend_profile,
    runtime_backend_profile_is_compatible,
};
