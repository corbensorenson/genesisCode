use gc_coreform::{Term, TermOrdKey, hash_term};

#[path = "support_policy_readers.rs"]
mod readers;
use readers::{READER_RECORDS, ReaderRecord};

pub const SUPPORT_POLICY_PROFILE_ID: &str = "genesis/support-policy-v0.1";
pub const CURRENT_RELEASE_TRAIN: &str = "0.2.0";
pub const SOURCE_API_DEPRECATION_MIN_DAYS: u32 = 365;
pub const SOURCE_API_DEPRECATION_MIN_MINOR_RELEASES: u16 = 2;
pub const STABLE_READER_RETIREMENT_MIN_DAYS: u32 = 730;
pub const STANDARD_SUPPORT_MIN_DAYS: u32 = 548;
pub const MAINTENANCE_SUPPORT_MIN_DAYS: u32 = 365;
pub const SECURITY_ONLY_SUPPORT_MIN_DAYS: u32 = 365;
pub const RELEASE_LINE_EOL_MIN_DAYS: u32 = 1_278;
pub const FINAL_MAJOR_SUCCESSOR_SUPPORT_MIN_DAYS: u32 = 730;
pub const SECURITY_EXCEPTION_MAX_DAYS: u16 = 90;
pub const SUPPORT_FIELD_MAX_BYTES: usize = 4_096;
pub const SUPPORT_EVIDENCE_MAX_REFS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseSupportPhase {
    Preview,
    Standard,
    Maintenance,
    SecurityOnly,
    EndOfLife,
}

impl ReleaseSupportPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Standard => "standard",
            Self::Maintenance => "maintenance",
            Self::SecurityOnly => "security-only",
            Self::EndOfLife => "end-of-life",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderSupportMode {
    Current,
    Legacy,
}

impl ReaderSupportMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Legacy => "legacy-read-only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportSnapshot {
    pub release_train: &'static str,
    pub release_phase: ReleaseSupportPhase,
    pub v1_stable_active: bool,
    pub active_deprecations: usize,
    pub active_security_exceptions: usize,
}

