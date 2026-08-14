use super::*;

const REQUEST_KIND: &str = "genesis/pkg-semver-select-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-semver-select-result-v0.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PkgSemverCandidate {
    pub(crate) ref_name: String,
    pub(crate) commit: String,
    pub(crate) rank: u64,
}

impl PkgResolutionIdentityAuthority {
    pub(crate) fn select_semver(
        &mut self,
        candidates: &[PkgSemverCandidate],
        policy: SemverSelectionPolicy,
    ) -> Result<Option<(String, String)>, PkgResolutionPlanError> {
        let candidate_terms: Result<Vec<Term>, PkgResolutionPlanError> = candidates
            .iter()
            .map(|candidate| {
                let rank = i64::try_from(candidate.rank).map_err(|_| {
                    PkgResolutionPlanError::Boundary(authority_error(
                        "semver candidate rank exceeds the protocol integer range",
                    ))
                })?;
                Ok(map([
                    (":commit", Term::Str(candidate.commit.clone())),
                    (":rank", Term::Int(rank.into())),
                    (":ref", Term::Str(candidate.ref_name.clone())),
                ]))
            })
            .collect();
        let request = map([
            (":candidates", Term::Vector(candidate_terms?)),
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":op", Term::symbol(":select")),
            (
                ":policy",
                Term::symbol(match policy {
                    SemverSelectionPolicy::Highest => ":highest",
                    SemverSelectionPolicy::Lowest => ":lowest",
                }),
            ),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .semver_select_authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| {
                PkgResolutionPlanError::Boundary(authority_error(format!(
                    "semver selection apply failed: {error}"
                )))
            })?;
        let result =
            plain_result(value, &self.context).map_err(PkgResolutionPlanError::Boundary)?;
        decode_select_result(result, request_hash, candidates)
    }
}

