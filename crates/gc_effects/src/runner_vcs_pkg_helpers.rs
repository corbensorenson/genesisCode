use super::*;

/// Borrowed policy, verifier cache and sealed diagnostic context for commit observations.
/// Constructing this context grants no authority and performs no observation or validation.
pub(crate) struct CommitValidationContext<'a> {
    pub(crate) policy: &'a CapsPolicy,
    pub(crate) commit_authority: &'a mut Option<CommitAuthority>,
    pub(crate) error_tok: SealId,
    pub(crate) op: &'a str,
}

impl<'a> CommitValidationContext<'a> {
    pub(crate) fn new(
        policy: &'a CapsPolicy,
        commit_authority: &'a mut Option<CommitAuthority>,
        error_tok: SealId,
        op: &'a str,
    ) -> Self {
        Self {
            policy,
            commit_authority,
            error_tok,
            op,
        }
    }
}

#[path = "runner_vcs_pkg_helpers/pkg_resolution.rs"]
mod pkg_resolution;
#[path = "runner_vcs_pkg_helpers/vcs_history.rs"]
mod vcs_history;
#[path = "runner_vcs_pkg_helpers/vcs_patch_merge.rs"]
mod vcs_patch_merge;

pub(super) use pkg_resolution::*;
pub(super) use vcs_history::*;
pub(super) use vcs_patch_merge::*;
