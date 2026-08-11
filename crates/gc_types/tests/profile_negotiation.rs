use std::collections::BTreeSet;
use std::path::Path;

use gc_coreform::{Term, canonicalize_module, parse_module};
use gc_types::profile_negotiation::{
    COREFORM_ARTIFACT_PROFILE_ID, HOST_ABI_CAPABILITY_PROFILE_ID, PORTABLE_HOST_TARGET_PROFILE_ID,
    PROFILE_FAMILY_ARTIFACT, PROFILE_FAMILY_CAPABILITY, PROFILE_FAMILY_LANGUAGE,
    PROFILE_FAMILY_TARGET, PROFILE_NEGOTIATION_PROFILE_ID, PURE_CAPABILITY_PROFILE_ID,
};
use gc_types::{
    ModuleForTypecheck, ProfileOffer, negotiate_package_profiles, typecheck_package,
    typecheck_package_with_profile_offer,
};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

const PROFILE: &str = include_str!("../../../docs/spec/PROFILE_NEGOTIATION_v0.1.json");
const SPEC: &[u8] = include_bytes!("../../../docs/spec/PROFILE_NEGOTIATION_v0.1.md");
const SCHEMA: &[u8] = include_bytes!("../../../docs/spec/PROFILE_NEGOTIATION_v0.1.schema.json");

const MODULE_PROFILE: &str = r#"
  :module-profile genesis/module-resolution-profile-v0.1
  :requires-profiles {
    genesis/coreform-profile genesis/coreform/v0.2
    genesis/hash-profile genesis/hash-profile/gcv0.2-blake3
    genesis/language-profile genesis/language-profile/v0.2
    genesis/module-resolution-profile genesis/module-resolution-profile-v0.1}
"#;

fn requirements(capability_mode: &str, capability: &str, target: &str) -> String {
    format!(
        r#"
          :profile-negotiation genesis/profile-negotiation-v0.1
          :package-profile-requirements {{
            genesis/profile-family/language {{:mode exact :profile genesis/language-profile/v0.2}}
            genesis/profile-family/capability {{:mode {capability_mode} :profile {capability}}}
            genesis/profile-family/artifact {{:mode exact :profile genesis/artifact-profile/coreform-v0.2}}
            genesis/profile-family/target {{:mode exact :profile {target}}}}}
        "#
    )
}

fn module(path: &str, symbol: &str, requirements: &str, caps: &str) -> ModuleForTypecheck {
    module_from_metadata(
        path,
        &format!(
            "{MODULE_PROFILE} {requirements} :imports [] :exports [{symbol}] :caps {caps} :types {{{symbol} Int}} :strict-shapes true :strict-effects true"
        ),
        &format!("(def {symbol} 1)"),
    )
}