pub fn current_support_snapshot() -> SupportSnapshot {
    SupportSnapshot {
        release_train: CURRENT_RELEASE_TRAIN,
        release_phase: ReleaseSupportPhase::Preview,
        v1_stable_active: false,
        active_deprecations: 0,
        active_security_exceptions: 0,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderSupportDecision {
    pub compatibility_id: String,
    pub component: String,
    pub reader: String,
    pub current_writer: String,
    pub mode: ReaderSupportMode,
    pub migration_record: Option<String>,
    pub release_phase: ReleaseSupportPhase,
    pub v1_stable_active: bool,
    pub decision_identity: [u8; 32],
}

impl ReaderSupportDecision {
    pub fn to_term(&self) -> Term {
        map([
            (
                ":compatibility-id",
                Term::Str(self.compatibility_id.clone()),
            ),
            (":component", Term::Str(self.component.clone())),
            (
                ":decision-identity",
                Term::Bytes(self.decision_identity.to_vec().into()),
            ),
            (":kind", Term::symbol(SUPPORT_POLICY_PROFILE_ID)),
            (
                ":migration-record",
                self.migration_record
                    .as_ref()
                    .map(|value| Term::Str(value.clone()))
                    .unwrap_or(Term::Nil),
            ),
            (":mode", Term::symbol(self.mode.as_str())),
            (":reader", Term::Str(self.reader.clone())),
            (":release-phase", Term::symbol(self.release_phase.as_str())),
            (":v1-stable-active", Term::Bool(self.v1_stable_active)),
            (":writer", Term::Str(self.current_writer.clone())),
        ])
    }
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SupportPolicyError {
    #[error("support-policy field {field} must be non-empty")]
    EmptyField { field: &'static str },
    #[error("support-policy field {field} exceeds {limit} bytes")]
    FieldTooLong { field: &'static str, limit: usize },
    #[error("support-policy field {field} is not a portable identifier")]
    InvalidIdentifier { field: &'static str },
    #[error("unknown compatibility component")]
    UnknownCompatibilityComponent,
    #[error("unsupported or future reader identity")]
    UnsupportedReader,
    #[error("support evidence exceeds {limit} references")]
    TooManyEvidenceReferences { limit: usize },
    #[error("support evidence references must be strict lexical unique order")]
    NonCanonicalEvidenceReferences,
    #[error("support-policy prerequisite missing: {0}")]
    MissingPrerequisite(&'static str),
    #[error("support-policy window is not complete: {0}")]
    WindowIncomplete(&'static str),
    #[error("support lifecycle transition is not the next ordered phase")]
    InvalidLifecycleTransition,
    #[error("security exception may not bypass a protected invariant")]
    ProtectedInvariantBypass,
    #[error("security exception duration must be between 1 and {limit} days")]
    InvalidExceptionDuration { limit: u16 },
}

pub fn query_reader_support(
    compatibility_id: &str,
    component: &str,
    reader_identity: &str,
) -> Result<ReaderSupportDecision, SupportPolicyError> {
    validate_identifier("compatibility-id", compatibility_id)?;
    validate_identifier("component", component)?;
    validate_identifier("reader", reader_identity)?;

    let component_exists = READER_RECORDS
        .iter()
        .any(|record| record.compatibility_id == compatibility_id && record.component == component);
    if !component_exists {
        return Err(SupportPolicyError::UnknownCompatibilityComponent);
    }
    let record = READER_RECORDS
        .iter()
        .find(|record| {
            record.compatibility_id == compatibility_id
                && record.component == component
                && record.reader == reader_identity
        })
        .ok_or(SupportPolicyError::UnsupportedReader)?;

    let snapshot = current_support_snapshot();
    let payload = reader_decision_payload(record, &snapshot);
    Ok(ReaderSupportDecision {
        compatibility_id: compatibility_id.to_string(),
        component: component.to_string(),
        reader: reader_identity.to_string(),
        current_writer: record.current_writer.to_string(),
        mode: record.mode,
        migration_record: record.migration_record.map(str::to_string),
        release_phase: snapshot.release_phase,
        v1_stable_active: snapshot.v1_stable_active,
        decision_identity: hash_term(&payload),
    })
}

#[derive(Debug, Clone)]
pub struct LifecycleTransitionRequest<'a> {
    pub release_identity: &'a str,
    pub from: ReleaseSupportPhase,
    pub to: ReleaseSupportPhase,
    pub elapsed_phase_days: u32,
    pub elapsed_since_ga_days: u32,
    pub v1_activation_evidence: Option<&'a str>,
    pub final_major_line: bool,
    pub successor_major_policy: Option<&'a str>,
    pub days_since_successor_major: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleTransitionEligibility {
    pub from: ReleaseSupportPhase,
    pub to: ReleaseSupportPhase,
    pub eligible_for_review: bool,
    pub grants_transition_authority: bool,
    pub decision_identity: [u8; 32],
}

pub fn validate_lifecycle_transition(
    request: &LifecycleTransitionRequest<'_>,
) -> Result<LifecycleTransitionEligibility, SupportPolicyError> {
    validate_identifier("release-identity", request.release_identity)?;
    let minimum_phase_days = match (request.from, request.to) {
        (ReleaseSupportPhase::Preview, ReleaseSupportPhase::Standard) => {
            require_reference("v1-activation-evidence", request.v1_activation_evidence)?;
            0
        }
        (ReleaseSupportPhase::Standard, ReleaseSupportPhase::Maintenance) => {
            STANDARD_SUPPORT_MIN_DAYS
        }
        (ReleaseSupportPhase::Maintenance, ReleaseSupportPhase::SecurityOnly) => {
            MAINTENANCE_SUPPORT_MIN_DAYS
        }
        (ReleaseSupportPhase::SecurityOnly, ReleaseSupportPhase::EndOfLife) => {
            SECURITY_ONLY_SUPPORT_MIN_DAYS
        }
        _ => return Err(SupportPolicyError::InvalidLifecycleTransition),
    };
    if request.elapsed_phase_days < minimum_phase_days {
        return Err(SupportPolicyError::WindowIncomplete(
            "current lifecycle phase days",
        ));
    }
    if request.to == ReleaseSupportPhase::EndOfLife {
        if request.elapsed_since_ga_days < RELEASE_LINE_EOL_MIN_DAYS {
            return Err(SupportPolicyError::WindowIncomplete(
                "release-line lifecycle days",
            ));
        }
        if request.final_major_line {
            require_reference("successor-major-policy", request.successor_major_policy)?;
            if request.days_since_successor_major < FINAL_MAJOR_SUCCESSOR_SUPPORT_MIN_DAYS {
                return Err(SupportPolicyError::WindowIncomplete(
                    "final major line successor support days",
                ));
            }
        }
    }

    let payload = lifecycle_transition_payload(request);
    Ok(LifecycleTransitionEligibility {
        from: request.from,
        to: request.to,
        eligible_for_review: true,
        grants_transition_authority: false,
        decision_identity: hash_term(&payload),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalClass {
    SourceOrApi,
    StableFormatReader,
    ReleaseLineEndOfLife,
}

impl RemovalClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::SourceOrApi => "source-or-api",
            Self::StableFormatReader => "stable-format-reader",
            Self::ReleaseLineEndOfLife => "release-line-end-of-life",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RemovalEvidence<'a> {
    pub identifier: &'a str,
    pub replacement: &'a str,
    pub migration: &'a str,
    pub rollback: &'a str,
    pub announced_release: &'a str,
    pub elapsed_days: u32,
    pub subsequent_minor_releases: u16,
    pub successor_major_policy: Option<&'a str>,
    pub corpus_or_telemetry_evidence: Option<&'a str>,
    pub golden_corpus: Option<&'a str>,
    pub final_major_line: bool,
    pub days_since_successor_major: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalEligibility {
    pub class: RemovalClass,
    pub eligible_for_review: bool,
    pub grants_removal_authority: bool,
    pub decision_identity: [u8; 32],
}

pub fn validate_removal_eligibility(
    class: RemovalClass,
    evidence: &RemovalEvidence<'_>,
) -> Result<RemovalEligibility, SupportPolicyError> {
    validate_identifier("identifier", evidence.identifier)?;
    validate_identifier("replacement", evidence.replacement)?;
    validate_reference("migration", evidence.migration)?;
    validate_reference("rollback", evidence.rollback)?;
    validate_identifier("announced-release", evidence.announced_release)?;

    match class {
        RemovalClass::SourceOrApi => {
            if evidence.elapsed_days < SOURCE_API_DEPRECATION_MIN_DAYS {
                return Err(SupportPolicyError::WindowIncomplete(
                    "source/API elapsed days",
                ));
            }
            if evidence.subsequent_minor_releases < SOURCE_API_DEPRECATION_MIN_MINOR_RELEASES {
                return Err(SupportPolicyError::WindowIncomplete(
                    "source/API subsequent minor releases",
                ));
            }
        }
        RemovalClass::StableFormatReader => {
            require_reference("successor-major-policy", evidence.successor_major_policy)?;
            require_reference(
                "corpus-or-telemetry-evidence",
                evidence.corpus_or_telemetry_evidence,
            )?;
            require_reference("golden-corpus", evidence.golden_corpus)?;
            if evidence.elapsed_days < STABLE_READER_RETIREMENT_MIN_DAYS {
                return Err(SupportPolicyError::WindowIncomplete(
                    "stable reader deprecation days",
                ));
            }
        }
        RemovalClass::ReleaseLineEndOfLife => {
            if evidence.elapsed_days < RELEASE_LINE_EOL_MIN_DAYS {
                return Err(SupportPolicyError::WindowIncomplete(
                    "release-line lifecycle days",
                ));
            }
            if evidence.final_major_line {
                require_reference("successor-major-policy", evidence.successor_major_policy)?;
                if evidence.days_since_successor_major < FINAL_MAJOR_SUCCESSOR_SUPPORT_MIN_DAYS {
                    return Err(SupportPolicyError::WindowIncomplete(
                        "final major line successor support days",
                    ));
                }
            }
        }
    }

    let payload = removal_payload(class, evidence);
    Ok(RemovalEligibility {
        class,
        eligible_for_review: true,
        grants_removal_authority: false,
        decision_identity: hash_term(&payload),
    })
}

#[derive(Debug, Clone)]
pub struct SecurityExceptionRequest<'a> {
    pub exception_id: &'a str,
    pub advisory: &'a str,
    pub scope: &'a str,
    pub rationale: &'a str,
    pub replacement_identity: Option<&'a str>,
    pub migrator: Option<&'a str>,
    pub rollback: &'a str,
    pub test_evidence: &'a [&'a str],
    pub duration_days: u16,
    pub changes_wire_or_semantics: bool,
    pub bypasses_protected_invariant: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityExceptionEligibility {
    pub eligible_for_review: bool,
    pub grants_exception_authority: bool,
    pub decision_identity: [u8; 32],
}

pub fn validate_security_exception(
    request: &SecurityExceptionRequest<'_>,
) -> Result<SecurityExceptionEligibility, SupportPolicyError> {
    validate_identifier("exception-id", request.exception_id)?;
    validate_reference("advisory", request.advisory)?;
    validate_reference("scope", request.scope)?;
    validate_reference("rationale", request.rationale)?;
    validate_reference("rollback", request.rollback)?;
    if request.duration_days == 0 || request.duration_days > SECURITY_EXCEPTION_MAX_DAYS {
        return Err(SupportPolicyError::InvalidExceptionDuration {
            limit: SECURITY_EXCEPTION_MAX_DAYS,
        });
    }
    if request.bypasses_protected_invariant {
        return Err(SupportPolicyError::ProtectedInvariantBypass);
    }
    if request.test_evidence.is_empty() {
        return Err(SupportPolicyError::MissingPrerequisite("test-evidence"));
    }
    if request.test_evidence.len() > SUPPORT_EVIDENCE_MAX_REFS {
        return Err(SupportPolicyError::TooManyEvidenceReferences {
            limit: SUPPORT_EVIDENCE_MAX_REFS,
        });
    }
    if request
        .test_evidence
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return Err(SupportPolicyError::NonCanonicalEvidenceReferences);
    }
    for item in request.test_evidence {
        validate_reference("test-evidence", item)?;
    }
    if request.changes_wire_or_semantics {
        require_reference("replacement-identity", request.replacement_identity)?;
        require_reference("migrator", request.migrator)?;
    } else {
        validate_optional_reference("replacement-identity", request.replacement_identity)?;
        validate_optional_reference("migrator", request.migrator)?;
    }

    let payload = security_exception_payload(request);
    Ok(SecurityExceptionEligibility {
        eligible_for_review: true,
        grants_exception_authority: false,
        decision_identity: hash_term(&payload),
    })
}

fn reader_decision_payload(record: &ReaderRecord, snapshot: &SupportSnapshot) -> Term {
    map([
        (
            ":compatibility-id",
            Term::Str(record.compatibility_id.to_string()),
        ),
        (":component", Term::Str(record.component.to_string())),
        (":kind", Term::symbol(SUPPORT_POLICY_PROFILE_ID)),
        (
            ":migration-record",
            record
                .migration_record
                .map(|value| Term::Str(value.to_string()))
                .unwrap_or(Term::Nil),
        ),
        (":mode", Term::symbol(record.mode.as_str())),
        (":reader", Term::Str(record.reader.to_string())),
        (
            ":release-phase",
            Term::symbol(snapshot.release_phase.as_str()),
        ),
        (":v1-stable-active", Term::Bool(snapshot.v1_stable_active)),
        (":writer", Term::Str(record.current_writer.to_string())),
    ])
}

fn lifecycle_transition_payload(request: &LifecycleTransitionRequest<'_>) -> Term {
    map([
        (
            ":days-since-successor-major",
            Term::Int(request.days_since_successor_major.into()),
        ),
        (
            ":elapsed-phase-days",
            Term::Int(request.elapsed_phase_days.into()),
        ),
        (
            ":elapsed-since-ga-days",
            Term::Int(request.elapsed_since_ga_days.into()),
        ),
        (":final-major-line", Term::Bool(request.final_major_line)),
        (":from", Term::symbol(request.from.as_str())),
        (":kind", Term::symbol(SUPPORT_POLICY_PROFILE_ID)),
        (
            ":release-identity",
            Term::Str(request.release_identity.to_string()),
        ),
        (
            ":successor-major-policy",
            optional_str_term(request.successor_major_policy),
        ),
        (":to", Term::symbol(request.to.as_str())),
        (
            ":v1-activation-evidence",
            optional_str_term(request.v1_activation_evidence),
        ),
    ])
}

fn removal_payload(class: RemovalClass, evidence: &RemovalEvidence<'_>) -> Term {
    map([
        (
            ":announced-release",
            Term::Str(evidence.announced_release.to_string()),
        ),
        (":class", Term::symbol(class.as_str())),
        (
            ":corpus-or-telemetry",
            optional_str_term(evidence.corpus_or_telemetry_evidence),
        ),
        (
            ":days-since-successor-major",
            Term::Int(evidence.days_since_successor_major.into()),
        ),
        (":elapsed-days", Term::Int(evidence.elapsed_days.into())),
        (":final-major-line", Term::Bool(evidence.final_major_line)),
        (":golden-corpus", optional_str_term(evidence.golden_corpus)),
        (":identifier", Term::Str(evidence.identifier.to_string())),
        (":kind", Term::symbol(SUPPORT_POLICY_PROFILE_ID)),
        (":migration", Term::Str(evidence.migration.to_string())),
        (":replacement", Term::Str(evidence.replacement.to_string())),
        (":rollback", Term::Str(evidence.rollback.to_string())),
        (
            ":subsequent-minor-releases",
            Term::Int(evidence.subsequent_minor_releases.into()),
        ),
        (
            ":successor-major-policy",
            optional_str_term(evidence.successor_major_policy),
        ),
    ])
}

fn security_exception_payload(request: &SecurityExceptionRequest<'_>) -> Term {
    map([
        (":advisory", Term::Str(request.advisory.to_string())),
        (
            ":changes-wire-or-semantics",
            Term::Bool(request.changes_wire_or_semantics),
        ),
        (":duration-days", Term::Int(request.duration_days.into())),
        (":exception-id", Term::Str(request.exception_id.to_string())),
        (":kind", Term::symbol(SUPPORT_POLICY_PROFILE_ID)),
        (":migrator", optional_str_term(request.migrator)),
        (":rationale", Term::Str(request.rationale.to_string())),
        (
            ":replacement-identity",
            optional_str_term(request.replacement_identity),
        ),
        (":rollback", Term::Str(request.rollback.to_string())),
        (":scope", Term::Str(request.scope.to_string())),
        (
            ":test-evidence",
            Term::Vector(
                request
                    .test_evidence
                    .iter()
                    .map(|item| Term::Str((*item).to_string()))
                    .collect(),
            ),
        ),
    ])
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), SupportPolicyError> {
    validate_reference(field, value)?;
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b':' | b'=')
    }) {
        return Err(SupportPolicyError::InvalidIdentifier { field });
    }
    Ok(())
}

fn validate_reference(field: &'static str, value: &str) -> Result<(), SupportPolicyError> {
    if value.is_empty() {
        return Err(SupportPolicyError::EmptyField { field });
    }
    if value.len() > SUPPORT_FIELD_MAX_BYTES {
        return Err(SupportPolicyError::FieldTooLong {
            field,
            limit: SUPPORT_FIELD_MAX_BYTES,
        });
    }
    Ok(())
}

fn validate_optional_reference(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), SupportPolicyError> {
    if let Some(value) = value {
        validate_reference(field, value)?;
    }
    Ok(())
}

fn require_reference(field: &'static str, value: Option<&str>) -> Result<(), SupportPolicyError> {
    let value = value.ok_or(SupportPolicyError::MissingPrerequisite(field))?;
    validate_reference(field, value)
}

fn optional_str_term(value: Option<&str>) -> Term {
    value
        .map(|value| Term::Str(value.to_string()))
        .unwrap_or(Term::Nil)
}

fn map<const N: usize>(entries: [(&str, Term); N]) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value))
            .collect(),
    )
}
