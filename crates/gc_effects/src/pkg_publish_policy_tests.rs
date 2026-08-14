use base64ct::{Base64, Encoding};
use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};
use gc_vcs::{Policy, PolicyClass};

const BINDING: &str = "selfhost/pkg-publish-policy::select";

struct Harness {
    context: EvalCtx,
    selector: Value,
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
        let mut context = EvalCtx::with_step_limit(Some(30_000_000));
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(120_000_000),
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
        let selector = environment.get(BINDING).expect("publish policy binding");
        context.reset_counters();
        Self { context, selector }
    }

    fn evaluate(&mut self, policy: Term, refname: &str) -> Term {
        self.context.reset_counters();
        let partial = self
            .selector
            .clone()
            .apply(&mut self.context, Value::data(policy))
            .expect("apply publish policy");
        partial
            .apply(
                &mut self.context,
                Value::data(Term::Str(refname.to_string())),
            )
            .expect("apply publish ref")
            .to_plain_term()
            .expect("plain publish policy result")
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

fn string_map(entries: impl IntoIterator<Item = (String, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(key, value)| (TermOrdKey(Term::Str(key)), value))
            .collect(),
    )
}

fn strings(values: impl IntoIterator<Item = impl Into<String>>) -> Term {
    Term::Vector(
        values
            .into_iter()
            .map(|value| Term::Str(value.into()))
            .collect(),
    )
}

fn policy(classes: Term) -> Term {
    map([
        (":type", Term::symbol(":vcs/policy")),
        (":v", Term::Int(1_i64.into())),
        (":classes", classes),
    ])
}

fn class(patterns: &[&str]) -> Term {
    map([(":patterns", strings(patterns.iter().copied()))])
}

fn field<'a>(term: &'a Term, key: &str) -> &'a Term {
    let Term::Map(values) = term else {
        panic!("expected map, got {term:?}");
    };
    values
        .get(&TermOrdKey(Term::symbol(key)))
        .unwrap_or_else(|| panic!("missing field {key}"))
}

fn selected_name(result: &Term) -> Option<&str> {
    if field(result, ":ok") != &Term::Bool(true) {
        return None;
    }
    match field(field(result, ":value"), ":name") {
        Term::Symbol(name) => Some(name.as_str()),
        other => panic!("selected class name must be symbol, got {other:?}"),
    }
}

fn error_code(result: &Term) -> Option<&str> {
    if field(result, ":ok") == &Term::Bool(true) {
        return None;
    }
    match field(result, ":code") {
        Term::Symbol(code) => Some(code.as_str()),
        other => panic!("policy error code must be symbol, got {other:?}"),
    }
}

fn expected_class(class: &PolicyClass, patterns: &[&str], excludes: &[&str]) -> Term {
    let allowed_public_keys = Term::Vector(
        class
            .allowed_public_keys
            .iter()
            .map(|key| Term::Str(Base64::encode_string(key.as_bytes())))
            .collect(),
    );
    let obligation_evidence_kinds = string_map(
        class
            .obligation_evidence_kinds
            .iter()
            .map(|(obligation, kinds)| (obligation.clone(), strings(kinds.iter().cloned()))),
    );
    let role_min_signatures = string_map(
        class
            .role_min_signatures
            .iter()
            .map(|(role, minimum)| (role.clone(), Term::Int((*minimum).into()))),
    );
    let independent_role_pairs = Term::Vector(
        class
            .independent_role_pairs
            .iter()
            .map(|(left, right)| {
                map([
                    (":left", Term::Str(left.clone())),
                    (":right", Term::Str(right.clone())),
                ])
            })
            .collect(),
    );
    map([
        (":allowed-public-keys", allowed_public_keys),
        (":exclude", strings(excludes.iter().copied())),
        (":independent-role-pairs", independent_role_pairs),
        (":min-signatures", Term::Int(class.min_signatures.into())),
        (":name", Term::symbol(format!(":{}", class.name))),
        (":obligation-evidence-kinds", obligation_evidence_kinds),
        (":patterns", strings(patterns.iter().copied())),
        (":require-signatures", Term::Bool(class.require_signatures)),
        (
            ":required-attestation-roles",
            strings(class.required_attestation_roles.iter().cloned()),
        ),
        (
            ":required-evidence-kinds",
            strings(class.required_evidence_kinds.iter().cloned()),
        ),
        (
            ":required-obligations",
            strings(class.required_obligations.iter().cloned()),
        ),
        (":role-min-signatures", role_min_signatures),
    ])
}

