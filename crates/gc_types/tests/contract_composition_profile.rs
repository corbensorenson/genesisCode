use std::collections::BTreeSet;
use std::path::Path;

use gc_coreform::{Term, canonicalize_module, parse_module};
use gc_types::{
    CONTRACT_COMPOSITION_PROFILE_ID, MODULE_RESOLUTION_PROFILE_ID, ModuleForTypecheck,
    compose_contract_profile, typecheck_package,
};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

const PROFILE: &str = include_str!("../../../docs/spec/CONTRACT_COMPOSITION_PROFILE_v0.1.json");
const SPEC: &[u8] = include_bytes!("../../../docs/spec/CONTRACT_COMPOSITION_PROFILE_v0.1.md");
const SCHEMA: &[u8] =
    include_bytes!("../../../docs/spec/CONTRACT_COMPOSITION_PROFILE_v0.1.schema.json");

const BASE_PROFILE: &str = r#"
  :module-profile genesis/module-resolution-profile-v0.1
  :requires-profiles {
    genesis/coreform-profile genesis/coreform/v0.2
    genesis/hash-profile genesis/hash-profile/gcv0.2-blake3
    genesis/language-profile genesis/language-profile/v0.2
    genesis/module-resolution-profile genesis/module-resolution-profile-v0.1}
  :contract-composition-profile genesis/contract-composition-profile-v0.1
  :strict-shapes true
  :strict-effects true
"#;

fn module(path: &str, metadata: &str, body: &str) -> ModuleForTypecheck {
    let source = format!("(def ::meta '{{{BASE_PROFILE} {metadata}}})\n{body}");
    module_from_source(path, &source)
}

