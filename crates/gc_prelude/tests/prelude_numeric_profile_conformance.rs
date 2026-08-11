use std::collections::BTreeSet;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{
    Env, EvalCtx, KernelErrorKind, Value, eval_module, eval_module_compiled, value_hash,
};
use gc_opt::{OptimizeCommandError, optimize_command_pipeline};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

const PROFILE: &str = include_str!("../../../docs/spec/NUMERIC_PROFILE_v0.1.json");
const SPEC: &[u8] = include_bytes!("../../../docs/spec/NUMERIC_PROFILE_v0.1.md");
const SCHEMA: &[u8] = include_bytes!("../../../docs/spec/NUMERIC_PROFILE_v0.1.schema.json");

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn content_identity(value: &JsonValue) -> String {
    let mut payload = value.clone();
    payload
        .as_object_mut()
        .expect("profile root object")
        .remove("contentIdentitySha256");
    sha256_hex(
        serde_json::to_string(&payload)
            .expect("canonical profile JSON")
            .as_bytes(),
    )
}

fn error_payload(value: Value, expected_token: gc_kernel::SealId) -> Term {
    let Value::Sealed { token, payload } = value else {
        panic!("expected trusted sealed ERROR");
    };
    assert_eq!(token, expected_token);
    payload.to_plain_term().expect("plain sealed payload")
}

#[test]
fn numeric_profile_is_closed_and_source_bound() {
    let profile: JsonValue = serde_json::from_str(PROFILE).expect("valid numeric profile");
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
            "backends",
            "canonicalSpec",
            "canonicalSpecSha256",
            "contentIdentitySha256",
            "decimal",
            "float",
            "integer",
            "kind",
            "nonclaims",
            "schema",
            "schemaSha256",
            "sourceBindings",
            "version",
        ]
    );
    assert_eq!(profile["kind"], "genesis/numeric-profile-v0.1");
    assert_eq!(profile["version"], "0.1");
    assert_eq!(profile["canonicalSpecSha256"], sha256_hex(SPEC));
    assert_eq!(profile["schemaSha256"], sha256_hex(SCHEMA));
    assert_eq!(profile["contentIdentitySha256"], content_identity(&profile));
    assert_eq!(
        profile["integer"]["representation"],
        "arbitrary-precision-signed"
    );
    assert_eq!(
        profile["integer"]["division"],
        "euclidean-a=b*q+r-and-0<=r<abs(b)"
    );
    assert_eq!(profile["decimal"]["maxScale"], 4096);
    assert_eq!(profile["float"]["coreStatus"], "unsupported");
    assert_eq!(
        profile["backends"]["stage2"],
        "i64-candidate-exact-translation-validation-required-before-emission"
    );

    let schema: JsonValue = serde_json::from_slice(SCHEMA).expect("valid schema JSON");
    assert_eq!(schema["additionalProperties"], false);
    let schema_required = schema["required"]
        .as_array()
        .expect("schema required array")
        .iter()
        .map(|item| item.as_str().expect("required string"))
        .collect::<BTreeSet<_>>();
    assert_eq!(schema_required, keys.into_iter().collect());

    let bindings = profile["sourceBindings"]
        .as_array()
        .expect("source bindings");
    let ids = bindings
        .iter()
        .map(|binding| binding["id"].as_str().expect("binding id"))
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), bindings.len());
    assert_eq!(
        ids,
        [
            "source/compiled-evaluator",
            "source/decimal-canonical-data",
            "source/decimal-runtime",
            "source/numeric-primitives",
            "source/numeric-division",
            "source/prelude",
            "source/selfhost-parser-core",
            "source/selfhost-parser-lexical",
            "source/stage1",
            "source/stage2",
            "source/type-inference",
            "source/wasm-route",
        ]
        .into_iter()
        .collect()
    );
    for binding in bindings {
        let path = binding["path"].as_str().expect("binding path");
        assert!(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(path)
                .is_file(),
            "missing source binding: {path}"
        );
    }
}