fn ok(value: Term) -> Term {
    map([
        (":code", Term::Nil),
        (":ok", Term::Bool(true)),
        (":value", value),
    ])
}

#[test]
fn publish_policy_matches_native_precedence_excludes_and_frozen_refs() {
    let tags = map([
        (":patterns", strings(["refs/tags/**"])),
        (":exclude", strings(["refs/tags/private/**"])),
    ]);
    let main = map([(
        ":patterns",
        strings(["refs/heads/main", "refs/tags/private/**"]),
    )]);
    let dev = class(&["refs/heads/**"]);
    let subject = map([
        (":type", Term::symbol(":vcs/policy")),
        (":v", Term::Int(1_i64.into())),
        (
            ":refs",
            map([(
                ":frozen-prefixes",
                strings(["refs/archive/", "refs/locked/"]),
            )]),
        ),
        (
            ":classes",
            map([(":tags", tags), (":main", main), (":dev", dev)]),
        ),
    ]);
    let native = Policy::from_term(&subject).expect("native policy");
    let mut harness = Harness::new();

    for (refname, expected) in [
        ("refs/tags/v1", Some(":tags")),
        ("refs/tags/private/v1", Some(":main")),
        ("refs/heads/main", Some(":main")),
        ("refs/heads/feature", Some(":dev")),
        ("refs/notes/build", None),
    ] {
        assert_eq!(
            native
                .class_for_ref(refname)
                .map(|class| class.name.as_str()),
            expected.map(|name| name.trim_start_matches(':')),
            "native control for {refname}"
        );
        let result = harness.evaluate(subject.clone(), refname);
        assert_eq!(
            selected_name(&result),
            expected,
            "selfhost selection for {refname}"
        );
        if expected.is_none() {
            assert_eq!(error_code(&result), Some(":no-policy-class"));
        }
    }

    for refname in ["refs/archive/v1", "refs/locked/release"] {
        assert!(native.is_frozen_ref(refname));
        assert_eq!(
            error_code(&harness.evaluate(subject.clone(), refname)),
            Some(":ref-frozen")
        );
    }
}

#[test]
fn publish_policy_normalizes_the_same_class_payload_as_native() {
    let key = Base64::encode_string(&[
        0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
        0x66, 0x66,
    ]);
    let patterns = ["refs/heads/main"];
    let excludes = ["refs/heads/blocked"];
    let rich = map([
        (":patterns", strings(patterns)),
        (":exclude", strings(excludes)),
        (
            ":required-obligations",
            Term::Vector(vec![Term::Str("build".into()), Term::symbol(":lint")]),
        ),
        (
            ":required-evidence-kinds",
            Term::Vector(vec![
                Term::Str("\u{2003}replay\u{2003}".into()),
                Term::symbol(":proof"),
                Term::Str("proof".into()),
            ]),
        ),
        (
            ":obligation-evidence-kinds",
            map([
                (
                    ":build",
                    Term::Vector(vec![Term::Str("sbom".into()), Term::symbol(":proof")]),
                ),
                (
                    ":lint",
                    Term::Vector(vec![Term::Str(" proof ".into()), Term::Str("proof".into())]),
                ),
            ]),
        ),
        (":require-signatures", Term::Bool(true)),
        (":min-signatures", Term::Int(2_i64.into())),
        (":allowed-public-keys", strings([key.clone()])),
        (
            ":required-attestation-roles",
            Term::Vector(vec![
                Term::Str("\u{a0}maintainer\u{a0}".into()),
                Term::symbol(":reviewer"),
                Term::Str(":reviewer".into()),
            ]),
        ),
        (
            ":role-min-signatures",
            map([
                (":maintainer", Term::Int(1_i64.into())),
                (":reviewer", Term::Int(2_i64.into())),
            ]),
        ),
        (
            ":independent-role-pairs",
            Term::Vector(vec![
                map([
                    (":left", Term::Str("reviewer".into())),
                    (":right", Term::symbol(":maintainer")),
                ]),
                map([
                    (":left", Term::symbol(":maintainer")),
                    (":right", Term::Str(":reviewer".into())),
                ]),
            ]),
        ),
    ]);
    let subject = policy(map([(":main", rich)]));
    let native = Policy::from_term(&subject).expect("native rich policy");
    let native_class = native
        .class_for_ref("refs/heads/main")
        .expect("native class");
    let expected = ok(expected_class(native_class, &patterns, &excludes));

    let actual = Harness::new().evaluate(subject, "refs/heads/main");
    assert_eq!(actual, expected);
}

