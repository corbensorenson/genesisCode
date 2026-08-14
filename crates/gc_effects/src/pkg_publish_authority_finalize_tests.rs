use base64ct::{Base64, Encoding};
use ed25519_dalek::SigningKey;
use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};

const BINDING: &str = "core/pkg::publish-authority";

struct Harness {
    context: EvalCtx,
    authority: Value,
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
        let mut context = EvalCtx::with_step_limit(Some(100_000_000));
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(300_000_000),
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
        let authority = environment.get(BINDING).expect("publish authority binding");
        context.reset_counters();
        Self { context, authority }
    }

    fn evaluate(&mut self, request: Term) -> Term {
        self.context.reset_counters();
        self.authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .expect("apply publish authority")
            .to_plain_term()
            .expect("plain publish authority result")
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

fn commit(obligations: Term, evidence: &[Term], attestations: &[Term]) -> Term {
    map([
        (":type", Term::symbol(":vcs/commit")),
        (":v", Term::Int(1_i64.into())),
        (":parents", strings([hash(1)])),
        (":base", Term::Nil),
        (":patch", Term::Str(hash(2))),
        (":result", Term::Str(hash(3))),
        (":obligations", obligations),
        (":evidence", strings(evidence.iter().map(artifact_hash))),
        (
            ":attestations",
            strings(attestations.iter().map(artifact_hash)),
        ),
        (":message", Term::Str("publish candidate".into())),
    ])
}

fn generic_evidence(kind: &str) -> Term {
    map([
        (":type", Term::symbol(":vcs/evidence")),
        (":v", Term::Int(1_i64.into())),
        (":kind", Term::symbol(kind)),
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

fn facts(policy: Term, commit: Term, depth: i64, expected_old: Option<String>) -> Term {
    map([
        (":commit-h", Term::Str(artifact_hash(&commit))),
        (":commit", commit),
        (":depth", Term::Int(depth.into())),
        (
            ":expected-old",
            expected_old.map(Term::Str).unwrap_or(Term::Nil),
        ),
        (":policy-h", Term::Str(artifact_hash(&policy))),
        (":policy", policy),
        (":ref", Term::Str("refs/heads/main".into())),
        (":remote", Term::Str("https://registry.invalid".into())),
    ])
}

fn phase_request(facts: Term, phase: &str, mechanism: Term) -> Term {
    map([
        (":facts", facts),
        (
            ":kind",
            Term::Str("genesis/pkg-publish-authority-request-v0.1".into()),
        ),
        (":mechanism", mechanism),
        (":phase", Term::symbol(phase)),
        (":v", Term::Int(1_i64.into())),
    ])
}

fn prepare_mechanism(commit: &Term, evidence: &[Term], attestations: &[Term]) -> Term {
    map([
        (
            ":attestations",
            Term::Vector(attestations.iter().cloned().map(object).collect()),
        ),
        (
            ":evidence",
            Term::Vector(evidence.iter().cloned().map(object).collect()),
        ),
        (":inspect-h", Term::Str(inspect_hash(commit))),
    ])
}

struct Prepared {
    facts: Term,
    mechanism: Term,
    prepare_h: String,
    requests: Vec<Term>,
}

fn prepare_case(
    harness: &mut Harness,
    policy: Term,
    commit: Term,
    evidence: &[Term],
    attestations: &[Term],
    depth: i64,
    expected_old: Option<String>,
) -> Prepared {
    let facts = facts(policy, commit.clone(), depth, expected_old);
    let mechanism = prepare_mechanism(&commit, evidence, attestations);
    let request = phase_request(facts.clone(), ":prepare", mechanism.clone());
    let result = harness.evaluate(request);
    assert_eq!(
        field(&result, ":ok"),
        &Term::Bool(true),
        "prepare result: {}",
        print_term(&result)
    );
    let value = field(&result, ":value");
    let Term::Vector(requests) = field(value, ":crypto-requests") else {
        panic!("crypto requests must be a vector");
    };
    let Term::Str(prepare_h) = field(value, ":prepare-h") else {
        panic!("prepare hash must be a string");
    };
    Prepared {
        facts,
        mechanism,
        prepare_h: prepare_h.clone(),
        requests: requests.clone(),
    }
}

fn crypto_facts(requests: &[Term], valid: &[bool]) -> Term {
    assert_eq!(requests.len(), valid.len());
    Term::Vector(
        requests
            .iter()
            .zip(valid)
            .map(|(request, valid)| {
                map([
                    (":request-h", field(request, ":request-h").clone()),
                    (":signature-valid", Term::Bool(*valid)),
                ])
            })
            .collect(),
    )
}

fn finalize_request(prepared: &Prepared, facts: Term) -> Term {
    let mechanism = map([
        (
            ":attestations",
            field(&prepared.mechanism, ":attestations").clone(),
        ),
        (":crypto-facts", facts),
        (":evidence", field(&prepared.mechanism, ":evidence").clone()),
        (
            ":inspect-h",
            field(&prepared.mechanism, ":inspect-h").clone(),
        ),
        (":prepare-h", Term::Str(prepared.prepare_h.clone())),
    ]);
    phase_request(prepared.facts.clone(), ":finalize", mechanism)
}

fn error_code(result: &Term) -> &str {
    assert_eq!(field(result, ":ok"), &Term::Bool(false));
    match field(result, ":code") {
        Term::Str(code) => code,
        other => panic!("error code must be a string, got {other:?}"),
    }
}

fn attestation(signing_h: [u8; 32], key: &SigningKey, role: &str) -> Term {
    map([
        (":type", Term::symbol(":vcs/attestation")),
        (":v", Term::Int(1_i64.into())),
        (":alg", Term::Str("ed25519".into())),
        (":signing-h", Term::Bytes(signing_h.to_vec().into())),
        (
            ":pk",
            Term::Bytes(key.verifying_key().to_bytes().to_vec().into()),
        ),
        (":sig", Term::Bytes(vec![42_u8; 64].into())),
        (":role", Term::Str(role.into())),
    ])
}

fn signing_class(
    keys: &[&SigningKey],
    minimum: i64,
    required_roles: &[&str],
    role_minimums: Term,
    independent_pairs: Term,
) -> Term {
    let allowed = strings(
        keys.iter()
            .map(|key| Base64::encode_string(&key.verifying_key().to_bytes())),
    );
    let signing = put(
        put(
            put(
                put(
                    put(class(&[], &[]), ":require-signatures", Term::Bool(true)),
                    ":min-signatures",
                    Term::Int(minimum.into()),
                ),
                ":allowed-public-keys",
                allowed,
            ),
            ":role-min-signatures",
            role_minimums,
        ),
        ":independent-role-pairs",
        independent_pairs,
    );
    if required_roles.is_empty() {
        signing
    } else {
        put(
            signing,
            ":required-attestation-roles",
            strings(required_roles.iter().copied()),
        )
    }
}

#[test]
fn publish_finalize_emits_exact_sync_and_provenance_without_signature_work() {
    let evidence = generic_evidence(":build");
    let ignored_attestation = map([(
        ":note",
        Term::Str("not interpreted when signatures are disabled".into()),
    )]);
    let subject_commit = commit(
        Term::Vector(vec![Term::symbol(":build")]),
        std::slice::from_ref(&evidence),
        std::slice::from_ref(&ignored_attestation),
    );
    let subject_policy = policy(class(&[":build"], &[":build"]));
    let mut harness = Harness::new();
    let prepared = prepare_case(
        &mut harness,
        subject_policy,
        subject_commit.clone(),
        &[evidence],
        &[ignored_attestation],
        2,
        Some(hash(9)),
    );
    assert!(prepared.requests.is_empty());
    let request = finalize_request(&prepared, Term::Vector(vec![]));
    let actual = harness.evaluate(request.clone());
    assert_eq!(field(&actual, ":ok"), &Term::Bool(true));
    assert_eq!(
        field(&actual, ":request-h"),
        &Term::Str(term_hash(&request))
    );

    let expected_set_ref = map([
        (":expected-old", Term::Str(hash(9))),
        (":hash", field(&prepared.facts, ":commit-h").clone()),
        (":name", Term::Str("refs/heads/main".into())),
        (":policy", field(&prepared.facts, ":policy-h").clone()),
    ]);
    let expected_sync = map([
        (":depth", Term::Int(2_i64.into())),
        (":remote", Term::Str("https://registry.invalid".into())),
        (
            ":roots",
            Term::Vector(vec![
                field(&prepared.facts, ":commit-h").clone(),
                field(&prepared.facts, ":policy-h").clone(),
            ]),
        ),
        (":set-refs", Term::Vector(vec![expected_set_ref])),
    ]);
    let expected_provenance = map([
        (
            ":attestations",
            field(&subject_commit, ":attestations").clone(),
        ),
        (":base", Term::Nil),
        (":evidence", field(&subject_commit, ":evidence").clone()),
        (":obligations", Term::Vector(vec![Term::symbol(":build")])),
        (":parents", strings([hash(1)])),
        (":patch", Term::Str(hash(2))),
        (":result", Term::Str(hash(3))),
    ]);
    assert_eq!(
        field(&actual, ":value"),
        &map([
            (":commit", field(&prepared.facts, ":commit-h").clone()),
            (":provenance", expected_provenance),
            (":ref", Term::Str("refs/heads/main".into())),
            (":sync", expected_sync),
        ])
    );
}

fn signed_case(harness: &mut Harness, class: Term, attestations: Vec<Term>) -> Prepared {
    let subject_commit = commit(strings([] as [&str; 0]), &[], &attestations);
    prepare_case(
        harness,
        policy(class),
        subject_commit,
        &[],
        &attestations,
        0,
        None,
    )
}

#[test]
fn publish_finalize_enforces_distinct_signers_role_minima_and_independence() {
    let key_a = SigningKey::from_bytes(&[7_u8; 32]);
    let key_b = SigningKey::from_bytes(&[8_u8; 32]);
    let unsigned = commit(strings([] as [&str; 0]), &[], &[]);
    let signing_h = gc_vcs::commit_signing_hash(&unsigned).expect("signing hash");
    let mut harness = Harness::new();

    let duplicate_class = signing_class(&[&key_a, &key_b], 2, &[], map([]), Term::Vector(vec![]));
    let duplicate = signed_case(
        &mut harness,
        duplicate_class,
        vec![
            attestation(signing_h, &key_a, "maintainer"),
            attestation(signing_h, &key_a, "reviewer"),
        ],
    );
    assert_eq!(
        error_code(&harness.evaluate(finalize_request(
            &duplicate,
            crypto_facts(&duplicate.requests, &[true, true]),
        ))),
        "core/pkg/missing-signatures"
    );

    let role_min_class = signing_class(
        &[&key_a, &key_b],
        2,
        &["reviewer"],
        map([(":reviewer", Term::Int(2_i64.into()))]),
        Term::Vector(vec![]),
    );
    let role_min = signed_case(
        &mut harness,
        role_min_class,
        vec![
            attestation(signing_h, &key_a, "reviewer"),
            attestation(signing_h, &key_b, "maintainer"),
        ],
    );
    assert_eq!(
        error_code(&harness.evaluate(finalize_request(
            &role_min,
            crypto_facts(&role_min.requests, &[true, true]),
        ))),
        "core/pkg/missing-attestation-role-signatures"
    );

    let pair = map([
        (":left", Term::Str("maintainer".into())),
        (":right", Term::Str("reviewer".into())),
    ]);
    let independent_class = signing_class(
        &[&key_a, &key_b],
        1,
        &["maintainer", "reviewer"],
        map([]),
        Term::Vector(vec![pair]),
    );
    let independence = signed_case(
        &mut harness,
        independent_class,
        vec![
            attestation(signing_h, &key_a, "maintainer"),
            attestation(signing_h, &key_a, "reviewer"),
        ],
    );
    assert_eq!(
        error_code(&harness.evaluate(finalize_request(
            &independence,
            crypto_facts(&independence.requests, &[true, true]),
        ))),
        "core/pkg/role-independence-violation"
    );
}

#[test]
fn publish_finalize_accepts_independent_roles_and_rejects_unbound_crypto_facts() {
    let key_a = SigningKey::from_bytes(&[9_u8; 32]);
    let key_b = SigningKey::from_bytes(&[10_u8; 32]);
    let unsigned = commit(strings([] as [&str; 0]), &[], &[]);
    let signing_h = gc_vcs::commit_signing_hash(&unsigned).expect("signing hash");
    let pair = map([
        (":left", Term::Str("maintainer".into())),
        (":right", Term::Str("reviewer".into())),
    ]);
    let subject_class = signing_class(
        &[&key_a, &key_b],
        2,
        &["maintainer", "reviewer"],
        map([
            (":maintainer", Term::Int(1_i64.into())),
            (":reviewer", Term::Int(1_i64.into())),
        ]),
        Term::Vector(vec![pair]),
    );
    let mut harness = Harness::new();
    let prepared = signed_case(
        &mut harness,
        subject_class,
        vec![
            attestation(signing_h, &key_a, " maintainer "),
            attestation(signing_h, &key_b, ":reviewer"),
        ],
    );
    let valid_facts = crypto_facts(&prepared.requests, &[true, true]);
    let accepted = harness.evaluate(finalize_request(&prepared, valid_facts.clone()));
    assert_eq!(field(&accepted, ":ok"), &Term::Bool(true));
    assert!(!matches!(
        field(field(&accepted, ":value"), ":sync"),
        Term::Map(values) if values.contains_key(&TermOrdKey(Term::symbol(":depth")))
    ));
    let Term::Vector(facts) = valid_facts else {
        panic!("crypto facts must be a vector");
    };
    let reordered = Term::Vector(vec![facts[1].clone(), facts[0].clone()]);
    let open = put(facts[0].clone(), ":extra", Term::Nil);
    let wrong_hash = put(facts[0].clone(), ":request-h", Term::Str(hash(99)));
    for malformed in [
        reordered,
        Term::Vector(vec![facts[0].clone()]),
        Term::Vector(vec![open, facts[1].clone()]),
        Term::Vector(vec![wrong_hash, facts[1].clone()]),
    ] {
        assert_eq!(
            error_code(&harness.evaluate(finalize_request(&prepared, malformed))),
            "core/pkg/bad-authority-request"
        );
    }

    let false_facts = crypto_facts(&prepared.requests, &[true, false]);
    assert_eq!(
        error_code(&harness.evaluate(finalize_request(&prepared, false_facts))),
        "core/pkg/missing-signatures"
    );

    let valid = finalize_request(&prepared, crypto_facts(&prepared.requests, &[true, true]));
    let mechanism = put(
        field(&valid, ":mechanism").clone(),
        ":prepare-h",
        Term::Str(hash(98)),
    );
    assert_eq!(
        error_code(&harness.evaluate(put(valid, ":mechanism", mechanism))),
        "core/pkg/bad-authority-request"
    );
}

#[test]
fn publish_authority_dispatcher_rejects_unknown_and_open_phases() {
    let mut harness = Harness::new();
    let unknown = phase_request(map([]), ":unknown", Term::Nil);
    assert_eq!(
        error_code(&harness.evaluate(unknown)),
        "core/pkg/bad-authority-request"
    );
    assert_eq!(
        error_code(&harness.evaluate(map([(":phase", Term::symbol(":inspect"),)]))),
        "core/pkg/bad-authority-request"
    );
}
