use std::collections::BTreeSet;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey};
use gc_types::support_policy::{
    CURRENT_RELEASE_TRAIN, FINAL_MAJOR_SUCCESSOR_SUPPORT_MIN_DAYS, LifecycleTransitionRequest,
    MAINTENANCE_SUPPORT_MIN_DAYS, RELEASE_LINE_EOL_MIN_DAYS, ReaderSupportMode,
    ReleaseSupportPhase, RemovalClass, RemovalEvidence, SECURITY_EXCEPTION_MAX_DAYS,
    SECURITY_ONLY_SUPPORT_MIN_DAYS, SOURCE_API_DEPRECATION_MIN_DAYS,
    SOURCE_API_DEPRECATION_MIN_MINOR_RELEASES, STABLE_READER_RETIREMENT_MIN_DAYS,
    STANDARD_SUPPORT_MIN_DAYS, SUPPORT_EVIDENCE_MAX_REFS, SUPPORT_FIELD_MAX_BYTES,
    SUPPORT_POLICY_PROFILE_ID, SecurityExceptionRequest, SupportPolicyError,
    current_support_snapshot, query_reader_support, validate_lifecycle_transition,
    validate_removal_eligibility, validate_security_exception,
};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

const PROFILE: &str = include_str!("../../../docs/spec/SUPPORT_POLICY_v0.1.json");
const SPEC: &[u8] = include_bytes!("../../../docs/spec/SUPPORT_POLICY_v0.1.md");
const SCHEMA: &[u8] = include_bytes!("../../../docs/spec/SUPPORT_POLICY_v0.1.schema.json");
const COMPATIBILITY: &str = include_str!("../../../genesis.compatibility.json");
const VERSION_SURFACES: &str = include_str!("../../../genesis.version-surfaces.json");

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn content_identity(value: &JsonValue) -> String {
    let mut payload = value.clone();
    payload
        .as_object_mut()
        .expect("profile object")
        .remove("contentIdentitySha256");
    sha256_hex(
        serde_json::to_string(&payload)
            .expect("canonical profile JSON")
            .as_bytes(),
    )
}

fn profile() -> JsonValue {
    serde_json::from_str(PROFILE).expect("valid support profile")
}

fn base_removal() -> RemovalEvidence<'static> {
    RemovalEvidence {
        identifier: "genesis/api/old",
        replacement: "genesis/api/new",
        migration: "docs/migrate-old-to-new",
        rollback: "docs/rollback-new-to-old",
        announced_release: "genesis/release/v1.1.0",
        elapsed_days: SOURCE_API_DEPRECATION_MIN_DAYS,
        subsequent_minor_releases: SOURCE_API_DEPRECATION_MIN_MINOR_RELEASES,
        successor_major_policy: None,
        corpus_or_telemetry_evidence: None,
        golden_corpus: None,
        final_major_line: false,
        days_since_successor_major: 0,
    }
}

fn base_exception() -> SecurityExceptionRequest<'static> {
    SecurityExceptionRequest {
        exception_id: "genesis/security-exception/SA-0001",
        advisory: "security/advisories/SA-0001",
        scope: "genesis/compat/v1/effect-log:gclog:2",
        rationale: "temporary quarantine while authenticated migration completes",
        replacement_identity: None,
        migrator: None,
        rollback: "security/rollback/SA-0001",
        test_evidence: &["tests/security/SA-0001"],
        duration_days: SECURITY_EXCEPTION_MAX_DAYS,
        changes_wire_or_semantics: false,
        bypasses_protected_invariant: false,
    }
}

