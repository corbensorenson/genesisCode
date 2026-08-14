use num_traits::ToPrimitive;

use super::*;

const REQUEST_KIND: &str = "genesis/pkg-verify-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-verify-result-v0.1";

#[derive(Debug, Clone)]
pub(crate) struct PkgVerifyStep {
    pub(crate) commit: Option<String>,
    pub(crate) name: String,
    pub(crate) snapshot: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PkgVerifyPlan {
    pub(crate) hash: String,
    pub(crate) model: Term,
    pub(crate) steps: Vec<PkgVerifyStep>,
    pub(crate) term: Term,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PkgVerifySnapshotStatus {
    Available,
    Missing,
    Corrupt,
    BadSnapshot,
}

impl PkgVerifySnapshotStatus {
    fn term(self) -> Term {
        Term::symbol(match self {
            Self::Available => ":available",
            Self::Missing => ":missing",
            Self::Corrupt => ":corrupt",
            Self::BadSnapshot => ":bad-snapshot",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PkgVerifyHashStatus {
    Present,
    Missing,
    Corrupt,
}

impl PkgVerifyHashStatus {
    fn term(self) -> Term {
        Term::symbol(match self {
            Self::Present => ":present",
            Self::Missing => ":missing",
            Self::Corrupt => ":corrupt",
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PkgVerifyHashObservation {
    pub(crate) hash: String,
    pub(crate) status: PkgVerifyHashStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PkgVerifyClosureStatus {
    Ok,
    Missing,
    Corrupt,
    BadCommit,
    SnapshotMismatch,
    MissingEvidence,
    BadEvidence,
    BadAttestation,
}

impl PkgVerifyClosureStatus {
    fn term(self) -> Term {
        Term::symbol(match self {
            Self::Ok => ":ok",
            Self::Missing => ":missing",
            Self::Corrupt => ":corrupt",
            Self::BadCommit => ":bad-commit",
            Self::SnapshotMismatch => ":snapshot-mismatch",
            Self::MissingEvidence => ":missing-evidence",
            Self::BadEvidence => ":bad-evidence",
            Self::BadAttestation => ":bad-attestation",
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PkgVerifyClosureObservation {
    pub(crate) checked: u64,
    pub(crate) detail: Option<String>,
    pub(crate) hash: Option<String>,
    pub(crate) status: PkgVerifyClosureStatus,
}

#[derive(Debug, Clone)]
pub(crate) struct PkgVerifyObservation {
    pub(crate) closure: Option<PkgVerifyClosureObservation>,
    pub(crate) detail: Option<String>,
    pub(crate) hashes: Vec<PkgVerifyHashObservation>,
    pub(crate) name: String,
    pub(crate) snapshot_status: PkgVerifySnapshotStatus,
}

#[derive(Debug)]
pub(crate) enum PkgVerifyFinalized {
    Report(Term),
    Error { code: String, message: String },
}

impl PkgResolutionIdentityAuthority {
    pub(crate) fn plan_verify(&mut self, model: Term) -> Result<PkgVerifyPlan, EffectsError> {
        let retained_model = model.clone();
        let request = map([
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":model", model),
            (":op", Term::symbol(":plan")),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .verify_authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("verify plan apply failed: {error}")))?;
        decode_plan_result(
            plain_result(value, &self.context)?,
            request_hash,
            retained_model,
        )
    }

    pub(crate) fn finalize_verify(
        &mut self,
        plan: &PkgVerifyPlan,
        observations: &[PkgVerifyObservation],
    ) -> Result<PkgVerifyFinalized, EffectsError> {
        let request = map([
            (":kind", Term::Str(REQUEST_KIND.to_string())),
            (":model", plan.model.clone()),
            (":observations", observations_term(observations)),
            (":op", Term::symbol(":finalize")),
            (":plan", plan.term.clone()),
            (":plan-h", Term::Str(plan.hash.clone())),
            (":v", Term::Int(1.into())),
        ]);
        let request_hash = hash_term(&request);
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .verify_authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("verify finalize apply failed: {error}")))?;
        decode_finalize_result(plain_result(value, &self.context)?, request_hash)
    }
}

fn observations_term(observations: &[PkgVerifyObservation]) -> Term {
    Term::Vector(
        observations
            .iter()
            .map(|observation| {
                map([
                    (
                        ":closure",
                        observation
                            .closure
                            .as_ref()
                            .map(closure_observation_term)
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        ":detail",
                        observation
                            .detail
                            .clone()
                            .map(Term::Str)
                            .unwrap_or(Term::Nil),
                    ),
                    (
                        ":hashes",
                        Term::Vector(
                            observation
                                .hashes
                                .iter()
                                .map(|hash| {
                                    map([
                                        (":h", Term::Str(hash.hash.clone())),
                                        (":status", hash.status.term()),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                    (":name", Term::Str(observation.name.clone())),
                    (":snapshot-status", observation.snapshot_status.term()),
                ])
            })
            .collect(),
    )
}

fn closure_observation_term(observation: &PkgVerifyClosureObservation) -> Term {
    map([
        (":checked", Term::Int(observation.checked.into())),
        (
            ":detail",
            observation
                .detail
                .clone()
                .map(Term::Str)
                .unwrap_or(Term::Nil),
        ),
        (
            ":hash",
            observation.hash.clone().map(Term::Str).unwrap_or(Term::Nil),
        ),
        (":status", observation.status.term()),
    ])
}

fn decode_envelope(
    term: &Term,
    request_hash: [u8; 32],
) -> Result<Result<&Term, (String, String)>, EffectsError> {
    let fields = exact_map(
        term,
        &[
            ":code",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
            ":value",
        ],
    )?;
    require_string(fields, ":kind", RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if required_bool(fields, ":ok")? {
        require_nil(fields, ":code")?;
        require_nil(fields, ":message")?;
        Ok(Ok(field(fields, ":value")?))
    } else {
        require_nil(fields, ":value")?;
        let code = required_string(fields, ":code")?.to_string();
        if code != "core/pkg/bad-authority-request" {
            return Err(authority_error(
                "verify result :code is outside the closed rejection inventory",
            ));
        }
        Ok(Err((
            code,
            required_string(fields, ":message")?.to_string(),
        )))
    }
}

fn decode_plan_result(
    term: Term,
    request_hash: [u8; 32],
    model: Term,
) -> Result<PkgVerifyPlan, EffectsError> {
    let value = decode_envelope(&term, request_hash)?.map_err(|(code, message)| {
        authority_error(format!("verify authority rejected plan: {code}: {message}"))
    })?;
    let fields = exact_map(value, &[":plan", ":plan-h"])?;
    let plan_term = field(fields, ":plan")?.clone();
    let plan_hash = required_string(fields, ":plan-h")?.to_string();
    if !is_hash(&plan_hash) || hex32(hash_term(&plan_term)) != plan_hash {
        return Err(authority_error("verify plan term and :plan-h contradict"));
    }
    let plan_fields = exact_map(&plan_term, &[":dependencies", ":lock"])?;
    required_string(plan_fields, ":lock")?;
    let Term::Vector(step_terms) = field(plan_fields, ":dependencies")? else {
        return Err(authority_error("verify plan :dependencies must be vector"));
    };
    let steps = step_terms
        .iter()
        .map(decode_step)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PkgVerifyPlan {
        hash: plan_hash,
        model,
        steps,
        term: plan_term,
    })
}

fn decode_step(term: &Term) -> Result<PkgVerifyStep, EffectsError> {
    let fields = exact_map(term, &[":commit", ":name", ":snapshot"])?;
    let snapshot = required_string(fields, ":snapshot")?.to_string();
    if !is_hash(&snapshot) {
        return Err(authority_error("verify step :snapshot must be hash"));
    }
    Ok(PkgVerifyStep {
        commit: optional_hash(fields, ":commit")?,
        name: required_string(fields, ":name")?.to_string(),
        snapshot,
    })
}

fn decode_finalize_result(
    term: Term,
    request_hash: [u8; 32],
) -> Result<PkgVerifyFinalized, EffectsError> {
    let value = decode_envelope(&term, request_hash)?.map_err(|(code, message)| {
        authority_error(format!(
            "verify authority rejected finalize: {code}: {message}"
        ))
    })?;
    let fields = exact_map(value, &[":code", ":decision", ":message", ":report"])?;
    match required_symbol(fields, ":decision")? {
        ":report" => {
            require_nil(fields, ":code")?;
            require_nil(fields, ":message")?;
            let report = field(fields, ":report")?;
            let report_fields = exact_map(report, &[":checked", ":lock", ":missing", ":ok"])?;
            required_u64(report_fields, ":checked")?;
            required_string(report_fields, ":lock")?;
            let missing = string_vector(report_fields, ":missing", true)?;
            let ok = required_bool(report_fields, ":ok")?;
            if ok != missing.is_empty() {
                return Err(authority_error(
                    "verify report :ok must match whether :missing is empty",
                ));
            }
            Ok(PkgVerifyFinalized::Report(report.clone()))
        }
        ":error" => {
            require_nil(fields, ":report")?;
            let code = required_string(fields, ":code")?.to_string();
            if !matches!(
                code.as_str(),
                "core/store/not-found"
                    | "core/store/corruption"
                    | "core/pkg/bad-snapshot"
                    | "core/pkg/bad-commit"
                    | "core/pkg/commit-snapshot-mismatch"
                    | "core/pkg/missing-evidence"
                    | "core/pkg/bad-evidence"
                    | "core/pkg/bad-attestation"
            ) {
                return Err(authority_error(
                    "verify finalize :code is outside the closed error inventory",
                ));
            }
            Ok(PkgVerifyFinalized::Error {
                code,
                message: required_string(fields, ":message")?.to_string(),
            })
        }
        _ => Err(authority_error(
            "verify finalize :decision must be :report or :error",
        )),
    }
}

fn required_symbol<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, EffectsError> {
    match field(fields, name)? {
        Term::Symbol(value) => Ok(value),
        _ => Err(authority_error(format!("verify {name} must be symbol"))),
    }
}

fn required_u64(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<u64, EffectsError> {
    match field(fields, name)? {
        Term::Int(value) => value
            .to_u64()
            .ok_or_else(|| authority_error(format!("verify {name} must be u64"))),
        _ => Err(authority_error(format!("verify {name} must be integer"))),
    }
}

fn optional_hash(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, EffectsError> {
    match field(fields, name)? {
        Term::Nil => Ok(None),
        Term::Str(value) if is_hash(value) => Ok(Some(value.clone())),
        _ => Err(authority_error(format!(
            "verify {name} must be lowercase hash or nil"
        ))),
    }
}

fn string_vector(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    hashes: bool,
) -> Result<Vec<String>, EffectsError> {
    let Term::Vector(values) = field(fields, name)? else {
        return Err(authority_error(format!("verify {name} must be vector")));
    };
    values
        .iter()
        .map(|value| match value {
            Term::Str(value) if !hashes || is_hash(value) => Ok(value.clone()),
            _ => Err(authority_error(format!(
                "verify {name} entries must be{} strings",
                if hashes { " lowercase hash" } else { "" }
            ))),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact_config() -> SelfhostAuthorityConfig {
        let artifact = std::env::var_os("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/toolchain.gc")
            })
            .canonicalize()
            .expect("canonical selfhost artifact path");
        SelfhostAuthorityConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        }
    }

    fn locked(snapshot: char, commit: Option<char>) -> gc_pkg::LockedEntry {
        let snapshot = snapshot.to_string().repeat(64);
        gc_pkg::LockedEntry {
            commit: commit.map(|value| value.to_string().repeat(64)),
            snapshot: snapshot.clone(),
            registry: None,
            source_selector: format!("snapshot:{snapshot}"),
            resolved_ref: None,
            exports_hash: None,
            environment_fingerprint: None,
        }
    }

    fn model() -> gc_pkg::GenesisLock {
        let mut locked_entries = BTreeMap::new();
        locked_entries.insert("alpha".to_string(), locked('a', None));
        locked_entries.insert("beta".to_string(), locked('b', Some('c')));
        gc_pkg::GenesisLock {
            version: 2,
            workspace: "workspace".to_string(),
            policy: "policy:default-v0.1".to_string(),
            registries: BTreeMap::new(),
            requirements: BTreeMap::new(),
            locked: locked_entries,
            artifacts: BTreeMap::new(),
        }
    }

    #[test]
    fn authority_plans_ordered_dependencies_and_constructs_exact_report() {
        let model = model();
        let model_term =
            crate::pkg_lock_write_authority::lock_model_payload("genesis.lock", &model).unwrap();
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let plan = authority.plan_verify(model_term).unwrap();
        assert_eq!(
            plan.steps
                .iter()
                .map(|step| step.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
        let observations = vec![
            PkgVerifyObservation {
                closure: None,
                detail: None,
                hashes: Vec::new(),
                name: "alpha".to_string(),
                snapshot_status: PkgVerifySnapshotStatus::Missing,
            },
            PkgVerifyObservation {
                closure: Some(PkgVerifyClosureObservation {
                    checked: 3,
                    detail: None,
                    hash: None,
                    status: PkgVerifyClosureStatus::Ok,
                }),
                detail: None,
                hashes: vec![PkgVerifyHashObservation {
                    hash: "b".repeat(64),
                    status: PkgVerifyHashStatus::Present,
                }],
                name: "beta".to_string(),
                snapshot_status: PkgVerifySnapshotStatus::Available,
            },
        ];
        let PkgVerifyFinalized::Report(report) =
            authority.finalize_verify(&plan, &observations).unwrap()
        else {
            panic!("valid observations did not produce report");
        };
        let fields = exact_map(&report, &[":checked", ":lock", ":missing", ":ok"]).unwrap();
        assert_eq!(field(fields, ":checked").unwrap(), &Term::Int(4.into()));
        assert_eq!(field(fields, ":ok").unwrap(), &Term::Bool(false));
        assert_eq!(
            field(fields, ":missing").unwrap(),
            &Term::Vector(vec![Term::Str("a".repeat(64))])
        );
    }

    #[test]
    fn authority_owns_terminal_error_and_rejects_trailing_or_substituted_facts() {
        let model = model();
        let model_term =
            crate::pkg_lock_write_authority::lock_model_payload("genesis.lock", &model).unwrap();
        let mut authority = PkgResolutionIdentityAuthority::load(&artifact_config()).unwrap();
        let plan = authority.plan_verify(model_term).unwrap();
        let terminal = PkgVerifyObservation {
            closure: None,
            detail: None,
            hashes: vec![PkgVerifyHashObservation {
                hash: "0".repeat(64),
                status: PkgVerifyHashStatus::Corrupt,
            }],
            name: "alpha".to_string(),
            snapshot_status: PkgVerifySnapshotStatus::Available,
        };
        let PkgVerifyFinalized::Error { code, message } = authority
            .finalize_verify(&plan, std::slice::from_ref(&terminal))
            .unwrap()
        else {
            panic!("terminal corruption did not produce error");
        };
        assert_eq!(code, "core/store/corruption");
        assert_eq!(
            message,
            format!("artifact store corruption: {}", "0".repeat(64))
        );

        let mut substituted = terminal.clone();
        substituted.name = "beta".to_string();
        assert!(authority.finalize_verify(&plan, &[substituted]).is_err());
        assert!(
            authority
                .finalize_verify(&plan, &[terminal.clone(), terminal])
                .is_err()
        );

        let contradictory = vec![
            PkgVerifyObservation {
                closure: None,
                detail: None,
                hashes: Vec::new(),
                name: "alpha".to_string(),
                snapshot_status: PkgVerifySnapshotStatus::Missing,
            },
            PkgVerifyObservation {
                closure: Some(PkgVerifyClosureObservation {
                    checked: 1,
                    detail: Some("unexpected detail".to_string()),
                    hash: None,
                    status: PkgVerifyClosureStatus::Ok,
                }),
                detail: None,
                hashes: vec![PkgVerifyHashObservation {
                    hash: "b".repeat(64),
                    status: PkgVerifyHashStatus::Present,
                }],
                name: "beta".to_string(),
                snapshot_status: PkgVerifySnapshotStatus::Available,
            },
        ];
        assert!(authority.finalize_verify(&plan, &contradictory).is_err());

        let mut zero_checked = contradictory;
        let Some(closure) = zero_checked[1].closure.as_mut() else {
            panic!("test closure missing");
        };
        closure.checked = 0;
        closure.detail = None;
        assert!(authority.finalize_verify(&plan, &zero_checked).is_err());
    }

    #[test]
    fn decoder_rejects_report_success_that_contradicts_missing_inventory() {
        let request_hash = [7; 32];
        let report = map([
            (":checked", Term::Int(0.into())),
            (":lock", Term::Str("a".repeat(64))),
            (":missing", Term::Vector(vec![Term::Str("b".repeat(64))])),
            (":ok", Term::Bool(true)),
        ]);
        let result = map([
            (":code", Term::Nil),
            (":kind", Term::Str(RESULT_KIND.to_string())),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":request-h", Term::Str(hex32(request_hash))),
            (":v", Term::Int(1.into())),
            (
                ":value",
                map([
                    (":code", Term::Nil),
                    (":decision", Term::symbol(":report")),
                    (":message", Term::Nil),
                    (":report", report),
                ]),
            ),
        ]);
        assert!(decode_finalize_result(result, request_hash).is_err());
    }
}
