use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};

const BINDING: &str = "selfhost/pkg-publish-authority::inspect";

struct Harness {
    context: EvalCtx,
    inspect: Value,
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
        let mut context = EvalCtx::with_step_limit(Some(40_000_000));
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(140_000_000),
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
        let inspect = environment.get(BINDING).expect("publish inspect binding");
        context.reset_counters();
        Self { context, inspect }
    }

    fn evaluate(&mut self, request: Term) -> Term {
        self.context.reset_counters();
        self.inspect
            .clone()
            .apply(&mut self.context, Value::data(request))
            .expect("apply publish inspect authority")
            .to_plain_term()
            .expect("plain publish inspect result")
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

fn policy(class: Term) -> Term {
    map([
        (":type", Term::symbol(":vcs/policy")),
        (":v", Term::Int(1_i64.into())),
        (":classes", map([(":main", class)])),
    ])
}

fn class(required: Term) -> Term {
    map([
        (":patterns", strings(["refs/heads/main"])),
        (":required-obligations", required),
    ])
}

fn commit(obligations: Term, evidence: Vec<String>, attestations: Vec<String>) -> Term {
    map([
        (":type", Term::symbol(":vcs/commit")),
        (":v", Term::Int(1_i64.into())),
        (":parents", strings([hash(1)])),
        (":base", Term::Nil),
        (":patch", Term::Str(hash(2))),
        (":result", Term::Str(hash(3))),
        (":obligations", obligations),
        (":evidence", strings(evidence)),
        (":attestations", strings(attestations)),
        (":message", Term::Str("publish candidate".into())),
    ])
}

fn request(policy: Term, commit: Term) -> Term {
    let policy_h = artifact_hash(&policy);
    let commit_h = artifact_hash(&commit);
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
        (":mechanism", Term::Nil),
        (":phase", Term::symbol(":inspect")),
        (":v", Term::Int(1_i64.into())),
    ])
}

fn expected_success(request: &Term, evidence: Term, attestations: Term) -> Term {
    let inspect_base = map([
        (":attestation-hashes", attestations),
        (":evidence-hashes", evidence),
    ]);
    let inspect_value = put(
        inspect_base.clone(),
        ":inspect-h",
        Term::Str(term_hash(&inspect_base)),
    );
    map([
        (":code", Term::Nil),
        (
            ":kind",
            Term::Str("genesis/pkg-publish-authority-result-v0.1".into()),
        ),
        (":message", Term::Nil),
        (":ok", Term::Bool(true)),
        (":request-h", Term::Str(term_hash(request))),
        (":v", Term::Int(1_i64.into())),
        (":value", inspect_value),
    ])
}

fn error_code(result: &Term) -> &str {
    assert_eq!(field(result, ":ok"), &Term::Bool(false));
    match field(result, ":code") {
        Term::Str(code) => code,
        other => panic!("error code must be a string, got {other:?}"),
    }
}

#[test]
fn publish_inspect_binds_raw_artifact_and_domain_separated_phase_identities() {
    let evidence = vec![hash(10), hash(10), hash(11)];
    let attestations = vec![hash(21), hash(20)];
    let subject_commit = commit(
        Term::Vector(vec![Term::Str("build".into()), Term::symbol(":lint")]),
        evidence.clone(),
        attestations.clone(),
    );
    let subject_policy = policy(class(Term::Vector(vec![
        Term::Str("build".into()),
        Term::symbol(":lint"),
    ])));
    let subject_request = request(subject_policy.clone(), subject_commit.clone());

    assert_ne!(artifact_hash(&subject_policy), term_hash(&subject_policy));
    assert_ne!(artifact_hash(&subject_commit), term_hash(&subject_commit));

    let actual = Harness::new().evaluate(subject_request.clone());
    assert_eq!(
        actual,
        expected_success(&subject_request, strings(evidence), strings(attestations),)
    );
}