#[test]
fn machine_profile_is_closed_content_addressed_and_source_bound() {
    let profile = profile();
    let keys = profile
        .as_object()
        .expect("profile object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "applicability",
            "auditDate",
            "canonicalSpec",
            "canonicalSpecSha256",
            "contentIdentitySha256",
            "deprecation",
            "endOfLife",
            "failure",
            "identity",
            "kind",
            "lifecycle",
            "nonclaims",
            "readers",
            "resourceLimits",
            "schema",
            "schemaSha256",
            "securityExceptions",
            "sourceBindings",
            "version",
        ]
    );
    assert_eq!(profile["kind"], SUPPORT_POLICY_PROFILE_ID);
    assert_eq!(profile["canonicalSpecSha256"], sha256_hex(SPEC));
    assert_eq!(profile["schemaSha256"], sha256_hex(SCHEMA));
    assert_eq!(profile["contentIdentitySha256"], content_identity(&profile));

    let schema: JsonValue = serde_json::from_slice(SCHEMA).expect("valid schema JSON");
    assert_eq!(schema["additionalProperties"], false);
    let required = schema["required"]
        .as_array()
        .expect("required array")
        .iter()
        .map(|item| item.as_str().expect("required key"))
        .collect::<BTreeSet<_>>();
    assert_eq!(required, keys.into_iter().collect());

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bindings = profile["sourceBindings"]
        .as_array()
        .expect("source bindings");
    let ids = bindings
        .iter()
        .map(|binding| binding["id"].as_str().expect("binding id"))
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), bindings.len());
    for binding in bindings {
        let path = binding["path"].as_str().expect("binding path");
        assert!(root.join(path).is_file(), "missing source binding {path}");
    }
}

#[test]
fn current_snapshot_and_windows_make_no_false_v1_claim() {
    let profile = profile();
    let snapshot = current_support_snapshot();
    assert_eq!(snapshot.release_train, CURRENT_RELEASE_TRAIN);
    assert_eq!(snapshot.release_phase, ReleaseSupportPhase::Preview);
    assert!(!snapshot.v1_stable_active);
    assert_eq!(snapshot.active_deprecations, 0);
    assert_eq!(snapshot.active_security_exceptions, 0);
    assert_eq!(profile["applicability"]["v1StableActive"], false);
    assert_eq!(
        profile["lifecycle"]["standardDays"],
        STANDARD_SUPPORT_MIN_DAYS
    );
    assert_eq!(
        profile["lifecycle"]["maintenanceDays"],
        MAINTENANCE_SUPPORT_MIN_DAYS
    );
    assert_eq!(
        profile["lifecycle"]["securityOnlyDays"],
        SECURITY_ONLY_SUPPORT_MIN_DAYS
    );
    assert_eq!(
        profile["endOfLife"]["minimumLifecycleDays"],
        RELEASE_LINE_EOL_MIN_DAYS
    );
    assert_eq!(
        STANDARD_SUPPORT_MIN_DAYS + MAINTENANCE_SUPPORT_MIN_DAYS + SECURITY_ONLY_SUPPORT_MIN_DAYS,
        RELEASE_LINE_EOL_MIN_DAYS
    );
    assert_eq!(
        profile["endOfLife"]["finalMajorSuccessorDays"],
        FINAL_MAJOR_SUCCESSOR_SUPPORT_MIN_DAYS
    );
    assert_eq!(
        profile["resourceLimits"]["fieldBytes"],
        SUPPORT_FIELD_MAX_BYTES
    );
    assert_eq!(
        profile["resourceLimits"]["evidenceReferences"],
        SUPPORT_EVIDENCE_MAX_REFS
    );
    assert_eq!(
        profile["resourceLimits"]["securityExceptionDays"],
        SECURITY_EXCEPTION_MAX_DAYS
    );
}