fn module_from_source(path: &str, source: &str) -> ModuleForTypecheck {
    let forms = canonicalize_module(parse_module(source).expect("parse module"))
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
            "blame",
            "canonicalSpec",
            "canonicalSpecSha256",
            "compatibility",
            "contentIdentitySha256",
            "effects",
            "identity",
            "kind",
            "metadata",
            "nonclaims",
            "optimization",
            "parametricity",
            "profiles",
            "refinements",
            "runtimeContracts",
            "schema",
            "schemaSha256",
            "sourceBindings",
            "version",
        ]
    );
    assert_eq!(profile["kind"], CONTRACT_COMPOSITION_PROFILE_ID);
    assert_eq!(profile["canonicalSpecSha256"], sha256_hex(SPEC));
    assert_eq!(profile["schemaSha256"], sha256_hex(SCHEMA));
    assert_eq!(profile["contentIdentitySha256"], content_identity(&profile));
    assert_eq!(
        profile["compatibility"]["functionParameters"],
        "contravariant"
    );
    assert_eq!(profile["refinements"]["supported"], "empty-set-only");
    assert_eq!(
        profile["runtimeContracts"]["separation"],
        "runtime-and-static-identity-domains-never-interchangeable"
    );
    assert_eq!(
        profile["profiles"]["moduleResolution"],
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
fn closed_pure_interface_is_identified_and_optimization_eligible() {
    let subject = module(
        "math.gc",
        ":imports [] :exports [pkg/math::inc] :caps [] :refinements {pkg/math::inc []} :types {pkg/math::inc (Fn Int Int (Eff [] nil))}",
        "(def pkg/math::inc (fn (x) (prim int/add x 1)))\npkg/math::inc",
    );
    let composition = compose_contract_profile(std::slice::from_ref(&subject));
    assert!(composition.active);
    assert!(composition.ok, "{:?}", composition.errors_by_module);
    assert!(composition.profile_identity.is_some());
    let export = &composition.exports["pkg/math::inc"];
    assert!(export.optimization.eligible);
    assert_eq!(export.blame.provider, "math.gc#pkg/math::inc".to_string());
    assert_eq!(
        composition.profile_identity,
        compose_contract_profile(std::slice::from_ref(&subject)).profile_identity
    );

    let typecheck = typecheck_package(&[subject]);
    assert!(typecheck.ok, "{:?}", typecheck.errors);
}

#[test]
fn rank1_effect_rows_are_alpha_normalized_but_not_optimizer_eligible() {
    let with_variable = |variable: &str| {
        module(
            "carry.gc",
            &format!(
                ":imports [] :exports [pkg/effect::carry] :caps [] :refinements {{pkg/effect::carry []}} :types {{pkg/effect::carry (Fn (Prog Int (Eff [] {variable})) (Prog Int (Eff [] {variable})) (Eff [] nil))}}"
            ),
            "(def pkg/effect::carry (fn (program) program))\npkg/effect::carry",
        )
    };
    let first = with_variable("e");
    let renamed = with_variable("renamed");
    let first_report = compose_contract_profile(std::slice::from_ref(&first));
    let renamed_report = compose_contract_profile(std::slice::from_ref(&renamed));
    assert!(first_report.ok, "{:?}", first_report.errors_by_module);
    assert!(renamed_report.ok, "{:?}", renamed_report.errors_by_module);
    let first_export = &first_report.exports["pkg/effect::carry"];
    let renamed_export = &renamed_report.exports["pkg/effect::carry"];
    assert_eq!(first_export.shape_identity, renamed_export.shape_identity);
    assert_eq!(
        first_export.interface_identity,
        renamed_export.interface_identity
    );
    assert_eq!(first_export.effect_row_variables, ["e"]);
    assert!(!first_export.optimization.closed_effects);
    assert!(!first_export.optimization.monomorphic);
    assert!(!first_export.optimization.eligible);
    assert!(typecheck_package(&[first]).ok);
    assert!(typecheck_package(&[renamed]).ok);
}

#[test]
fn effects_shapes_and_runtime_contracts_block_static_optimizer_admission() {
    let effectful = module(
        "effect.gc",
        ":imports [] :exports [pkg/effect::clock] :caps [sys/time::now] :refinements {pkg/effect::clock []} :types {pkg/effect::clock (Prog Int (Eff [sys/time::now] nil))}",
        "(def pkg/effect::clock (core/effect::perform 'sys/time::now nil (fn (_) (core/effect::pure 1))))\npkg/effect::clock",
    );
    let contract = module(
        "contract.gc",
        ":imports [] :exports [pkg/contract::handler] :caps [] :refinements {pkg/contract::handler []} :types {pkg/contract::handler (Contract [[pkg/op::run (Fn (Msg Int) Int (Eff [] nil))]] r)}",
        "(def pkg/contract::handler (core/contract::extend core/contract::genesis {pkg/op::run (fn (_) 1)} {}))\npkg/contract::handler",
    );
    let effect_report = compose_contract_profile(&[effectful]);
    let contract_report = compose_contract_profile(&[contract]);
    assert!(effect_report.ok, "{:?}", effect_report.errors_by_module);
    assert!(contract_report.ok, "{:?}", contract_report.errors_by_module);
    let effect_optimization = &effect_report.exports["pkg/effect::clock"].optimization;
    assert!(effect_optimization.closed_effects);
    assert!(!effect_optimization.pure);
    assert!(!effect_optimization.eligible);
    let contract_optimization = &contract_report.exports["pkg/contract::handler"].optimization;
    assert!(!contract_optimization.closed_shapes);
    assert!(!contract_optimization.contract_free);
    assert!(!contract_optimization.eligible);
}

#[test]
fn effect_and_shape_changes_invalidate_static_interface_identity() {
    let subject = |type_term: &str| {
        module(
            "identity.gc",
            &format!(
                ":imports [] :exports [pkg/id::value] :caps [sys/time::now] :refinements {{pkg/id::value []}} :types {{pkg/id::value {type_term}}}"
            ),
            "(def pkg/id::value 1)\npkg/id::value",
        )
    };
    let pure = compose_contract_profile(&[subject("Int")]);
    let effectful = compose_contract_profile(&[subject("(Prog Int (Eff [sys/time::now] nil))")]);
    assert!(pure.ok && effectful.ok);
    assert_ne!(
        pure.exports["pkg/id::value"].shape_identity,
        effectful.exports["pkg/id::value"].shape_identity
    );
    assert_ne!(
        pure.exports["pkg/id::value"].interface_identity,
        effectful.exports["pkg/id::value"].interface_identity
    );
}

#[test]
fn malformed_closure_and_nonempty_refinements_fail_without_profile_identity() {
    let invalid = module_from_source(
        "invalid.gc",
        r#"
          (def ::meta
            '{:module-profile genesis/module-resolution-profile-v0.1
              :requires-profiles {
                genesis/coreform-profile genesis/coreform/v0.2
                genesis/hash-profile genesis/hash-profile/gcv0.2-blake3
                genesis/language-profile genesis/language-profile/v0.2
                genesis/module-resolution-profile genesis/module-resolution-profile-v0.1}
              :contract-composition-profile genesis/contract-composition-profile-v0.1
              :imports []
              :exports [pkg/bad::value]
              :caps []
              :strict-shapes false
              :strict-effects true
              :refinements {pkg/bad::value [pkg/refinement::positive]}
              :types {pkg/bad::value Int}})
          (def pkg/bad::value 1)
          pkg/bad::value
        "#,
    );
    let report = compose_contract_profile(&[invalid]);
    assert!(!report.ok);
    assert!(report.profile_identity.is_none());
    let errors = &report.errors_by_module["invalid.gc"];
    assert!(errors.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(errors.iter().any(|error| {
        error.contains("[blame=boundary]") && error.contains(":strict-shapes true")
    }));
    assert!(errors.iter().any(|error| {
        error.contains("pkg/refinement::positive") && error.contains("unsupported")
    }));
}

#[test]
fn mixed_profile_closure_fails_closed() {
    let profiled = module(
        "profiled.gc",
        ":imports [] :exports [pkg/a::value] :caps [] :refinements {pkg/a::value []} :types {pkg/a::value Int}",
        "(def pkg/a::value 1)\npkg/a::value",
    );
    let legacy = module_from_source(
        "legacy.gc",
        "(def ::meta '{:exports [pkg/b::value] :caps [] :types {pkg/b::value Int}}) (def pkg/b::value 2)",
    );
    let report = compose_contract_profile(&[profiled, legacy]);
    assert!(!report.ok);
    assert!(report.profile_identity.is_none());
    assert!(report.errors_by_module["legacy.gc"].iter().any(|error| {
        error.contains("every module in the contract closure")
            && error.contains(CONTRACT_COMPOSITION_PROFILE_ID)
    }));
}
