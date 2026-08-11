use std::collections::BTreeSet;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{
    Env, EvalCtx, Value, eval_module, eval_module_compiled,
    text_profile::{
        NORMALIZATION_IMPLEMENTATION_VERSION, SEGMENTATION_IMPLEMENTATION_VERSION,
        UNICODE_STANDARD_VERSION,
    },
    value_hash,
};
use gc_opt::{Stage2LoweringMode, stage2_validation_report};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

const PROFILE: &str = include_str!("../../../docs/spec/TEXT_PATH_PROFILE_v0.1.json");
const SPEC: &[u8] = include_bytes!("../../../docs/spec/TEXT_PATH_PROFILE_v0.1.md");
const SCHEMA: &[u8] = include_bytes!("../../../docs/spec/TEXT_PATH_PROFILE_v0.1.schema.json");
const FS_DISPATCH: &str = include_str!("../../gc_effects/src/runner_capability_dispatch/fs.rs");
const IO_BOUNDARY: &str = include_str!("../../gc_effects/src/runner_io_ops.rs");

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

fn error_code(value: Value, expected_token: gc_kernel::SealId) -> String {
    let Value::Sealed { token, payload } = value else {
        panic!("expected trusted sealed ERROR");
    };
    assert_eq!(token, expected_token);
    let Term::Map(fields) = payload.to_plain_term().expect("plain error payload") else {
        panic!("error payload must be a map");
    };
    let Some(Term::Str(code)) = fields.get(&TermOrdKey(Term::symbol(":error/code"))) else {
        panic!("error payload must contain :error/code");
    };
    code.clone()
}

#[test]
fn text_path_profile_is_closed_versioned_and_source_bound() {
    let profile: JsonValue = serde_json::from_str(PROFILE).expect("valid text/path profile");
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
            "bytes",
            "canonicalSpec",
            "canonicalSpecSha256",
            "contentIdentitySha256",
            "kind",
            "nonclaims",
            "paths",
            "schema",
            "schemaSha256",
            "sourceBindings",
            "text",
            "unicode",
            "version",
        ]
    );
    assert_eq!(profile["kind"], "genesis/text-path-profile-v0.1");
    assert_eq!(profile["canonicalSpecSha256"], sha256_hex(SPEC));
    assert_eq!(profile["schemaSha256"], sha256_hex(SCHEMA));
    assert_eq!(profile["contentIdentitySha256"], content_identity(&profile));
    assert_eq!(profile["unicode"]["version"], "17.0.0");
    assert_eq!(UNICODE_STANDARD_VERSION, (17, 0, 0));
    assert_eq!(
        profile["unicode"]["normalizationImplementation"],
        NORMALIZATION_IMPLEMENTATION_VERSION
    );
    assert_eq!(
        profile["unicode"]["segmentationImplementation"],
        SEGMENTATION_IMPLEMENTATION_VERSION
    );

    let schema: JsonValue = serde_json::from_slice(SCHEMA).expect("valid schema JSON");
    assert_eq!(schema["additionalProperties"], false);
    let required = schema["required"]
        .as_array()
        .expect("schema required")
        .iter()
        .map(|item| item.as_str().expect("required string"))
        .collect::<BTreeSet<_>>();
    assert_eq!(required, keys.into_iter().collect());

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
fn unicode_values_hashes_and_errors_match_runtime_tiers() {
    let forms = canonicalize_module(
        parse_module(
            r#"
              {
                :byte-len (prim str/len "é")
                :scalar-len (prim str/scalar-len "é")
                :grapheme-len (prim str/grapheme-len "👩‍👩‍👧‍👦")
                :nfc (prim str/nfc "é")
                :exact-equal (prim core/eq? "é" "é")
                :slice (prim str/grapheme-slice "a👩‍👩‍👧‍👦z" 1 1)
                :bytes (prim str/to-bytes-utf8 "é")
              }
            "#,
        )
        .expect("parse text fixture"),
    )
    .expect("canonicalize text fixture");
    let expected = parse_module(
        r#"{:byte-len 2 :bytes b"\xC3\xA9" :exact-equal false :grapheme-len 1 :nfc "é" :scalar-len 2 :slice "👩‍👩‍👧‍👦"}"#,
    )
    .expect("parse expected")[0]
        .clone();

    let mut values = Vec::new();
    for compiled in [false, true] {
        let mut ctx = EvalCtx::new();
        let mut env = Env::empty();
        let value = if compiled {
            eval_module_compiled(&mut ctx, &mut env, &forms).expect("compiled text eval")
        } else {
            eval_module(&mut ctx, &mut env, &forms).expect("reference text eval")
        };
        assert_eq!(value.to_plain_term(), Some(expected.clone()));
        values.push(value);
    }
    assert_eq!(value_hash(&values[0]), value_hash(&values[1]));

    for (source, code) in [
        ("(prim bytes/to-str-utf8 b\"\\xFF\")", "core/type-error"),
        (
            "(prim str/grapheme-slice \"a\" 2 0)",
            "core/text-range-error",
        ),
        (
            "(prim str/grapheme-slice \"a\" -1 1)",
            "core/text-range-error",
        ),
    ] {
        let forms = canonicalize_module(parse_module(source).expect("parse error fixture"))
            .expect("canonicalize error fixture");
        let mut codes = Vec::new();
        for compiled in [false, true] {
            let mut ctx = EvalCtx::new();
            let protocol = ctx.protocol.expect("protocol tokens");
            let mut env = Env::empty();
            let value = if compiled {
                eval_module_compiled(&mut ctx, &mut env, &forms).expect("compiled error eval")
            } else {
                eval_module(&mut ctx, &mut env, &forms).expect("reference error eval")
            };
            codes.push(error_code(value, protocol.error));
        }
        assert_eq!(codes, [code.to_string(), code.to_string()]);
    }
}

