use std::fs;
use std::path::PathBuf;

use gc_coreform::{
    COREFORM_PROFILE_ID, HASH_PROFILE_ID, LANGUAGE_PROFILE_ID, canonicalize_module, parse_module,
    parse_term, print_term,
};
use gc_kernel::{EvalCtx, MemLimits, VALUE_EFFECT_HASH_PROFILE_ID, Value, eval_module};
use gc_pkg::PackageManifest;
use gc_prelude::build_prelude;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn profile() -> JsonValue {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/GC_AGENT_PROFILE_v0.3.json");
    serde_json::from_str(&fs::read_to_string(path).expect("read GC-AGENT-v0.3"))
        .expect("parse GC-AGENT-v0.3")
}

fn core_card() -> JsonValue {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/GC_AGENT_CORE_CARD_v0.3.json");
    serde_json::from_str(&fs::read_to_string(path).expect("read GC-AGENT core card"))
        .expect("parse GC-AGENT core card")
}

fn cases<'a>(profile: &'a JsonValue, name: &str) -> &'a Vec<JsonValue> {
    profile["conformance"][name]
        .as_array()
        .unwrap_or_else(|| panic!("missing conformance case group {name}"))
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Data(_) => "Data",
        Value::Int(_) => "Int",
        Value::Vector(_) => "Vector",
        Value::Map(_) => "Map",
        Value::Closure(_) => "Closure",
        Value::CompiledClosure(_) => "CompiledClosure",
        Value::SealToken(_) => "SealToken",
        Value::Sealed { .. } => "Sealed",
        Value::NativeFn(_) => "NativeFn",
        Value::Contract(_) => "Contract",
        Value::EffectProgram(_) => "EffectProgram",
        Value::EffectRequest(_) => "EffectRequest",
    }
}

fn eval_source(source: &str, ctx: &mut EvalCtx) -> Result<Value, gc_kernel::KernelError> {
    let forms = canonicalize_module(parse_module(source).expect("profile evaluator source parses"))
        .expect("profile evaluator source canonicalizes");
    let prelude = build_prelude(ctx);
    let mut env = prelude.env;
    ctx.reset_counters();
    eval_module(ctx, &mut env, &forms)
}

#[test]
fn profile_identity_matches_runtime_constants() {
    let profile = profile();
    assert_eq!(profile["profileId"], "GC-AGENT-v0.3");
    assert_eq!(
        profile["compatibility"]["languageProfile"],
        LANGUAGE_PROFILE_ID
    );
    assert_eq!(
        profile["compatibility"]["coreformProfile"],
        COREFORM_PROFILE_ID
    );
    assert_eq!(profile["compatibility"]["hashProfile"], HASH_PROFILE_ID);
    assert_eq!(
        profile["compatibility"]["valueEffectHashProfile"],
        VALUE_EFFECT_HASH_PROFILE_ID
    );
}

#[test]
fn parser_cases_match_the_frozen_profile() {
    let profile = profile();
    for case in cases(&profile, "parserCases") {
        let id = case["id"].as_str().expect("parser case id");
        let source = case["source"].as_str().expect("parser source");
        let accepted = match case["mode"].as_str().expect("parser mode") {
            "module" => parse_module(source).map(|forms| forms.len()),
            "term" => parse_term(source).map(|_| 1),
            other => panic!("unknown parser mode {other}"),
        };
        match case["expected"].as_str().expect("parser expectation") {
            "accept" => {
                let count = accepted.unwrap_or_else(|error| panic!("{id} rejected: {error}"));
                assert_eq!(
                    Some(count as u64),
                    case["formCount"].as_u64(),
                    "{id} form count"
                );
            }
            "reject" => assert!(accepted.is_err(), "{id} unexpectedly parsed"),
            other => panic!("unknown parser expectation {other}"),
        }
    }
}

#[test]
fn evaluator_cases_match_the_frozen_profile() {
    let profile = profile();
    for case in cases(&profile, "evaluatorCases") {
        let id = case["id"].as_str().expect("evaluator case id");
        let source = case["source"].as_str().expect("evaluator source");
        let mut ctx = EvalCtx::new();
        match eval_source(source, &mut ctx) {
            Ok(value) => {
                assert!(
                    case["expectedErrorKind"].is_null(),
                    "{id} expected an error"
                );
                assert_eq!(
                    Some(value_kind(&value)),
                    case["expectedValueKind"].as_str(),
                    "{id} value kind"
                );
                if let Some(expected) = case["expected"].as_str() {
                    let term = value
                        .to_plain_term()
                        .unwrap_or_else(|| panic!("{id} has no plain term"));
                    assert_eq!(print_term(&term), expected, "{id} plain value");
                }
            }
            Err(error) => {
                assert!(
                    case["expectedValueKind"].is_null(),
                    "{id} unexpectedly failed"
                );
                assert_eq!(
                    format!("{:?}", error.kind),
                    case["expectedErrorKind"].as_str().unwrap(),
                    "{id} error kind"
                );
            }
        }
    }
}