fn module_from_metadata(path: &str, metadata: &str, body: &str) -> ModuleForTypecheck {
    let source = format!("(def ::meta '{{{metadata}}})\n{body}");
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

fn core_offer_with_capabilities(capabilities: &[&str]) -> ProfileOffer {
    let mut profiles = vec![
        (
            PROFILE_FAMILY_LANGUAGE.to_string(),
            "genesis/language-profile/v0.2".to_string(),
        ),
        (
            PROFILE_FAMILY_ARTIFACT.to_string(),
            COREFORM_ARTIFACT_PROFILE_ID.to_string(),
        ),
        (
            PROFILE_FAMILY_TARGET.to_string(),
            PORTABLE_HOST_TARGET_PROFILE_ID.to_string(),
        ),
    ];
    profiles.extend(capabilities.iter().map(|profile| {
        (
            PROFILE_FAMILY_CAPABILITY.to_string(),
            (*profile).to_string(),
        )
    }));
    ProfileOffer::from_profiles(profiles).expect("valid test offer")
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
            "compatibility",
            "contentIdentitySha256",
            "failure",
            "identity",
            "kind",
            "nonclaims",
            "offer",
            "packageClosure",
            "profiles",
            "schema",
            "schemaSha256",
            "sourceBindings",
            "version",
        ]
    );
    assert_eq!(profile["kind"], PROFILE_NEGOTIATION_PROFILE_ID);
    assert_eq!(profile["canonicalSpecSha256"], sha256_hex(SPEC));
    assert_eq!(profile["schemaSha256"], sha256_hex(SCHEMA));
    assert_eq!(profile["contentIdentitySha256"], content_identity(&profile));
    assert_eq!(profile["compatibility"]["versionInference"], "forbidden");
    assert_eq!(
        profile["offer"]["meaning"],
        "verified-implementation-availability-not-authorization"
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
fn exact_and_minimum_select_the_least_offered_compatible_member() {
    let minimum_pure = requirements(
        "minimum",
        PURE_CAPABILITY_PROFILE_ID,
        PORTABLE_HOST_TARGET_PROFILE_ID,
    );
    let package = module("pkg/main.gc", "pkg/main::value", &minimum_pure, "[]");

    let default_report = typecheck_package(std::slice::from_ref(&package));
    assert!(default_report.ok, "{:?}", default_report.errors);
    assert_eq!(
        default_report
            .profile_negotiation
            .selected_profiles
            .get(PROFILE_FAMILY_CAPABILITY)
            .map(String::as_str),
        Some(PURE_CAPABILITY_PROFILE_ID)
    );
    let identity = default_report
        .profile_negotiation
        .negotiation_identity
        .expect("negotiated identity");
    let Term::Map(machine_report) = default_report.to_term() else {
        panic!("typecheck report must be a map")
    };
    let Some(Term::Map(machine_negotiation)) = machine_report.get(&gc_coreform::TermOrdKey(
        Term::symbol(":profile-negotiation"),
    )) else {
        panic!("typecheck report must expose negotiation")
    };
    assert_eq!(
        machine_negotiation.get(&gc_coreform::TermOrdKey(Term::symbol(":identity"))),
        Some(&Term::Bytes(identity.to_vec().into()))
    );

    let pure_only = core_offer_with_capabilities(&[PURE_CAPABILITY_PROFILE_ID]);
    let pure_report =
        typecheck_package_with_profile_offer(std::slice::from_ref(&package), &pure_only);
    assert!(pure_report.ok, "{:?}", pure_report.errors);
    assert_eq!(
        pure_report.profile_negotiation.negotiation_identity,
        Some(identity),
        "unselected offer members must not change identity"
    );

    let host_only = core_offer_with_capabilities(&[HOST_ABI_CAPABILITY_PROFILE_ID]);
    let host_report = typecheck_package_with_profile_offer(&[package], &host_only);
    assert!(host_report.ok, "{:?}", host_report.errors);
    assert_eq!(
        host_report
            .profile_negotiation
            .selected_profiles
            .get(PROFILE_FAMILY_CAPABILITY)
            .map(String::as_str),
        Some(HOST_ABI_CAPABILITY_PROFILE_ID)
    );
    assert_ne!(
        host_report.profile_negotiation.negotiation_identity,
        Some(identity)
    );
}

#[test]
fn exact_does_not_accept_a_later_compatible_member() {
    let exact_pure = requirements(
        "exact",
        PURE_CAPABILITY_PROFILE_ID,
        PORTABLE_HOST_TARGET_PROFILE_ID,
    );
    let package = module("pkg/main.gc", "pkg/main::value", &exact_pure, "[]");
    let host_only = core_offer_with_capabilities(&[HOST_ABI_CAPABILITY_PROFILE_ID]);
    let report = typecheck_package_with_profile_offer(&[package], &host_only);

    assert!(!report.ok);
    assert!(report.profile_negotiation.negotiation_identity.is_none());
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("unsupported exact profile requirement"))
    );
    let Term::Map(machine_report) = report.to_term() else {
        panic!("typecheck report must be a map")
    };
    let Some(Term::Map(machine_negotiation)) = machine_report.get(&gc_coreform::TermOrdKey(
        Term::symbol(":profile-negotiation"),
    )) else {
        panic!("typecheck report must expose negotiation")
    };
    assert_eq!(
        machine_negotiation.get(&gc_coreform::TermOrdKey(Term::symbol(":identity"))),
        Some(&Term::Nil)
    );
}

#[test]
fn unsupported_declared_target_fails_before_execution() {
    let unsupported = requirements(
        "exact",
        PURE_CAPABILITY_PROFILE_ID,
        "genesis/target-profile/browser-v9",
    );
    let package = module("pkg/main.gc", "pkg/main::value", &unsupported, "[]");

    let report = typecheck_package(&[package]);
    assert!(!report.ok, "unsupported target profile was ignored");
    assert!(report.profile_negotiation.negotiation_identity.is_none());
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("browser-v9")),
        "negotiation error must identify the unavailable profile: {:?}",
        report.errors
    );
}

#[test]
fn capability_availability_never_grants_or_hides_declared_caps() {
    let pure = requirements(
        "exact",
        PURE_CAPABILITY_PROFILE_ID,
        PORTABLE_HOST_TARGET_PROFILE_ID,
    );
    let impure = module("pkg/main.gc", "pkg/main::value", &pure, "[io/fs::read]");
    let denied = typecheck_package(&[impure]);
    assert!(!denied.ok);
    assert!(denied.errors.iter().any(|error| {
        error.contains(PURE_CAPABILITY_PROFILE_ID) && error.contains("io/fs::read")
    }));

    let malformed = module("pkg/main.gc", "pkg/main::value", &pure, "[io/fs::read 1]");
    let malformed_report = typecheck_package(&[malformed]);
    assert!(!malformed_report.ok);
    assert!(
        malformed_report
            .errors
            .iter()
            .any(|error| error.contains(":caps entries to be symbols"))
    );

    let host = requirements(
        "exact",
        HOST_ABI_CAPABILITY_PROFILE_ID,
        PORTABLE_HOST_TARGET_PROFILE_ID,
    );
    let declared = module("pkg/main.gc", "pkg/main::value", &host, "[io/fs::read]");
    let accepted = typecheck_package(&[declared]);
    assert!(accepted.ok, "{:?}", accepted.errors);
    assert_eq!(
        accepted
            .profile_negotiation
            .selected_profiles
            .get(PROFILE_FAMILY_CAPABILITY)
            .map(String::as_str),
        Some(HOST_ABI_CAPABILITY_PROFILE_ID)
    );
}