#[test]
fn reader_inventory_exactly_matches_the_compatibility_registry() {
    let profile = profile();
    let compatibility: JsonValue =
        serde_json::from_str(COMPATIBILITY).expect("valid compatibility registry");
    let version_surfaces: JsonValue =
        serde_json::from_str(VERSION_SURFACES).expect("valid version surfaces");
    let migrations = version_surfaces["migrations"]
        .as_array()
        .expect("migration records")
        .iter()
        .map(|entry| entry["id"].as_str().expect("migration id"))
        .collect::<BTreeSet<_>>();

    let expected = compatibility["entries"]
        .as_array()
        .expect("compatibility entries")
        .iter()
        .flat_map(|entry| {
            let compatibility_id = entry["stableId"].as_str().expect("stable id");
            entry["components"]
                .as_array()
                .expect("components")
                .iter()
                .flat_map(move |component| {
                    let component_id = component["id"].as_str().expect("component id");
                    let writer = component["currentWriter"].as_str().expect("writer");
                    let migration_ids = component["migrationRecords"]
                        .as_array()
                        .expect("migration ids")
                        .iter()
                        .map(|item| item.as_str().expect("migration id"))
                        .collect::<Vec<_>>();
                    component["acceptedReaders"]
                        .as_array()
                        .expect("accepted readers")
                        .iter()
                        .map(move |reader| {
                            let reader = reader.as_str().expect("reader");
                            let migration = (reader != writer)
                                .then(|| *migration_ids.first().expect("legacy migration"));
                            (
                                compatibility_id.to_string(),
                                component_id.to_string(),
                                writer.to_string(),
                                reader.to_string(),
                                migration.map(str::to_string),
                            )
                        })
                })
        })
        .collect::<BTreeSet<_>>();

    let declared = profile["readers"]["inventory"]
        .as_array()
        .expect("reader inventory")
        .iter()
        .map(|item| {
            let compatibility_id = item["compatibilityId"].as_str().expect("compatibility id");
            let component = item["component"].as_str().expect("component");
            let writer = item["currentWriter"].as_str().expect("writer");
            let reader = item["reader"].as_str().expect("reader");
            let migration = item["migrationRecord"].as_str().map(str::to_string);
            let decision = query_reader_support(compatibility_id, component, reader)
                .expect("declared reader must resolve");
            assert_eq!(decision.current_writer, writer);
            assert_eq!(decision.migration_record, migration);
            assert_eq!(decision.release_phase, ReleaseSupportPhase::Preview);
            assert!(!decision.v1_stable_active);
            assert_eq!(
                decision.mode,
                if migration.is_some() {
                    ReaderSupportMode::Legacy
                } else {
                    ReaderSupportMode::Current
                }
            );
            if let Some(migration) = migration.as_deref() {
                assert!(migrations.contains(migration));
            }
            let replay = query_reader_support(compatibility_id, component, reader)
                .expect("deterministic replay");
            assert_eq!(decision, replay);
            let Term::Map(machine) = decision.to_term() else {
                panic!("reader decision must be a map")
            };
            assert_eq!(
                machine.get(&TermOrdKey(Term::symbol(":v1-stable-active"))),
                Some(&Term::Bool(false))
            );
            (
                compatibility_id.to_string(),
                component.to_string(),
                writer.to_string(),
                reader.to_string(),
                migration,
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(declared, expected);
}

#[test]
fn reader_queries_fail_closed_for_unknown_future_and_oversized_inputs() {
    assert_eq!(
        query_reader_support("genesis/compat/v1/package", "lock", "99"),
        Err(SupportPolicyError::UnsupportedReader)
    );
    assert_eq!(
        query_reader_support("genesis/compat/v1/package", "future", "1"),
        Err(SupportPolicyError::UnknownCompatibilityComponent)
    );
    assert!(matches!(
        query_reader_support(
            "genesis/compat/v1/package",
            "lock",
            &"x".repeat(SUPPORT_FIELD_MAX_BYTES + 1),
        ),
        Err(SupportPolicyError::FieldTooLong {
            field: "reader",
            ..
        })
    ));
    assert!(matches!(
        query_reader_support("genesis/compat/v1/package", "lock", "not portable"),
        Err(SupportPolicyError::InvalidIdentifier { field: "reader" })
    ));
}

#[test]
fn lifecycle_transitions_are_ordered_bounded_and_non_authoritative() {
    let activation = LifecycleTransitionRequest {
        release_identity: "genesis/release/v1.0.0",
        from: ReleaseSupportPhase::Preview,
        to: ReleaseSupportPhase::Standard,
        elapsed_phase_days: 0,
        elapsed_since_ga_days: 0,
        v1_activation_evidence: Some("evidence/R9.1.a/freeze"),
        final_major_line: false,
        successor_major_policy: None,
        days_since_successor_major: 0,
    };
    let accepted = validate_lifecycle_transition(&activation).expect("reviewed v1 activation");
    assert!(accepted.eligible_for_review);
    assert!(!accepted.grants_transition_authority);
    assert_eq!(
        accepted,
        validate_lifecycle_transition(&activation).expect("deterministic lifecycle replay")
    );

    let mut skipped = activation.clone();
    skipped.to = ReleaseSupportPhase::Maintenance;
    assert_eq!(
        validate_lifecycle_transition(&skipped),
        Err(SupportPolicyError::InvalidLifecycleTransition)
    );
    let mut unreviewed = activation;
    unreviewed.v1_activation_evidence = None;
    assert_eq!(
        validate_lifecycle_transition(&unreviewed),
        Err(SupportPolicyError::MissingPrerequisite(
            "v1-activation-evidence"
        ))
    );
}

#[test]
fn lifecycle_eol_requires_every_phase_and_final_line_successor_support() {
    let mut transition = LifecycleTransitionRequest {
        release_identity: "genesis/release/v1.4.0",
        from: ReleaseSupportPhase::SecurityOnly,
        to: ReleaseSupportPhase::EndOfLife,
        elapsed_phase_days: SECURITY_ONLY_SUPPORT_MIN_DAYS,
        elapsed_since_ga_days: RELEASE_LINE_EOL_MIN_DAYS,
        v1_activation_evidence: None,
        final_major_line: true,
        successor_major_policy: Some("genesis/compat/v2/policy"),
        days_since_successor_major: FINAL_MAJOR_SUCCESSOR_SUPPORT_MIN_DAYS,
    };
    assert!(validate_lifecycle_transition(&transition).is_ok());
    transition.elapsed_phase_days -= 1;
    assert!(matches!(
        validate_lifecycle_transition(&transition),
        Err(SupportPolicyError::WindowIncomplete(_))
    ));
}

#[test]
fn removal_windows_require_complete_migration_and_retirement_evidence() {
    let source = base_removal();
    let accepted = validate_removal_eligibility(RemovalClass::SourceOrApi, &source)
        .expect("complete source/API window");
    assert!(accepted.eligible_for_review);
    assert!(!accepted.grants_removal_authority);
    assert_eq!(
        accepted,
        validate_removal_eligibility(RemovalClass::SourceOrApi, &source)
            .expect("deterministic removal replay")
    );

    let mut short = base_removal();
    short.elapsed_days -= 1;
    assert!(matches!(
        validate_removal_eligibility(RemovalClass::SourceOrApi, &short),
        Err(SupportPolicyError::WindowIncomplete(_))
    ));
    let mut too_few_releases = base_removal();
    too_few_releases.subsequent_minor_releases -= 1;
    assert!(matches!(
        validate_removal_eligibility(RemovalClass::SourceOrApi, &too_few_releases),
        Err(SupportPolicyError::WindowIncomplete(_))
    ));

    let mut reader = base_removal();
    reader.elapsed_days = STABLE_READER_RETIREMENT_MIN_DAYS;
    reader.successor_major_policy = Some("genesis/compat/v2/policy");
    reader.corpus_or_telemetry_evidence = Some("evidence/reader-census/v1");
    reader.golden_corpus = Some("goldens/readers/v1");
    assert!(
        validate_removal_eligibility(RemovalClass::StableFormatReader, &reader)
            .expect("complete stable-reader retirement evidence")
            .eligible_for_review
    );
    reader.golden_corpus = None;
    assert_eq!(
        validate_removal_eligibility(RemovalClass::StableFormatReader, &reader),
        Err(SupportPolicyError::MissingPrerequisite("golden-corpus"))
    );
}

#[test]
fn release_line_eol_preserves_the_final_major_successor_window() {
    let mut ordinary = base_removal();
    ordinary.elapsed_days = RELEASE_LINE_EOL_MIN_DAYS;
    assert!(
        validate_removal_eligibility(RemovalClass::ReleaseLineEndOfLife, &ordinary)
            .expect("ordinary line reached minimum lifecycle")
            .eligible_for_review
    );

    ordinary.final_major_line = true;
    assert_eq!(
        validate_removal_eligibility(RemovalClass::ReleaseLineEndOfLife, &ordinary),
        Err(SupportPolicyError::MissingPrerequisite(
            "successor-major-policy"
        ))
    );
    ordinary.successor_major_policy = Some("genesis/compat/v2/policy");
    ordinary.days_since_successor_major = FINAL_MAJOR_SUCCESSOR_SUPPORT_MIN_DAYS - 1;
    assert!(matches!(
        validate_removal_eligibility(RemovalClass::ReleaseLineEndOfLife, &ordinary),
        Err(SupportPolicyError::WindowIncomplete(_))
    ));
    ordinary.days_since_successor_major = FINAL_MAJOR_SUCCESSOR_SUPPORT_MIN_DAYS;
    assert!(
        validate_removal_eligibility(RemovalClass::ReleaseLineEndOfLife, &ordinary)
            .expect("final line successor window complete")
            .eligible_for_review
    );
}

#[test]
fn security_exceptions_are_bounded_non_authoritative_and_fail_closed() {
    let request = base_exception();
    let accepted = validate_security_exception(&request).expect("bounded review candidate");
    assert!(accepted.eligible_for_review);
    assert!(!accepted.grants_exception_authority);
    assert_eq!(
        accepted,
        validate_security_exception(&request).expect("deterministic exception replay")
    );

    let mut semantic = base_exception();
    semantic.changes_wire_or_semantics = true;
    assert_eq!(
        validate_security_exception(&semantic),
        Err(SupportPolicyError::MissingPrerequisite(
            "replacement-identity"
        ))
    );
    semantic.replacement_identity = Some("genesis/effect-log/v4");
    assert_eq!(
        validate_security_exception(&semantic),
        Err(SupportPolicyError::MissingPrerequisite("migrator"))
    );
    semantic.migrator = Some("genesis/migrator/gclog-3-to-4");
    assert!(validate_security_exception(&semantic).is_ok());

    let mut bypass = base_exception();
    bypass.bypasses_protected_invariant = true;
    assert_eq!(
        validate_security_exception(&bypass),
        Err(SupportPolicyError::ProtectedInvariantBypass)
    );
    let mut expired = base_exception();
    expired.duration_days = SECURITY_EXCEPTION_MAX_DAYS + 1;
    assert!(matches!(
        validate_security_exception(&expired),
        Err(SupportPolicyError::InvalidExceptionDuration { .. })
    ));
}

#[test]
fn security_exception_resource_limits_precede_identity_generation() {
    let mut empty = base_exception();
    empty.test_evidence = &[];
    assert_eq!(
        validate_security_exception(&empty),
        Err(SupportPolicyError::MissingPrerequisite("test-evidence"))
    );

    let refs = vec!["tests/security/control"; SUPPORT_EVIDENCE_MAX_REFS + 1];
    let mut too_many = base_exception();
    too_many.test_evidence = &refs;
    assert!(matches!(
        validate_security_exception(&too_many),
        Err(SupportPolicyError::TooManyEvidenceReferences { .. })
    ));

    let duplicate_refs = ["tests/security/control", "tests/security/control"];
    let duplicate = SecurityExceptionRequest {
        test_evidence: &duplicate_refs,
        ..base_exception()
    };
    assert_eq!(
        validate_security_exception(&duplicate),
        Err(SupportPolicyError::NonCanonicalEvidenceReferences)
    );
    let unordered_refs = ["tests/security/z", "tests/security/a"];
    let unordered = SecurityExceptionRequest {
        test_evidence: &unordered_refs,
        ..base_exception()
    };
    assert_eq!(
        validate_security_exception(&unordered),
        Err(SupportPolicyError::NonCanonicalEvidenceReferences)
    );

    let oversized = "x".repeat(SUPPORT_FIELD_MAX_BYTES + 1);
    let mut too_large = base_exception();
    too_large.rationale = &oversized;
    assert!(matches!(
        validate_security_exception(&too_large),
        Err(SupportPolicyError::FieldTooLong {
            field: "rationale",
            ..
        })
    ));
}
