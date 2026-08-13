use super::*;

const PLAN_REQUEST_KIND: &str = "genesis/pkg-resolution-plan-request-v0.1";
const PLAN_RESULT_KIND: &str = "genesis/pkg-resolution-plan-result-v0.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PkgResolutionSelector {
    Commit(String),
    Snapshot(String),
    Ref(String),
    SemverRange(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemverSelectionPolicy {
    Highest,
    Lowest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PkgResolutionPlan {
    pub(crate) selector: PkgResolutionSelector,
    pub(crate) semver_policy: Option<SemverSelectionPolicy>,
    pub(crate) should_resolve: bool,
}

#[derive(Debug)]
pub(crate) enum PkgResolutionPlanError {
    Rejected { code: String, message: String },
    Boundary(EffectsError),
}

impl PkgResolutionIdentityAuthority {
    pub(crate) fn plan(
        &mut self,
        requirement: &gc_pkg::Requirement,
        has_existing: bool,
    ) -> Result<PkgResolutionPlan, PkgResolutionPlanError> {
        let request = map([
            (":has-existing", Term::Bool(has_existing)),
            (":kind", Term::Str(PLAN_REQUEST_KIND.to_string())),
            (":op", Term::symbol(":plan")),
            (":selector", Term::Str(requirement.selector.clone())),
            (
                ":strategy",
                Term::symbol(format!(":{}", requirement.strategy.as_str())),
            ),
            (
                ":tag-policy",
                requirement
                    .tag_policy
                    .clone()
                    .map(Term::Str)
                    .unwrap_or(Term::Nil),
            ),
            (
                ":update-policy",
                Term::symbol(match requirement.update_policy {
                    gc_pkg::UpdatePolicy::Manual => ":manual",
                    gc_pkg::UpdatePolicy::Auto => ":auto",
                }),
            ),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .plan_authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| {
                PkgResolutionPlanError::Boundary(authority_error(format!(
                    "plan apply failed: {error}"
                )))
            })?;
        let result =
            plain_result(value, &self.context).map_err(PkgResolutionPlanError::Boundary)?;
        decode_plan_result(result, request_hash)
    }
}

fn decode_plan_result(
    term: Term,
    request_hash: [u8; 32],
) -> Result<PkgResolutionPlan, PkgResolutionPlanError> {
    let fields = exact_map(
        &term,
        &[
            ":code",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":selector-kind",
            ":selector-value",
            ":semver-policy",
            ":should-resolve",
            ":v",
        ],
    )
    .map_err(PkgResolutionPlanError::Boundary)?;
    require_string(fields, ":kind", PLAN_RESULT_KIND).map_err(PkgResolutionPlanError::Boundary)?;
    require_int(fields, ":v", 1).map_err(PkgResolutionPlanError::Boundary)?;
    require_string(fields, ":request-h", &hex32(request_hash))
        .map_err(PkgResolutionPlanError::Boundary)?;
    if !required_bool(fields, ":ok").map_err(PkgResolutionPlanError::Boundary)? {
        for name in [
            ":selector-kind",
            ":selector-value",
            ":semver-policy",
            ":should-resolve",
        ] {
            require_nil(fields, name).map_err(PkgResolutionPlanError::Boundary)?;
        }
        let code = required_string(fields, ":code").map_err(PkgResolutionPlanError::Boundary)?;
        if !matches!(
            code,
            "core/pkg/bad-selector"
                | "core/pkg/strategy-mismatch"
                | "core/pkg/tag-policy-required"
                | "core/pkg/tag-policy-forbidden"
                | "core/pkg/semver-policy-unsupported"
                | "core/pkg/bad-authority-request"
        ) {
            return Err(PkgResolutionPlanError::Boundary(authority_error(
                "result :code is outside the closed rejection inventory",
            )));
        }
        return Err(PkgResolutionPlanError::Rejected {
            code: code.to_string(),
            message: required_string(fields, ":message")
                .map_err(PkgResolutionPlanError::Boundary)?
                .to_string(),
        });
    }
    require_nil(fields, ":code").map_err(PkgResolutionPlanError::Boundary)?;
    require_nil(fields, ":message").map_err(PkgResolutionPlanError::Boundary)?;
    let selector_value = required_string(fields, ":selector-value")
        .map_err(PkgResolutionPlanError::Boundary)?
        .to_string();
    let selector_kind =
        required_symbol(fields, ":selector-kind").map_err(PkgResolutionPlanError::Boundary)?;
    let selector = match selector_kind {
        ":commit" if is_hash_case_insensitive(&selector_value) => {
            PkgResolutionSelector::Commit(selector_value)
        }
        ":snapshot" if is_hash_case_insensitive(&selector_value) => {
            PkgResolutionSelector::Snapshot(selector_value)
        }
        ":ref" if selector_value.starts_with("refs/") => PkgResolutionSelector::Ref(selector_value),
        ":semver-range" if !selector_value.is_empty() => {
            PkgResolutionSelector::SemverRange(selector_value)
        }
        other => {
            return Err(PkgResolutionPlanError::Boundary(authority_error(format!(
                "invalid plan selector pair: kind={other} value={selector_value:?}"
            ))));
        }
    };
    let semver_policy =
        match field(fields, ":semver-policy").map_err(PkgResolutionPlanError::Boundary)? {
            Term::Nil => None,
            Term::Symbol(value) if value == ":highest" => Some(SemverSelectionPolicy::Highest),
            Term::Symbol(value) if value == ":lowest" => Some(SemverSelectionPolicy::Lowest),
            _ => {
                return Err(PkgResolutionPlanError::Boundary(authority_error(
                    "result :semver-policy must be nil, :highest, or :lowest",
                )));
            }
        };
    if matches!(selector, PkgResolutionSelector::SemverRange(_)) != semver_policy.is_some() {
        return Err(PkgResolutionPlanError::Boundary(authority_error(
            "semver selector and :semver-policy disagree",
        )));
    }
    let should_resolve =
        required_bool(fields, ":should-resolve").map_err(PkgResolutionPlanError::Boundary)?;
    Ok(PkgResolutionPlan {
        selector,
        semver_policy,
        should_resolve,
    })
}

fn required_symbol<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, EffectsError> {
    match field(fields, name)? {
        Term::Symbol(value) => Ok(value),
        _ => Err(authority_error(format!("result {name} must be symbol"))),
    }
}

fn is_hash_case_insensitive(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact_config() -> SelfhostAuthorityConfig {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let artifact = std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
            .map(std::path::PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    workspace.join(path)
                }
            })
            .unwrap_or_else(|| workspace.join("selfhost/toolchain.gc"))
            .canonicalize()
            .expect("canonical selfhost artifact path");
        SelfhostAuthorityConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        }
    }

    fn req(
        selector: &str,
        strategy: gc_pkg::ResolutionStrategy,
        tag_policy: Option<&str>,
        update_policy: gc_pkg::UpdatePolicy,
    ) -> gc_pkg::Requirement {
        gc_pkg::Requirement {
            selector: selector.to_string(),
            update_policy,
            registry: None,
            strategy,
            tag_policy: tag_policy.map(str::to_string),
        }
    }

    #[test]
    fn planning_authority_covers_selector_and_update_matrix() {
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let hash = "A".repeat(64);
        let cases = [
            (
                req(
                    &format!(" commit: {hash} "),
                    gc_pkg::ResolutionStrategy::Pinned,
                    None,
                    gc_pkg::UpdatePolicy::Manual,
                ),
                true,
                PkgResolutionSelector::Commit(hash.clone()),
                None,
                false,
            ),
            (
                req(
                    &format!("snapshot:{hash}"),
                    gc_pkg::ResolutionStrategy::Pinned,
                    None,
                    gc_pkg::UpdatePolicy::Manual,
                ),
                false,
                PkgResolutionSelector::Snapshot(hash.clone()),
                None,
                true,
            ),
            (
                req(
                    "ref: refs/heads/main ",
                    gc_pkg::ResolutionStrategy::TrackRef,
                    None,
                    gc_pkg::UpdatePolicy::Auto,
                ),
                true,
                PkgResolutionSelector::Ref("refs/heads/main".to_string()),
                None,
                true,
            ),
            (
                req(
                    "refs/tags/v1.2.3",
                    gc_pkg::ResolutionStrategy::TagPolicy,
                    Some("exact"),
                    gc_pkg::UpdatePolicy::Manual,
                ),
                true,
                PkgResolutionSelector::Ref("refs/tags/v1.2.3".to_string()),
                None,
                false,
            ),
            (
                req(
                    "semver: ^1.2",
                    gc_pkg::ResolutionStrategy::TagPolicy,
                    Some("lowest"),
                    gc_pkg::UpdatePolicy::Auto,
                ),
                true,
                PkgResolutionSelector::SemverRange("^1.2".to_string()),
                Some(SemverSelectionPolicy::Lowest),
                true,
            ),
        ];
        for (requirement, has_existing, selector, semver_policy, should_resolve) in cases {
            let plan = authority
                .plan(&requirement, has_existing)
                .unwrap_or_else(|error| panic!("selector {:?}: {error:?}", requirement.selector));
            assert_eq!(
                plan,
                PkgResolutionPlan {
                    selector,
                    semver_policy,
                    should_resolve
                }
            );
        }
    }

    #[test]
    fn planning_authority_rejects_malformed_and_incoherent_requirements() {
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let cases = [
            (
                req(
                    "commit:abc",
                    gc_pkg::ResolutionStrategy::Pinned,
                    None,
                    gc_pkg::UpdatePolicy::Manual,
                ),
                "core/pkg/bad-selector",
            ),
            (
                req(
                    "ref:main",
                    gc_pkg::ResolutionStrategy::TrackRef,
                    None,
                    gc_pkg::UpdatePolicy::Manual,
                ),
                "core/pkg/bad-selector",
            ),
            (
                req(
                    "refs/heads/main",
                    gc_pkg::ResolutionStrategy::Pinned,
                    None,
                    gc_pkg::UpdatePolicy::Manual,
                ),
                "core/pkg/strategy-mismatch",
            ),
            (
                req(
                    "semver:^1",
                    gc_pkg::ResolutionStrategy::TagPolicy,
                    None,
                    gc_pkg::UpdatePolicy::Manual,
                ),
                "core/pkg/tag-policy-required",
            ),
            (
                req(
                    "semver:^1",
                    gc_pkg::ResolutionStrategy::TagPolicy,
                    Some("newest-ish"),
                    gc_pkg::UpdatePolicy::Manual,
                ),
                "core/pkg/semver-policy-unsupported",
            ),
        ];
        for (requirement, expected_code) in cases {
            assert!(matches!(
                authority.plan(&requirement, true),
                Err(PkgResolutionPlanError::Rejected { ref code, .. }) if code == expected_code
            ));
        }
    }

    #[test]
    fn plan_decoder_rejects_open_unbound_contradictory_and_unknown_results() {
        let request_hash = [7_u8; 32];
        let base = map([
            (":code", Term::Nil),
            (":kind", Term::Str(PLAN_RESULT_KIND.to_string())),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":request-h", Term::Str(hex32(request_hash))),
            (":selector-kind", Term::symbol(":commit")),
            (":selector-value", Term::Str("a".repeat(64))),
            (":semver-policy", Term::Nil),
            (":should-resolve", Term::Bool(true)),
            (":v", Term::Int(1.into())),
        ]);
        let mutate = |term: &Term, name: &str, value: Term| match term.clone() {
            Term::Map(mut fields) => {
                fields.insert(TermOrdKey(Term::symbol(name)), value);
                Term::Map(fields)
            }
            _ => term.clone(),
        };
        assert!(decode_plan_result(mutate(&base, ":extra", Term::Nil), request_hash).is_err());
        assert!(
            decode_plan_result(
                mutate(&base, ":request-h", Term::Str("0".repeat(64))),
                request_hash
            )
            .is_err()
        );
        assert!(
            decode_plan_result(
                mutate(&base, ":semver-policy", Term::symbol(":highest")),
                request_hash
            )
            .is_err()
        );
        let rejected = map([
            (":code", Term::Str("core/pkg/unknown".to_string())),
            (":kind", Term::Str(PLAN_RESULT_KIND.to_string())),
            (":message", Term::Str("no".to_string())),
            (":ok", Term::Bool(false)),
            (":request-h", Term::Str(hex32(request_hash))),
            (":selector-kind", Term::Nil),
            (":selector-value", Term::Nil),
            (":semver-policy", Term::Nil),
            (":should-resolve", Term::Nil),
            (":v", Term::Int(1.into())),
        ]);
        assert!(matches!(
            decode_plan_result(rejected, request_hash),
            Err(PkgResolutionPlanError::Boundary(_))
        ));
    }
}