#[test]
fn stage2_validates_unicode_with_native_and_constant_fallback_lowering() {
    let supported = canonicalize_module(
        parse_module(
            r#"
              (if (prim int/eq? (prim str/scalar-len "é") 2)
                (if (prim int/eq? (prim str/grapheme-len "👩‍👩‍👧‍👦") 1)
                  (prim core/eq? (prim str/nfc "é") "é")
                  false)
                false)
            "#,
        )
        .expect("parse supported stage2 text"),
    )
    .expect("canonicalize supported stage2 text");
    let report = stage2_validation_report(&supported);
    assert!(report.supported, "{report:?}");
    assert!(report.ok, "{report:?}");
    assert_eq!(report.lowering_mode, Some(Stage2LoweringMode::Strict));

    let slice = canonicalize_module(
        parse_module("(prim str/grapheme-slice \"ab\" 0 1)")
            .expect("parse constant-fallback stage2 text"),
    )
    .expect("canonicalize constant-fallback stage2 text");
    let report = stage2_validation_report(&slice);
    assert!(report.supported, "{report:?}");
    assert!(report.ok, "{report:?}");
    assert_eq!(
        report.lowering_mode,
        Some(Stage2LoweringMode::ConstantFallback)
    );
}

#[test]
fn filesystem_path_boundary_has_no_lossy_or_absolute_payload_route() {
    assert!(!FS_DISPATCH.contains("to_string_lossy"));
    assert!(FS_DISPATCH.contains("core/path-encoding-error"));
    assert!(FS_DISPATCH.contains("core/path-collision-error"));
    assert!(IO_BOUNDARY.contains("validate_portable_effect_path"));
    assert!(IO_BOUNDARY.contains("filesystem path must be base-relative"));
    assert!(IO_BOUNDARY.contains("<outside-base>"));
    assert!(IO_BOUNDARY.contains("<invalid-path>"));

    let effects_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../gc_effects/src");
    let mut pending = vec![effects_src];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("read effects source directory") {
            let entry = entry.expect("read effects source entry");
            let file_type = entry.file_type().expect("read effects source file type");
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() && entry.path().extension().is_some_and(|ext| ext == "rs")
            {
                let source = std::fs::read_to_string(entry.path()).expect("UTF-8 Rust source");
                assert!(
                    !source.contains("to_string_lossy"),
                    "production effects source uses lossy path conversion: {}",
                    entry.path().display()
                );
            }
        }
    }
}