#[test]
fn publish_policy_rejects_native_invalid_policy_corpus_before_selection() {
    let valid_class = class(&["refs/heads/**"]);
    let invalid_subjects = vec![
        Term::Nil,
        map([
            (":type", Term::symbol(":wrong")),
            (":v", Term::Int(1_i64.into())),
            (":classes", map([(":main", valid_class.clone())])),
        ]),
        map([
            (":type", Term::symbol(":vcs/policy")),
            (":v", Term::Int(2_i64.into())),
            (":classes", Term::Map(Default::default())),
        ]),
        policy(map([(
            ":main",
            map([(":patterns", Term::Vector(Vec::new()))]),
        )])),
        policy(map([(
            ":main",
            map([(
                ":patterns",
                Term::Vector(vec![Term::symbol(":not-a-string")]),
            )]),
        )])),
        policy(map([(
            ":main",
            map([
                (":patterns", strings(["refs/heads/**"])),
                (":exclude", Term::Nil),
            ]),
        )])),
        policy(map([
            (":main", valid_class.clone()),
            (":dev", class(&["unrelated[z-a]"])),
        ])),
        policy(map([(
            ":main",
            map([
                (":patterns", strings(["refs/heads/**"])),
                (":require-signatures", Term::Bool(true)),
            ]),
        )])),
        policy(map([(
            ":main",
            map([
                (":patterns", strings(["refs/heads/**"])),
                (":required-attestation-roles", strings(["reviewer"])),
            ]),
        )])),
        policy(map([(
            ":main",
            map([
                (":patterns", strings(["refs/heads/**"])),
                (":min-signatures", Term::Int((-1_i64).into())),
            ]),
        )])),
        policy(map([(
            ":main",
            map([
                (":patterns", strings(["refs/heads/**"])),
                (":allowed-public-keys", strings(["not-base64"])),
            ]),
        )])),
        policy(map([(
            ":main",
            map([
                (":patterns", strings(["refs/heads/**"])),
                (":required-attestation-roles", Term::Vector(Vec::new())),
            ]),
        )])),
        policy(map([(
            ":main",
            map([
                (":patterns", strings(["refs/heads/**"])),
                (
                    ":independent-role-pairs",
                    Term::Vector(vec![map([
                        (":left", Term::Str("reviewer".into())),
                        (":right", Term::symbol(":reviewer")),
                    ])]),
                ),
            ]),
        )])),
        policy(map([(
            ":main",
            map([
                (":patterns", strings(["refs/heads/**"])),
                (
                    ":role-min-signatures",
                    map([(":reviewer", Term::Str("not-an-integer".into()))]),
                ),
            ]),
        )])),
        map([
            (":type", Term::symbol(":vcs/policy")),
            (":v", Term::Int(1_i64.into())),
            (
                ":refs",
                map([(
                    ":frozen-prefixes",
                    Term::Vector(vec![Term::Int(1_i64.into())]),
                )]),
            ),
            (":classes", map([(":main", valid_class)])),
        ]),
    ];
    let mut harness = Harness::new();
    for (index, subject) in invalid_subjects.into_iter().enumerate() {
        assert!(
            Policy::from_term(&subject).is_err(),
            "native negative control {index}"
        );
        assert_eq!(
            error_code(&harness.evaluate(subject, "refs/heads/main")),
            Some(":bad-policy"),
            "selfhost negative control {index}"
        );
    }
}
