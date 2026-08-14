use std::collections::BTreeSet;

use base64ct::{Base64, Encoding};
use ed25519_dalek::SigningKey;
use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};
use gc_vcs::{RequirementsTraceGateContext, ToolQualificationGateContext};

const BINDING: &str = "selfhost/pkg-publish-authority::prepare";

struct Harness {
    context: EvalCtx,
    prepare: Value,
}

fn artifact_path() -> std::path::PathBuf {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let artifact = std::env::var_os("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| workspace.join("selfhost/toolchain.gc"));
    let artifact = if artifact.is_absolute() {
        artifact
    } else {
        workspace.join(artifact)
    };
    artifact
        .canonicalize()
        .expect("canonical selfhost artifact path")
}

impl Harness {
    fn new() -> Self {
        let mut context = EvalCtx::with_step_limit(Some(80_000_000));
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(240_000_000),
            max_bytes_len: Some(4 * 1024 * 1024),
            max_map_len: Some(65_536),
            max_string_len: Some(4 * 1024 * 1024),
            max_vec_len: Some(65_536),
            ..MemLimits::default()
        });
        let prelude = build_prelude(&mut context);
        let mut environment = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut context,
            &mut environment,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact_path()),
        )
        .expect("artifact-only toolchain bootstrap");
        let prepare = environment.get(BINDING).expect("publish prepare binding");
        context.reset_counters();
        Self { context, prepare }
    }

    fn evaluate(&mut self, request: Term) -> Term {
        self.context.reset_counters();
        self.prepare
            .clone()
            .apply(&mut self.context, Value::data(request))
            .expect("apply publish prepare authority")
            .to_plain_term()
            .expect("plain publish prepare result")
    }
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value))
            .collect(),
    )
}

fn put(term: Term, key: &'static str, value: Term) -> Term {
    let Term::Map(mut values) = term else {
        panic!("expected map");
    };
    values.insert(TermOrdKey(Term::symbol(key)), value);
    Term::Map(values)
}

fn field<'a>(term: &'a Term, key: &str) -> &'a Term {
    let Term::Map(values) = term else {
        panic!("expected map, got {term:?}");
    };
    values
        .get(&TermOrdKey(Term::symbol(key)))
        .unwrap_or_else(|| panic!("missing field {key}"))
}

fn strings(values: impl IntoIterator<Item = impl Into<String>>) -> Term {
    Term::Vector(
        values
            .into_iter()
            .map(|value| Term::Str(value.into()))
            .collect(),
    )
}