#[test]
fn closure_must_be_complete_identical_and_module_resolved() {
    let pure = requirements(
        "exact",
        PURE_CAPABILITY_PROFILE_ID,
        PORTABLE_HOST_TARGET_PROFILE_ID,
    );
    let host = requirements(
        "exact",
        HOST_ABI_CAPABILITY_PROFILE_ID,
        PORTABLE_HOST_TARGET_PROFILE_ID,
    );
    let a = module("pkg/a.gc", "pkg/a::value", &pure, "[]");
    let b = module("pkg/b.gc", "pkg/b::value", &host, "[]");
    let drift = typecheck_package(&[a.clone(), b]);
    assert!(!drift.ok);
    assert!(drift.profile_negotiation.negotiation_identity.is_none());
    assert!(
        drift
            .errors
            .iter()
            .any(|error| error.contains("different :package-profile-requirements"))
    );

    let unprofiled = module_from_metadata(
        "pkg/b.gc",
        &format!(
            "{MODULE_PROFILE} :imports [] :exports [pkg/b::value] :caps [] :types {{pkg/b::value Int}} :strict-shapes true :strict-effects true"
        ),
        "(def pkg/b::value 1)",
    );
    let incomplete = typecheck_package(&[a, unprofiled]);
    assert!(!incomplete.ok);
    assert!(incomplete.errors.iter().any(|error| {
        error.contains("every module in the package closure must declare :profile-negotiation")
    }));

    let no_resolution = module_from_metadata(
        "pkg/main.gc",
        &format!(
            "{pure} :exports [pkg/main::value] :caps [] :types {{pkg/main::value Int}} :strict-shapes true :strict-effects true"
        ),
        "(def pkg/main::value 1)",
    );
    let unresolved = typecheck_package(&[no_resolution]);
    assert!(!unresolved.ok);
    assert!(
        unresolved
            .errors
            .iter()
            .any(|error| error.contains("requires successful module resolution"))
    );
}

#[test]
fn malformed_requirements_and_offers_fail_closed_without_version_inference() {
    let malformed = requirements(
        "compatible",
        PURE_CAPABILITY_PROFILE_ID,
        PORTABLE_HOST_TARGET_PROFILE_ID,
    );
    let package = module("pkg/main.gc", "pkg/main::value", &malformed, "[]");
    let report = typecheck_package(&[package]);
    assert!(!report.ok);
    assert!(report.profile_negotiation.negotiation_identity.is_none());
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("unsupported requirement mode compatible"))
    );

    let unknown_family = ProfileOffer::from_profiles([(
        "genesis/profile-family/future".to_string(),
        "genesis/future/v1".to_string(),
    )]);
    assert_eq!(
        unknown_family.expect_err("unknown family must fail"),
        "unknown profile family genesis/profile-family/future"
    );
    let inferred_version = ProfileOffer::from_profiles([(
        PROFILE_FAMILY_LANGUAGE.to_string(),
        "genesis/language-profile/v0.3".to_string(),
    )]);
    assert!(
        inferred_version
            .expect_err("unknown version must fail")
            .contains("is not registered")
    );
    let duplicate = ProfileOffer::from_profiles([
        (
            PROFILE_FAMILY_LANGUAGE.to_string(),
            "genesis/language-profile/v0.2".to_string(),
        ),
        (
            PROFILE_FAMILY_LANGUAGE.to_string(),
            "genesis/language-profile/v0.2".to_string(),
        ),
    ]);
    assert!(
        duplicate
            .expect_err("duplicate offer must fail")
            .contains("duplicate offered profile")
    );
}

#[test]
fn legacy_package_is_explicitly_inactive_and_has_no_negotiated_identity() {
    let legacy = module_from_metadata(
        "legacy.gc",
        ":exports [legacy::value] :caps [] :types {legacy::value Int} :strict-shapes true :strict-effects true",
        "(def legacy::value 1)",
    );
    let negotiation =
        negotiate_package_profiles(std::slice::from_ref(&legacy), &ProfileOffer::core_host());
    assert!(!negotiation.active);
    assert!(negotiation.ok);
    assert!(negotiation.negotiation_identity.is_none());
    assert!(typecheck_package(&[legacy]).ok);
}