#[test]
fn publish_inspect_rejects_open_malformed_and_substituted_envelopes() {
    let subject_commit = commit(strings(["build"]), vec![], vec![]);
    let subject_policy = policy(class(strings(["build"])));
    let valid = request(subject_policy, subject_commit);
    let mut harness = Harness::new();

    let open = put(valid.clone(), ":extra", Term::Bool(true));
    assert_eq!(
        error_code(&harness.evaluate(open)),
        "core/pkg/bad-authority-request"
    );
    for malformed in [
        put(valid.clone(), ":phase", Term::symbol(":prepare")),
        put(valid.clone(), ":mechanism", map([])),
        put(valid.clone(), ":v", Term::Int(2_i64.into())),
    ] {
        assert_eq!(
            error_code(&harness.evaluate(malformed)),
            "core/pkg/bad-authority-request"
        );
    }

    let facts = field(&valid, ":facts").clone();
    for malformed_facts in [
        put(facts.clone(), ":extra", Term::Bool(true)),
        put(facts.clone(), ":remote", Term::Str(String::new())),
        put(facts.clone(), ":ref", Term::Str(String::new())),
        put(facts.clone(), ":depth", Term::Int((-1_i64).into())),
        put(facts.clone(), ":expected-old", Term::Str("AA".repeat(32))),
        put(facts.clone(), ":commit-h", Term::Str(hash(99))),
        put(facts.clone(), ":policy-h", Term::Str(hash(98))),
    ] {
        let malformed = put(valid.clone(), ":facts", malformed_facts);
        assert_eq!(
            error_code(&harness.evaluate(malformed)),
            "core/pkg/bad-payload"
        );
    }
}

#[test]
fn publish_inspect_orders_policy_commit_and_obligation_failures() {
    let valid_commit = commit(strings(["build"]), vec![], vec![]);
    let valid_policy = policy(class(strings(["build"])));
    let mut harness = Harness::new();

    let bad_policy = put(valid_policy.clone(), ":type", Term::symbol(":wrong"));
    assert_eq!(
        error_code(&harness.evaluate(request(bad_policy, valid_commit.clone()))),
        "core/pkg/bad-policy"
    );

    let frozen_policy = put(
        valid_policy.clone(),
        ":refs",
        map([(":frozen-prefixes", strings(["refs/heads/"]))]),
    );
    assert_eq!(
        error_code(&harness.evaluate(request(frozen_policy, valid_commit.clone()))),
        "core/pkg/ref-frozen"
    );

    let unmatched_policy = policy(map([
        (":patterns", strings(["refs/tags/**"])),
        (":required-obligations", strings(["build"])),
    ]));
    assert_eq!(
        error_code(&harness.evaluate(request(unmatched_policy, valid_commit.clone()))),
        "core/pkg/no-policy-class"
    );

    let bad_commit = put(valid_commit.clone(), ":patch", Term::Str("bad".into()));
    assert_eq!(
        error_code(&harness.evaluate(request(valid_policy.clone(), bad_commit))),
        "core/pkg/bad-commit"
    );

    let missing = commit(strings(["other"]), vec![], vec![]);
    assert_eq!(
        error_code(&harness.evaluate(request(valid_policy, missing))),
        "core/pkg/missing-obligation"
    );
}

#[test]
fn publish_inspect_applies_each_policy_class_obligation_set() {
    let subject_policy = map([
        (":type", Term::symbol(":vcs/policy")),
        (":v", Term::Int(1_i64.into())),
        (
            ":classes",
            map([
                (
                    ":tags",
                    map([
                        (":patterns", strings(["refs/tags/**"])),
                        (":required-obligations", strings(["tag-release"])),
                    ]),
                ),
                (
                    ":main",
                    map([
                        (":patterns", strings(["refs/heads/main"])),
                        (":required-obligations", strings(["main-release"])),
                    ]),
                ),
                (
                    ":dev",
                    map([
                        (":patterns", strings(["refs/heads/**"])),
                        (":required-obligations", strings(["dev-build"])),
                    ]),
                ),
            ]),
        ),
    ]);
    let mut harness = Harness::new();

    for (refname, obligation) in [
        ("refs/tags/v1", "tag-release"),
        ("refs/heads/main", "main-release"),
        ("refs/heads/feature", "dev-build"),
    ] {
        let subject_commit = commit(strings([obligation]), vec![], vec![]);
        let mut subject_request = request(subject_policy.clone(), subject_commit);
        let facts = put(
            field(&subject_request, ":facts").clone(),
            ":ref",
            Term::Str(refname.into()),
        );
        subject_request = put(subject_request, ":facts", facts);
        assert_eq!(
            field(&harness.evaluate(subject_request), ":ok"),
            &Term::Bool(true),
            "inspect class for {refname}"
        );
    }
}