fn decode_select_result(
    term: Term,
    request_hash: [u8; 32],
    candidates: &[PkgSemverCandidate],
) -> Result<Option<(String, String)>, PkgResolutionPlanError> {
    let fields = exact_map(
        &term,
        &[
            ":code",
            ":commit",
            ":kind",
            ":message",
            ":ok",
            ":rank",
            ":ref",
            ":request-h",
            ":v",
        ],
    )
    .map_err(PkgResolutionPlanError::Boundary)?;
    require_string(fields, ":kind", RESULT_KIND).map_err(PkgResolutionPlanError::Boundary)?;
    require_int(fields, ":v", 1).map_err(PkgResolutionPlanError::Boundary)?;
    require_string(fields, ":request-h", &hex32(request_hash))
        .map_err(PkgResolutionPlanError::Boundary)?;
    if !required_bool(fields, ":ok").map_err(PkgResolutionPlanError::Boundary)? {
        for name in [":commit", ":rank", ":ref"] {
            require_nil(fields, name).map_err(PkgResolutionPlanError::Boundary)?;
        }
        let code = required_string(fields, ":code").map_err(PkgResolutionPlanError::Boundary)?;
        if code != "core/pkg/bad-authority-request" {
            return Err(PkgResolutionPlanError::Boundary(authority_error(
                "semver result :code is outside the closed rejection inventory",
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
    if matches!(
        field(fields, ":ref").map_err(PkgResolutionPlanError::Boundary)?,
        Term::Nil
    ) {
        require_nil(fields, ":commit").map_err(PkgResolutionPlanError::Boundary)?;
        require_nil(fields, ":rank").map_err(PkgResolutionPlanError::Boundary)?;
        if candidates.is_empty() {
            return Ok(None);
        }
        return Err(PkgResolutionPlanError::Boundary(authority_error(
            "semver authority returned no match for a nonempty candidate set",
        )));
    }
    let ref_name = required_string(fields, ":ref")
        .map_err(PkgResolutionPlanError::Boundary)?
        .to_string();
    let commit = required_string(fields, ":commit")
        .map_err(PkgResolutionPlanError::Boundary)?
        .to_string();
    let rank = match field(fields, ":rank").map_err(PkgResolutionPlanError::Boundary)? {
        Term::Int(value) => u64::try_from(value.clone()).map_err(|_| {
            PkgResolutionPlanError::Boundary(authority_error(
                "semver result :rank must be a nonnegative u64",
            ))
        })?,
        _ => {
            return Err(PkgResolutionPlanError::Boundary(authority_error(
                "semver result :rank must be an integer",
            )));
        }
    };
    if !candidates.iter().any(|candidate| {
        candidate.ref_name == ref_name && candidate.commit == commit && candidate.rank == rank
    }) {
        return Err(PkgResolutionPlanError::Boundary(authority_error(
            "semver authority selected a tuple outside the supplied candidate set",
        )));
    }
    Ok(Some((ref_name, commit)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact_config() -> SelfhostAuthorityConfig {
        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let artifact = workspace
            .join("selfhost/toolchain.gc")
            .canonicalize()
            .expect("canonical selfhost artifact path");
        SelfhostAuthorityConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        }
    }

    #[test]
    fn authority_selects_policy_extrema_and_lexical_ties() {
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let candidates = vec![
            PkgSemverCandidate {
                ref_name: "refs/tags/v1.2.5-z".to_string(),
                commit: "c".repeat(64),
                rank: 2,
            },
            PkgSemverCandidate {
                ref_name: "refs/tags/v1.2.0".to_string(),
                commit: "a".repeat(64),
                rank: 0,
            },
            PkgSemverCandidate {
                ref_name: "refs/tags/v1.2.5-a".to_string(),
                commit: "b".repeat(64),
                rank: 2,
            },
        ];
        assert_eq!(
            authority
                .select_semver(&candidates, SemverSelectionPolicy::Highest)
                .unwrap(),
            Some(("refs/tags/v1.2.5-a".to_string(), "b".repeat(64)))
        );
        assert_eq!(
            authority
                .select_semver(&candidates, SemverSelectionPolicy::Lowest)
                .unwrap(),
            Some(("refs/tags/v1.2.0".to_string(), "a".repeat(64)))
        );
        assert_eq!(
            authority
                .select_semver(&[], SemverSelectionPolicy::Highest)
                .unwrap(),
            None
        );
    }

    #[test]
    fn decoder_rejects_open_substituted_and_false_no_match_results() {
        let request_hash = [9_u8; 32];
        let candidate = PkgSemverCandidate {
            ref_name: "refs/tags/v1.0.0".to_string(),
            commit: "a".repeat(64),
            rank: 0,
        };
        let base = map([
            (":code", Term::Nil),
            (":commit", Term::Str(candidate.commit.clone())),
            (":kind", Term::Str(RESULT_KIND.to_string())),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":rank", Term::Int(0.into())),
            (":ref", Term::Str(candidate.ref_name.clone())),
            (":request-h", Term::Str(hex32(request_hash))),
            (":v", Term::Int(1.into())),
        ]);
        let mutate = |term: &Term, name: &str, value: Term| match term.clone() {
            Term::Map(mut fields) => {
                fields.insert(TermOrdKey(Term::symbol(name)), value);
                Term::Map(fields)
            }
            _ => term.clone(),
        };
        assert!(
            decode_select_result(base.clone(), request_hash, std::slice::from_ref(&candidate))
                .is_ok()
        );
        assert!(
            decode_select_result(
                mutate(&base, ":extra", Term::Nil),
                request_hash,
                std::slice::from_ref(&candidate),
            )
            .is_err()
        );
        assert!(
            decode_select_result(
                mutate(
                    &base,
                    ":ref",
                    Term::Str("refs/tags/substituted".to_string())
                ),
                request_hash,
                std::slice::from_ref(&candidate),
            )
            .is_err()
        );
        let false_no_match = mutate(
            &mutate(&mutate(&base, ":ref", Term::Nil), ":commit", Term::Nil),
            ":rank",
            Term::Nil,
        );
        assert!(
            decode_select_result(
                false_no_match,
                request_hash,
                std::slice::from_ref(&candidate)
            )
            .is_err()
        );
    }
}