#[test]
fn numeric_values_errors_and_hashes_match_across_tiers() {
    let forms = canonicalize_module(
        parse_module(
            r#"
              {
                :decimal-eq (prim dec/eq? (prim dec/parse "1.2300") (prim dec/parse "1.23"))
                :decimal-mul (prim dec/to-str (prim dec/mul (prim dec/parse "2.50") (prim dec/from-int 3)))
                :quotient (prim int/div -7 3)
                :remainder (prim int/mod -7 3)
              }
            "#,
        )
        .expect("parse numeric fixture"),
    )
    .expect("canonicalize numeric fixture");
    let expected =
        parse_module(r#"{:decimal-eq true :decimal-mul "7.5" :quotient -3 :remainder 2}"#)
            .expect("parse expected")[0]
            .clone();

    let mut values = Vec::new();
    for compiled in [false, true] {
        let mut ctx = EvalCtx::new();
        let mut env = Env::empty();
        let value = if compiled {
            eval_module_compiled(&mut ctx, &mut env, &forms).expect("compiled numeric eval")
        } else {
            eval_module(&mut ctx, &mut env, &forms).expect("reference numeric eval")
        };
        assert_eq!(value.to_plain_term(), Some(expected.clone()));
        values.push(value);
    }
    assert_eq!(value_hash(&values[0]), value_hash(&values[1]));

    for source in ["(prim int/div 1 0)", "(prim int/mod 1 0)"] {
        let forms = canonicalize_module(parse_module(source).expect("parse zero divisor"))
            .expect("canonicalize zero divisor");
        let mut payloads = Vec::new();
        for compiled in [false, true] {
            let mut ctx = EvalCtx::new();
            let protocol = ctx.protocol.expect("reserved protocol tokens");
            let mut env = Env::empty();
            let value = if compiled {
                eval_module_compiled(&mut ctx, &mut env, &forms).expect("compiled zero divisor")
            } else {
                eval_module(&mut ctx, &mut env, &forms).expect("reference zero divisor")
            };
            let payload = error_payload(value, protocol.error);
            let Term::Map(fields) = &payload else {
                panic!("numeric error payload must be a map");
            };
            assert_eq!(
                fields.get(&TermOrdKey(Term::symbol(":error/code"))),
                Some(&Term::Str("core/numeric-error".to_string()))
            );
            payloads.push(payload);
        }
        assert_eq!(payloads[0], payloads[1]);
    }

    let oversized = format!("(prim dec/parse \"0.{}1\")", "0".repeat(4096));
    let forms = canonicalize_module(parse_module(&oversized).expect("parse oversized decimal"))
        .expect("canonicalize oversized decimal");
    let mut ctx = EvalCtx::new();
    let protocol = ctx.protocol.expect("reserved protocol tokens");
    let mut env = Env::empty();
    let payload = error_payload(
        eval_module(&mut ctx, &mut env, &forms).expect("oversized decimal eval"),
        protocol.error,
    );
    let Term::Map(fields) = payload else {
        panic!("decimal error payload must be a map");
    };
    assert_eq!(
        fields.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("core/numeric-error".to_string()))
    );
}

#[test]
fn unsupported_numeric_surfaces_and_unvalidated_wasm_fail_closed() {
    let float_forms =
        canonicalize_module(parse_module("(prim float/add 1 2)").expect("parse unsupported float"))
            .expect("canonicalize unsupported float");
    for compiled in [false, true] {
        let mut ctx = EvalCtx::new();
        let mut env = Env::empty();
        let error = if compiled {
            eval_module_compiled(&mut ctx, &mut env, &float_forms).unwrap_err()
        } else {
            eval_module(&mut ctx, &mut env, &float_forms).unwrap_err()
        };
        assert_eq!(error.kind.to_string(), KernelErrorKind::BadForm.to_string());
        assert_eq!(error.msg, "unknown prim op: float/add");
    }

    let overflow = canonicalize_module(
        parse_module("(prim int/add 9223372036854775807 1)").expect("parse overflow"),
    )
    .expect("canonicalize overflow");
    assert!(matches!(
        optimize_command_pipeline(&overflow, false, false, true),
        Err(OptimizeCommandError::Stage2Gate(report)) if !report.ok
    ));
}
