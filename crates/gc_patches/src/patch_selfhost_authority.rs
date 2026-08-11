use super::*;
use crate::patch_protocol::extract_protocol_error;
use crate::patch_selfhost_toolchain::SelfhostPatchToolchain;

impl SelfhostPatchToolchain {
    fn apply_authority_binding(
        &mut self,
        binding: Value,
        request: Term,
        step_limit: StepLimit,
        context: &str,
    ) -> Result<Term, PatchError> {
        self.with_limits(step_limit);
        let value = binding
            .apply(&mut self.ctx, Value::data(request))
            .map_err(|error| PatchError::Validate(format!("{context} apply: {error}")))?;
        if let Some(error) = extract_protocol_error(&value, self.error_token) {
            return Err(PatchError::Validate(format!("{context} failed: {error}")));
        }
        Ok(value
            .as_data()
            .cloned()
            .unwrap_or_else(|| value.to_term_for_log(self.ctx.protocol.map(|p| p.error))))
    }

    pub(super) fn normalize_patch_report_term(
        &mut self,
        request: Term,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.apply_authority_binding(
            self.patch_normalize.clone(),
            request,
            step_limit,
            "patch-authority: patch-normalize",
        )
    }

    pub(super) fn preflight_report_term(
        &mut self,
        request: Term,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.apply_authority_binding(
            self.patch_preflight.clone(),
            request,
            step_limit,
            "patch-preflight: authority",
        )
    }

    pub(super) fn refactor_plan_report_term(
        &mut self,
        request: Term,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.apply_authority_binding(
            self.refactor_plan.clone(),
            request,
            step_limit,
            "refactor-plan: authority",
        )
    }

    pub(super) fn patch_diff_report_term(
        &mut self,
        request: Term,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.apply_authority_binding(
            self.patch_diff.clone(),
            request,
            step_limit,
            "patch-diff: authority",
        )
    }
}