fn hash(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn artifact_hash(term: &Term) -> String {
    blake3::hash(print_term(term).as_bytes())
        .to_hex()
        .to_string()
}

fn term_hash(term: &Term) -> String {
    gc_vcs::bytes32_to_hex(&hash_term(term))
}

fn object(term: Term) -> Term {
    let printed = print_term(&term);
    map([
        (":bytes", Term::Bytes(printed.as_bytes().to_vec().into())),
        (":h", Term::Str(artifact_hash(&term))),
        (":term", term),
    ])
}

fn class(required_obligations: &[&str], required_kinds: &[&str]) -> Term {
    map([
        (":patterns", strings(["refs/heads/main"])),
        (
            ":required-evidence-kinds",
            strings(required_kinds.iter().copied()),
        ),
        (
            ":required-obligations",
            strings(required_obligations.iter().copied()),
        ),
    ])
}

fn policy(class: Term) -> Term {
    map([
        (":type", Term::symbol(":vcs/policy")),
        (":v", Term::Int(1_i64.into())),
        (":classes", map([(":main", class)])),
    ])
}

fn commit(obligations: &[&str], evidence: &[Term], attestations: &[Term]) -> Term {
    map([
        (":type", Term::symbol(":vcs/commit")),
        (":v", Term::Int(1_i64.into())),
        (":parents", strings([hash(1)])),
        (":base", Term::Nil),
        (":patch", Term::Str(hash(2))),
        (":result", Term::Str(hash(3))),
        (":obligations", strings(obligations.iter().copied())),
        (":evidence", strings(evidence.iter().map(artifact_hash))),
        (
            ":attestations",
            strings(attestations.iter().map(artifact_hash)),
        ),
        (":message", Term::Str("publish candidate".into())),
    ])
}

fn inspect_hash(commit: &Term) -> String {
    let base = map([
        (
            ":attestation-hashes",
            field(commit, ":attestations").clone(),
        ),
        (":evidence-hashes", field(commit, ":evidence").clone()),
    ]);
    term_hash(&base)
}

fn request(
    policy: Term,
    commit: Term,
    evidence: impl IntoIterator<Item = Term>,
    attestations: impl IntoIterator<Item = Term>,
) -> Term {
    let policy_h = artifact_hash(&policy);
    let commit_h = artifact_hash(&commit);
    let inspect_h = inspect_hash(&commit);
    map([
        (
            ":facts",
            map([
                (":commit", commit),
                (":commit-h", Term::Str(commit_h)),
                (":depth", Term::Int(0_i64.into())),
                (":expected-old", Term::Nil),
                (":policy", policy),
                (":policy-h", Term::Str(policy_h)),
                (":ref", Term::Str("refs/heads/main".into())),
                (":remote", Term::Str("https://registry.invalid".into())),
            ]),
        ),
        (
            ":kind",
            Term::Str("genesis/pkg-publish-authority-request-v0.1".into()),
        ),
        (
            ":mechanism",
            map([
                (
                    ":attestations",
                    Term::Vector(attestations.into_iter().map(object).collect()),
                ),
                (
                    ":evidence",
                    Term::Vector(evidence.into_iter().map(object).collect()),
                ),
                (":inspect-h", Term::Str(inspect_h)),
            ]),
        ),
        (":phase", Term::symbol(":prepare")),
        (":v", Term::Int(1_i64.into())),
    ])
}

fn generic_evidence(kind: &str) -> Term {
    map([
        (":type", Term::symbol(":vcs/evidence")),
        (":v", Term::Int(1_i64.into())),
        (":kind", Term::symbol(kind)),
    ])
}

fn error_code(result: &Term) -> &str {
    assert_eq!(field(result, ":ok"), &Term::Bool(false));
    match field(result, ":code") {
        Term::Str(code) => code,
        other => panic!("error code must be a string, got {other:?}"),
    }
}

fn assert_empty_crypto_success(request: &Term, result: &Term) {
    assert_eq!(field(result, ":ok"), &Term::Bool(true));
    assert_eq!(field(result, ":request-h"), &Term::Str(term_hash(request)));
    let value = field(result, ":value");
    assert_eq!(field(value, ":crypto-requests"), &Term::Vector(vec![]));
    let base = map([(":crypto-requests", field(value, ":crypto-requests").clone())]);
    assert_eq!(field(value, ":prepare-h"), &Term::Str(term_hash(&base)));
}

#[test]
fn publish_prepare_accepts_exact_content_bound_evidence() {
    let evidence = generic_evidence(":build");
    let subject_commit = commit(&["build"], std::slice::from_ref(&evidence), &[]);
    let subject_request = request(
        policy(class(&["build"], &[":build"])),
        subject_commit,
        [evidence],
        [],
    );

    let actual = Harness::new().evaluate(subject_request.clone());
    assert_empty_crypto_success(&subject_request, &actual);
}

#[test]
fn publish_prepare_rejects_missing_reordered_and_substituted_objects() {
    let first = generic_evidence(":build");
    let second = generic_evidence(":proof");
    let subject_commit = commit(&["build"], &[first.clone(), second.clone()], &[]);
    let valid = request(
        policy(class(&["build"], &[":build", ":proof"])),
        subject_commit,
        [first, second],
        [],
    );
    let mut harness = Harness::new();

    let mechanism = field(&valid, ":mechanism").clone();
    let Term::Vector(objects) = field(&mechanism, ":evidence") else {
        panic!("evidence objects must be a vector");
    };
    let reversed = put(
        mechanism.clone(),
        ":evidence",
        Term::Vector(vec![objects[1].clone(), objects[0].clone()]),
    );
    let missing = put(
        mechanism.clone(),
        ":evidence",
        Term::Vector(vec![objects[0].clone()]),
    );
    let bad_bytes = put(
        objects[0].clone(),
        ":bytes",
        Term::Bytes(b"substitution".to_vec().into()),
    );
    let bad_term = put(objects[0].clone(), ":term", generic_evidence(":other"));
    let bad_hash = put(objects[0].clone(), ":h", Term::Str(hash(99)));
    let bad_inspect = put(mechanism.clone(), ":inspect-h", Term::Str(hash(98)));
    for bad_mechanism in [
        reversed,
        missing,
        put(
            mechanism.clone(),
            ":evidence",
            Term::Vector(vec![bad_bytes, objects[1].clone()]),
        ),
        put(
            mechanism.clone(),
            ":evidence",
            Term::Vector(vec![bad_term, objects[1].clone()]),
        ),
        put(
            mechanism.clone(),
            ":evidence",
            Term::Vector(vec![bad_hash, objects[1].clone()]),
        ),
        bad_inspect,
    ] {
        assert_eq!(
            error_code(&harness.evaluate(put(valid.clone(), ":mechanism", bad_mechanism,))),
            "core/pkg/bad-authority-request"
        );
    }
}

#[test]
fn publish_prepare_enforces_required_evidence_closure() {
    let mut harness = Harness::new();
    let no_evidence_commit = commit(&["build"], &[], &[]);
    let no_evidence = request(policy(class(&["build"], &[])), no_evidence_commit, [], []);
    assert_eq!(
        error_code(&harness.evaluate(no_evidence)),
        "core/pkg/missing-evidence"
    );

    let evidence = generic_evidence(":build");
    let wrong_kind_commit = commit(&["build"], std::slice::from_ref(&evidence), &[]);
    let wrong_kind = request(
        policy(class(&["build"], &[":proof"])),
        wrong_kind_commit,
        [evidence],
        [],
    );
    assert_eq!(
        error_code(&harness.evaluate(wrong_kind)),
        "core/pkg/missing-evidence-kind"
    );
}

fn requirements_trace(policy_h: &str) -> Term {
    map([
        (":type", Term::symbol(":vcs/evidence")),
        (":v", Term::Int(1_i64.into())),
        (":kind", Term::symbol(":requirements-trace")),
        (":status", Term::symbol(":verified")),
        (":graph-h", Term::Str(hash(31))),
        (
            ":release",
            map([
                (":snapshot", Term::Str(hash(3))),
                (":policy", Term::Str(policy_h.into())),
            ]),
        ),
        (
            ":requirements",
            Term::Vector(vec![map([
                (":id", Term::Str("REQ-1".into())),
                (":level", Term::symbol(":system")),
                (
                    ":links",
                    map([
                        (":obligations", strings(["build"])),
                        (
                            ":evidence-kinds",
                            Term::Vector(vec![Term::symbol(":tool-qualification")]),
                        ),
                    ]),
                ),
            ])]),
        ),
    ])
}

fn tool_qualification(policy_h: &str) -> Term {
    map([
        (":type", Term::symbol(":vcs/evidence")),
        (":v", Term::Int(1_i64.into())),
        (":kind", Term::symbol(":tool-qualification")),
        (":status", Term::symbol(":qualified")),
        (
            ":release",
            map([
                (":snapshot", Term::Str(hash(3))),
                (":policy", Term::Str(policy_h.into())),
            ]),
        ),
        (":requirements", strings(["REQ-1"])),
        (
            ":tools",
            Term::Vector(vec![map([
                (":name", Term::Str("genesis".into())),
                (":blake3", Term::Str(hash(32))),
            ])]),
        ),
        (
            ":qualification-tests",
            Term::Vector(vec![map([
                (":id", Term::Str("QT-1".into())),
                (":artifact", Term::Str(hash(33))),
                (":manifest", Term::Str(hash(34))),
                (":run-id", Term::Str("run-1".into())),
                (":runner", Term::Str("genesis".into())),
                (":profile", Term::Str("release".into())),
                (":snapshot", Term::Str(hash(3))),
                (":policy", Term::Str(policy_h.into())),
                (":result", Term::symbol(":pass")),
            ])]),
        ),
    ])
}

#[test]
fn publish_prepare_matches_native_assurance_validation_and_rejects_mutations() {
    let subject_policy = policy(class(
        &["build"],
        &[":requirements-trace", ":tool-qualification"],
    ));
    let policy_h = artifact_hash(&subject_policy);
    let trace = requirements_trace(&policy_h);
    let qualification = tool_qualification(&policy_h);
    let subject_commit = commit(&["build"], &[trace.clone(), qualification.clone()], &[]);
    let commit_h = artifact_hash(&subject_commit);
    let observed = BTreeSet::from([
        ":requirements-trace".to_string(),
        ":tool-qualification".to_string(),
    ]);
    gc_vcs::validate_requirements_trace_evidence(
        &trace,
        &RequirementsTraceGateContext {
            commit_hash: &commit_h,
            snapshot_hash: &hash(3),
            policy_hash: Some(&policy_h),
            commit_obligations: &["build".into()],
            observed_evidence_kinds: &observed,
        },
    )
    .expect("native requirements validation");
    gc_vcs::validate_tool_qualification_evidence(
        &qualification,
        &ToolQualificationGateContext {
            commit_hash: &commit_h,
            snapshot_hash: &hash(3),
            policy_hash: Some(&policy_h),
        },
    )
    .expect("native qualification validation");

    let valid = request(
        subject_policy.clone(),
        subject_commit,
        [trace.clone(), qualification.clone()],
        [],
    );
    let mut harness = Harness::new();
    assert_empty_crypto_success(&valid, &harness.evaluate(valid.clone()));

    let bad_release = put(
        field(&trace, ":release").clone(),
        ":snapshot",
        Term::Str(hash(90)),
    );
    let bad_trace = put(trace, ":release", bad_release);
    let commit_with_bad_trace =
        commit(&["build"], &[bad_trace.clone(), qualification.clone()], &[]);
    assert_eq!(
        error_code(&harness.evaluate(request(
            subject_policy.clone(),
            commit_with_bad_trace,
            [bad_trace, qualification.clone()],
            [],
        ))),
        "core/pkg/invalid-requirements-trace"
    );

    let Term::Vector(tests) = field(&qualification, ":qualification-tests") else {
        panic!("qualification tests must be a vector");
    };
    let failed_test = put(tests[0].clone(), ":result", Term::symbol(":fail"));
    let failed_qualification = put(
        qualification,
        ":qualification-tests",
        Term::Vector(vec![failed_test]),
    );
    let valid_trace = requirements_trace(&policy_h);
    let commit_with_failed_qualification = commit(
        &["build"],
        &[valid_trace.clone(), failed_qualification.clone()],
        &[],
    );
    assert_eq!(
        error_code(&harness.evaluate(request(
            subject_policy,
            commit_with_failed_qualification,
            [valid_trace, failed_qualification],
            [],
        ))),
        "core/pkg/invalid-tool-qualification"
    );
}

fn attestation(signing_h: [u8; 32], public_key: [u8; 32]) -> Term {
    map([
        (":type", Term::symbol(":vcs/attestation")),
        (":v", Term::Int(1_i64.into())),
        (":alg", Term::Str("ed25519".into())),
        (":signing-h", Term::Bytes(signing_h.to_vec().into())),
        (":pk", Term::Bytes(public_key.to_vec().into())),
        (":sig", Term::Bytes(vec![42_u8; 64].into())),
    ])
}

#[test]
fn publish_prepare_derives_exact_crypto_requests_and_rejects_signing_substitution() {
    let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
    let public_key = signing_key.verifying_key().to_bytes();
    let public_key_b64 = Base64::encode_string(&public_key);
    let signing_class = put(
        put(
            put(class(&[], &[]), ":require-signatures", Term::Bool(true)),
            ":min-signatures",
            Term::Int(1_i64.into()),
        ),
        ":allowed-public-keys",
        strings([public_key_b64.clone()]),
    );
    let subject_policy = policy(signing_class);
    let unsigned_commit = commit(&[], &[], &[]);
    let signing_h = gc_vcs::commit_signing_hash(&unsigned_commit).expect("commit signing hash");
    let subject_attestation = attestation(signing_h, public_key);
    let signed_commit = commit(&[], &[], std::slice::from_ref(&subject_attestation));
    assert_eq!(
        gc_vcs::commit_signing_hash(&signed_commit).expect("signed commit hash"),
        signing_h
    );
    let subject_request = request(
        subject_policy.clone(),
        signed_commit,
        [],
        [subject_attestation.clone()],
    );
    let mut harness = Harness::new();
    let actual = harness.evaluate(subject_request.clone());
    assert_eq!(field(&actual, ":ok"), &Term::Bool(true));
    assert_eq!(
        field(&actual, ":request-h"),
        &Term::Str(term_hash(&subject_request))
    );
    let Term::Vector(crypto) = field(field(&actual, ":value"), ":crypto-requests") else {
        panic!("crypto requests must be a vector");
    };
    assert_eq!(crypto.len(), 1);
    let crypto = &crypto[0];
    assert_eq!(field(crypto, ":alg"), &Term::Str("ed25519".into()));
    assert_eq!(
        field(crypto, ":allowed-public-keys"),
        &strings([public_key_b64])
    );
    assert_eq!(
        field(crypto, ":attestation-h"),
        &Term::Str(artifact_hash(&subject_attestation))
    );
    assert_eq!(
        field(crypto, ":signing-h"),
        &Term::Bytes(signing_h.to_vec().into())
    );
    assert_eq!(
        field(crypto, ":sign-message"),
        &Term::Bytes(gc_vcs::commit_attestation_message(&signing_h).into())
    );
    let mut base = crypto.clone();
    let Term::Map(ref mut fields) = base else {
        panic!("crypto request must be a map");
    };
    let request_h = fields
        .remove(&TermOrdKey(Term::symbol(":request-h")))
        .expect("crypto request hash");
    assert_eq!(request_h, Term::Str(term_hash(&base)));

    let substituted = put(
        subject_attestation,
        ":signing-h",
        Term::Bytes([99_u8; 32].to_vec().into()),
    );
    let substituted_commit = commit(&[], &[], std::slice::from_ref(&substituted));
    assert_eq!(
        error_code(&harness.evaluate(request(
            subject_policy,
            substituted_commit,
            [],
            [substituted],
        ))),
        "core/pkg/bad-attestation"
    );
}