#[test]
fn resource_cases_fail_at_the_declared_boundary() {
    let profile = profile();
    for case in cases(&profile, "resourceCases") {
        let id = case["id"].as_str().expect("resource case id");
        let limits = &case["limits"];
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(MemLimits {
            max_alloc_units: limits["maxAllocUnits"].as_u64(),
            max_live_units: limits["maxLiveUnits"].as_u64(),
            max_pair_cells: limits["maxPairCells"].as_u64(),
            max_vec_len: limits["maxVecLen"].as_u64(),
            max_map_len: limits["maxMapLen"].as_u64(),
            max_bytes_len: limits["maxBytesLen"].as_u64(),
            max_string_len: limits["maxStringLen"].as_u64(),
        });
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        ctx.reset_counters();
        ctx.step_limit = limits["steps"].as_u64();
        let forms = canonicalize_module(
            parse_module(case["source"].as_str().expect("resource source")).unwrap(),
        )
        .unwrap();
        let error = eval_module(&mut ctx, &mut env, &forms)
            .unwrap_err_or_else(|| panic!("{id} did not exhaust its resource"));
        assert_eq!(
            format!("{:?}", error.kind),
            case["expectedErrorKind"].as_str().unwrap(),
            "{id} error kind"
        );
    }
}

#[test]
fn package_cases_match_schema_and_path_policy() {
    let profile = profile();
    for case in cases(&profile, "packageCases") {
        let id = case["id"].as_str().expect("package case id");
        let temp = tempdir().unwrap();
        let path = temp.path().join("package.toml");
        fs::write(&path, case["source"].as_str().expect("package source")).unwrap();
        let accepted = PackageManifest::load(&path).is_ok();
        assert_eq!(accepted, case["expected"] == "accept", "{id}");
    }
}

#[test]
fn language_card_symbols_and_examples_parse() {
    let card = core_card();
    for symbol in card["symbols"].as_array().expect("card symbols") {
        let symbol = symbol.as_str().expect("card symbol string");
        parse_term(symbol).unwrap_or_else(|error| panic!("card symbol {symbol:?}: {error}"));
    }
    for example in card["examples"].as_array().expect("card examples") {
        let id = example["id"].as_str().expect("card example id");
        let source = example["source"].as_str().expect("card example source");
        parse_term(source).unwrap_or_else(|error| panic!("card example {id}: {error}"));
    }

    let expected_classes = [
        "experimental-syntax",
        "host-only-operation",
        "unavailable-target",
        "nondeterministic-facility",
        "out-of-profile-capability",
    ];
    let classes = card["unsupportedClasses"]
        .as_array()
        .expect("card unsupported classes");
    assert_eq!(
        classes
            .iter()
            .map(|value| value.as_str().expect("unsupported class string"))
            .collect::<Vec<_>>(),
        expected_classes
    );
    let unsupported = card["unsupportedBehavior"]
        .as_array()
        .expect("card unsupported behavior");
    for class in expected_classes {
        assert!(
            unsupported
                .iter()
                .any(|item| item["roadmapClass"].as_str() == Some(class)),
            "card manifest omitted unsupported class {class}"
        );
    }
    for item in unsupported {
        assert_eq!(item["status"], "unsupported");
        assert!(
            item["safeAlternative"]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty()),
            "unsupported behavior must include a safe alternative"
        );
    }
    let markdown_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/GC_AGENT_CORE_CARD_v0.3.md");
    let markdown = fs::read_to_string(markdown_path).expect("read GC-AGENT core card markdown");
    for class in expected_classes {
        assert!(markdown.contains(class), "compact card omitted {class}");
    }
}

trait ResultExt<T, E> {
    fn unwrap_err_or_else(self, success: impl FnOnce() -> E) -> E;
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    fn unwrap_err_or_else(self, success: impl FnOnce() -> E) -> E {
        match self {
            Ok(_) => success(),
            Err(error) => error,
        }
    }
}
