use std::collections::BTreeSet;
use std::path::Path;

use gc_coreform::{Term, canonicalize_module, parse_module};
use gc_types::{MODULE_RESOLUTION_PROFILE_ID, ModuleForTypecheck, resolve_module_profile};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

const PROFILE: &str = include_str!("../../../docs/spec/MODULE_RESOLUTION_PROFILE_v0.1.json");
const SPEC: &[u8] = include_bytes!("../../../docs/spec/MODULE_RESOLUTION_PROFILE_v0.1.md");
const SCHEMA: &[u8] =
    include_bytes!("../../../docs/spec/MODULE_RESOLUTION_PROFILE_v0.1.schema.json");

const PROFILE_BINDINGS: &str = r#"
  :module-profile genesis/module-resolution-profile-v0.1
  :requires-profiles {
    genesis/coreform-profile genesis/coreform/v0.2
    genesis/hash-profile genesis/hash-profile/gcv0.2-blake3
    genesis/language-profile genesis/language-profile/v0.2
    genesis/module-resolution-profile genesis/module-resolution-profile-v0.1}
"#;

fn module(path: &str, metadata: &str, body: &str) -> ModuleForTypecheck {
    let source = format!("(def ::meta '{{{PROFILE_BINDINGS} {metadata}}})\n{body}");
    let forms = canonicalize_module(parse_module(&source).expect("parse module"))
        .expect("canonicalize module");
    let meta = forms.iter().find_map(|term| {
        let items = term.as_proper_list()?;
        if items.len() != 3 || !matches!(&items[1], Term::Symbol(name) if name == "::meta") {
            return None;
        }
        let quote = items[2].as_proper_list()?;
        (quote.len() == 2 && matches!(&quote[0], Term::Symbol(name) if name == "quote"))
            .then(|| quote[1].clone())
    });
    ModuleForTypecheck {
        path: path.to_string(),
        forms,
        meta,
    }
}

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

#[test]
fn machine_profile_is_closed_content_addressed_and_source_bound() {
    let profile: JsonValue = serde_json::from_str(PROFILE).expect("valid profile JSON");
    let keys = profile
        .as_object()
        .expect("profile object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "auditDate",
            "canonicalSpec",
            "canonicalSpecSha256",
            "contentIdentitySha256",
            "cycles",
            "identity",
            "imports",
            "kind",
            "modules",
            "nonclaims",
            "packageBoundaries",
            "profiles",
            "schema",
            "schemaSha256",
            "sourceBindings",
            "version",
            "visibility",
            "workspaceOverrides",
        ]
    );
    assert_eq!(profile["kind"], MODULE_RESOLUTION_PROFILE_ID);
    assert_eq!(profile["canonicalSpecSha256"], sha256_hex(SPEC));
    assert_eq!(profile["schemaSha256"], sha256_hex(SCHEMA));
    assert_eq!(profile["contentIdentitySha256"], content_identity(&profile));
    assert_eq!(
        profile["cycles"]["proof"],
        "every-accepted-local-edge-targets-lower-manifest-index"
    );
    assert_eq!(
        profile["workspaceOverrides"]["ambient"],
        "unsupported-and-rejected"
    );
    assert_eq!(
        profile["profiles"]["bindings"]["genesis/module-resolution-profile"],
        MODULE_RESOLUTION_PROFILE_ID
    );

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
fn valid_graph_is_content_addressed_and_manifest_order_bound() {
    let provider = module(
        "a.gc",
        ":imports [] :exports [pkg/a::value] :caps [] :types {pkg/a::value Int}",
        "(def pkg/a::value 1)\npkg/a::value",
    );
    let consumer = module(
        "b.gc",
        ":imports [pkg/a::value] :exports [pkg/b::value] :caps [] :types {pkg/b::value Int}",
        "(def pkg/b::value pkg/a::value)\npkg/b::value",
    );

    let report = resolve_module_profile(&[provider.clone(), consumer.clone()]);
    assert!(report.active);
    assert!(report.ok, "{:?}", report.errors_by_module);
    assert_eq!(report.resolution_order, ["a.gc", "b.gc"]);
    assert_eq!(
        report.resolution_identity,
        resolve_module_profile(&[provider.clone(), consumer.clone()]).resolution_identity
    );

    let changed_consumer = module(
        "b.gc",
        ":imports [pkg/a::value] :exports [pkg/b::value] :caps [] :types {pkg/b::value Int}",
        "(def pkg/b::value (prim int/add pkg/a::value 1))\npkg/b::value",
    );
    assert_ne!(
        report.resolution_identity,
        resolve_module_profile(&[provider.clone(), changed_consumer]).resolution_identity
    );

    let reversed = resolve_module_profile(&[consumer, provider]);
    assert!(!reversed.ok);
    assert!(reversed.errors_by_module["b.gc"].iter().any(|error| {
        error.contains("later module a.gc") && error.contains("cycles and forward imports")
    }));
}

