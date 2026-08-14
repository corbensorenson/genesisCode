use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_prelude::{
    SelfhostBootstrapMode, build_prelude, load_selfhost_coreform_toolchain_v1_with_mode,
};

const BINDING: &str = "selfhost/pkg-publish-glob::match";
const VALID_BINDING: &str = "selfhost/pkg-publish-glob::valid?";

struct Harness {
    context: EvalCtx,
    matcher: Value,
    validator: Value,
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
        let mut context = EvalCtx::with_step_limit(Some(20_000_000));
        context.set_mem_limits(MemLimits {
            max_alloc_units: Some(80_000_000),
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
        let matcher = environment.get(BINDING).expect("publish glob binding");
        let validator = environment
            .get(VALID_BINDING)
            .expect("publish glob validator binding");
        context.reset_counters();
        Self {
            context,
            matcher,
            validator,
        }
    }

    fn evaluate(&mut self, pattern: &str, value: &str) -> Term {
        self.context.reset_counters();
        let partial = self
            .matcher
            .clone()
            .apply(
                &mut self.context,
                Value::data(Term::Str(pattern.to_string())),
            )
            .expect("apply publish glob pattern");
        partial
            .apply(&mut self.context, Value::data(Term::Str(value.to_string())))
            .expect("apply publish glob value")
            .to_plain_term()
            .expect("plain publish glob result")
    }

    fn valid(&mut self, pattern: &str) -> bool {
        self.context.reset_counters();
        self.validator
            .clone()
            .apply(
                &mut self.context,
                Value::data(Term::Str(pattern.to_string())),
            )
            .expect("apply publish glob validator")
            .to_plain_term()
            .expect("plain publish glob validity")
            == Term::Bool(true)
    }
}

fn result(ok: bool, value: bool) -> Term {
    Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(ok)),
            (TermOrdKey(Term::symbol(":value")), Term::Bool(value)),
        ]
        .into_iter()
        .collect(),
    )
}

fn term_map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value))
            .collect(),
    )
}

fn native_match(pattern: &str, value: &str) -> Result<bool, gc_vcs::PolicyError> {
    let class = term_map([
        (
            ":patterns",
            Term::Vector(vec![Term::Str(pattern.to_string())]),
        ),
        (":required-obligations", Term::Vector(Vec::new())),
    ]);
    let policy = term_map([
        (":type", Term::symbol(":vcs/policy")),
        (":v", Term::Int(1_i64.into())),
        (":classes", term_map([(":tags", class)])),
    ]);
    gc_vcs::Policy::from_term(&policy).map(|policy| policy.class_for_ref(value).is_some())
}

#[test]
fn publish_glob_matches_portable_ref_grammar() {
    let mut harness = Harness::new();
    for (pattern, value, expected) in [
        ("refs/heads/*", "refs/heads/main", true),
        ("refs/heads/*", "refs/tags/v1", false),
        ("refs/?ags/v1", "refs/tags/v1", true),
        ("refs/[hm]eads/*", "refs/heads/x", true),
        ("refs/[a-z]ags/*", "refs/tags/v1", true),
        ("refs/[!t]ags/*", "refs/tags/v1", false),
        ("refs/{heads,tags}/*", "refs/tags/v1", true),
        ("refs/{heads,{tags,releases}}/*", "refs/releases/v1", true),
        (r"refs/heads/\*", "refs/heads/*", true),
        ("refs/?/*", "refs/λ/x", false),
        ("refs/??/*", "refs/λ/x", true),
        ("refs/*", "refs/heads/nested/main", true),
    ] {
        assert_eq!(
            harness.evaluate(pattern, value),
            result(true, expected),
            "{pattern}"
        );
    }
}

#[test]
fn publish_glob_rejects_malformed_patterns() {
    let mut harness = Harness::new();
    for pattern in ["refs/[bad", "refs/[z-a]", "refs/{heads", r"refs/heads/\"] {
        assert!(!harness.valid(pattern), "{pattern:?}");
        assert_eq!(
            harness.evaluate(pattern, "refs/heads/main"),
            result(false, false)
        );
    }
    for pattern in ["unrelated[", "unrelated{bad", "unrelated[z-a]"] {
        assert!(!harness.valid(pattern), "{pattern:?}");
        assert_eq!(
            harness.evaluate(pattern, "refs/heads/main"),
            result(true, false),
            "ordinary matching must not substitute for structural validation"
        );
    }
}

#[test]
fn publish_glob_matches_native_globset_edge_corpus() {
    let mut harness = Harness::new();
    let mut mismatches = Vec::new();
    for (pattern, value) in [
        ("", ""),
        ("", "refs/heads/main"),
        ("*", ""),
        ("*", "refs/heads/main"),
        ("**", "refs/heads/main"),
        ("***", "refs/heads/main"),
        ("refs/**/main", "refs/heads/main"),
        ("refs/**/main", "refs/main"),
        ("**/main", "main"),
        ("**/main", "refs/heads/main"),
        ("refs/**", "refs"),
        ("refs/**", "refs/"),
        ("a**/main", "amain"),
        ("a**/main", "a/heads/main"),
        ("refs/*/main", "refs/a/b/main"),
        ("refs/?/main", "refs/λ/main"),
        ("refs/?/main", "refs/ab/main"),
        ("?", "λ"),
        ("??", "λ"),
        ("???", "λ"),
        ("*?", "λ"),
        ("?*", "λ"),
        (r"refs/\?/main", "refs/?/main"),
        (r"\λ", "λ"),
        (r"refs/\[main\]", "refs/[main]"),
        ("refs/[abc]/main", "refs/b/main"),
        ("refs/[!abc]/main", "refs/z/main"),
        ("refs/[0-9]/main", "refs/5/main"),
        ("[λ]", "λ"),
        ("[λπ]", "λ"),
        ("[!λ]", "π"),
        ("refs/[λ-ω]/main", "refs/π/main"),
        ("refs/{heads,tags}/main", "refs/heads/main"),
        ("refs/{heads,tags}/main", "refs/dev/main"),
        ("refs/{heads,{tags,releases}}/*", "refs/tags/v1"),
        (r"refs/\{heads,tags\}/*", "refs/{heads,tags}/v1"),
        ("refs/{heads,}/main", "refs//main"),
        ("refs/{heads,}/main", "refs/heads/main"),
        ("refs/{,heads}/main", "refs/heads/main"),
        ("refs/{,heads}/main", "refs//main"),
        ("refs/{heads}/main", "refs/heads/main"),
        ("{,}", ""),
        ("{}", ""),
        ("{a,,b}", "b"),
        ("{a,,b}", ""),
        ("refs/[z-a]/main", "refs/z/main"),
        ("refs/[abc/main", "refs/a/main"),
        (r"refs/heads/\", "refs/heads/"),
        ("refs/{heads,tags/main", "refs/heads/main"),
        ("refs/heads/main}", "refs/heads/main}"),
    ] {
        let expected = native_match(pattern, value);
        let actual = harness.evaluate(pattern, value);
        assert_eq!(harness.valid(pattern), expected.is_ok(), "{pattern:?}");
        let expected = match expected {
            Ok(expected) => result(true, expected),
            Err(_) => result(false, false),
        };
        if actual != expected {
            mismatches.push(format!(
                "{pattern:?} {value:?}: actual={actual:?} expected={expected:?}"
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "native globset parity mismatches:\n{}",
        mismatches.join("\n")
    );
}
