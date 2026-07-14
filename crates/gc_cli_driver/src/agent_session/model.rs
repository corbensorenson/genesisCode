use serde::{Deserialize, Serialize};

pub(super) const SESSION_SCHEMA: &str = "genesis/agent-transaction-v0.1";
pub(super) const SNAPSHOT_SCHEMA: &str = "genesis/workspace-snapshot-v0.1";

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum SessionStatus {
    Open,
    Applied,
    Aborted,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct PatchRecord {
    pub(super) patch: String,
    pub(super) before_snapshot: String,
    pub(super) after_snapshot: String,
    pub(super) obligations_ok: bool,
    pub(super) acceptance: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct VerificationRecord {
    pub(super) snapshot: String,
    pub(super) acceptance: String,
    pub(super) obligations_ok: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct SessionRecord {
    pub(super) schema: String,
    pub(super) session: String,
    pub(super) package_manifest: String,
    pub(super) base_snapshot: String,
    pub(super) current_snapshot: String,
    pub(super) status: SessionStatus,
    pub(super) patches: Vec<PatchRecord>,
    pub(super) verification: Option<VerificationRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub(super) struct SnapshotFile {
    pub(super) path: String,
    pub(super) blob: String,
    pub(super) bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct WorkspaceSnapshot {
    pub(super) schema: String,
    pub(super) identity: String,
    pub(super) files: Vec<SnapshotFile>,
}