#[test]
fn private_cross_module_access_fails_even_when_declared() {
    let provider = module(
        "a.gc",
        ":imports [] :exports [pkg/a::public] :caps [] :types {pkg/a::public Int}",
        "(def pkg/a::private 1)\n(def pkg/a::public pkg/a::private)\npkg/a::public",
    );
    let consumer = module(
        "b.gc",
        ":imports [pkg/a::private] :exports [pkg/b::value] :caps [] :types {pkg/b::value Int}",
        "(def pkg/b::value pkg/a::private)\npkg/b::value",
    );

    let report = resolve_module_profile(&[provider, consumer]);
    assert!(!report.ok);
    let errors = &report.errors_by_module["b.gc"];
    assert!(
        errors
            .iter()
            .any(|error| error.contains("import pkg/a::private is private"))
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("crosses the private boundary"))
    );
}

#[test]
fn lexical_shadowing_does_not_create_an_ambient_import() {
    let provider = module(
        "a.gc",
        ":imports [] :exports [pkg/a::call] :caps [] :types {pkg/a::call ?}",
        "(def pkg/a::call (fn (_) 1))\npkg/a::call",
    );
    let consumer = module(
        "b.gc",
        ":imports [] :exports [pkg/b::apply] :caps [] :types {pkg/b::apply ?}",
        "(def pkg/b::apply (fn (pkg/a::call) (pkg/a::call nil)))\npkg/b::apply",
    );

    let report = resolve_module_profile(&[provider, consumer]);
    assert!(report.ok, "{:?}", report.errors_by_module);
}

#[test]
fn cycles_duplicate_ownership_and_profile_drift_fail_closed() {
    let a = module(
        "a.gc",
        ":imports [pkg/b::value] :exports [pkg/a::value pkg/shared::value] :caps [] :types {pkg/a::value Int pkg/shared::value Int}",
        "(def pkg/a::value pkg/b::value)\n(def pkg/shared::value 1)\npkg/a::value",
    );
    let b = module(
        "b.gc",
        ":imports [pkg/a::value] :exports [pkg/b::value pkg/shared::value] :caps [] :types {pkg/b::value Int pkg/shared::value Int}",
        "(def pkg/b::value pkg/a::value)\n(def pkg/shared::value 2)\npkg/b::value",
    );
    let report = resolve_module_profile(&[a, b]);
    assert!(!report.ok);
    assert!(report.errors_by_module["a.gc"].iter().any(|error| {
        error.contains("later module b.gc") && error.contains("cycles and forward imports")
    }));
    assert!(
        report.errors_by_module["a.gc"]
            .iter()
            .any(|error| error.contains("multiple module owners"))
    );
    assert!(report.resolution_identity.is_none());

    let drifted_source = format!(
        "(def ::meta '{{:module-profile {MODULE_RESOLUTION_PROFILE_ID} :requires-profiles {{genesis/coreform-profile genesis/coreform/v9}} :imports [] :exports [pkg/x::value] :caps [] :types {{pkg/x::value Int}}}}) (def pkg/x::value 1)"
    );
    let forms = canonicalize_module(parse_module(&drifted_source).unwrap()).unwrap();
    let drifted = ModuleForTypecheck {
        path: "drift.gc".to_string(),
        meta: forms
            .first()
            .and_then(Term::as_proper_list)
            .and_then(|items| items[2].as_proper_list())
            .map(|quote| quote[1].clone()),
        forms,
    };
    let report = resolve_module_profile(&[drifted]);
    assert!(!report.ok);
    assert!(
        report.errors_by_module["drift.gc"]
            .iter()
            .any(|error| error.contains(":requires-profiles must exactly bind"))
    );
}

#[test]
fn nonportable_paths_and_unqualified_interface_symbols_fail_closed() {
    let invalid = module(
        "../escape.gc",
        ":imports [] :exports [unqualified] :caps [] :types {unqualified Int}",
        "(def unqualified 1)\nunqualified",
    );
    let report = resolve_module_profile(&[invalid]);
    assert!(!report.ok);
    let errors = &report.errors_by_module["../escape.gc"];
    assert!(errors.iter().any(|error| error.contains("'..' components")));
    assert!(
        errors
            .iter()
            .any(|error| error.contains("expected namespace::name"))
    );
}

#[test]
fn independent_manifest_order_model_matches_every_three_module_permutation() {
    let a = module(
        "a.gc",
        ":imports [] :exports [pkg/a::value] :caps [] :types {pkg/a::value Int}",
        "(def pkg/a::value 1)\npkg/a::value",
    );
    let b = module(
        "b.gc",
        ":imports [pkg/a::value] :exports [pkg/b::value] :caps [] :types {pkg/b::value Int}",
        "(def pkg/b::value pkg/a::value)\npkg/b::value",
    );
    let c = module(
        "c.gc",
        ":imports [pkg/b::value] :exports [pkg/c::value] :caps [] :types {pkg/c::value Int}",
        "(def pkg/c::value pkg/b::value)\npkg/c::value",
    );
    let base = [a, b, c];
    for permutation in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let ordered = permutation
            .iter()
            .map(|index| base[*index].clone())
            .collect::<Vec<_>>();
        // Independent specification model: b requires a and c requires b, so
        // acceptance is exactly the strict manifest order a < b < c.
        let position = |module_index| {
            permutation
                .iter()
                .position(|candidate| *candidate == module_index)
                .expect("permutation contains every module")
        };
        let expected = position(0) < position(1) && position(1) < position(2);
        let report = resolve_module_profile(&ordered);
        assert_eq!(
            report.ok, expected,
            "permutation {permutation:?}: {:?}",
            report.errors_by_module
        );
        assert_eq!(report.resolution_identity.is_some(), expected);
    }
}
