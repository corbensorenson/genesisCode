use gc_coreform::{SpecialForm, Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{
    Env, EvalCtx, KernelErrorKind, Value, ValueMap, eval_module, eval_module_compiled, value_hash,
};
use gc_prelude::build_prelude;
use gc_types::{ModuleForTypecheck, infer_effects, typecheck_package};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

const FOUNDATION_REQUIRED_SYMBOLS: &[&str] = &[
    "core/list::is-nil?",
    "core/list::len",
    "core/list::reverse",
    "core/list::append",
    "core/list::map",
    "core/list::filter",
    "core/list::foldl",
    "core/map::get",
    "core/map::put",
    "core/map::merge",
    "core/map::len",
    "core/map::entries",
    "core/vec::get",
    "core/vec::push",
    "core/vec::len",
    "core/vec::set",
    "core/str::to-utf8",
    "core/str::from-utf8",
    "core/str::len",
    "core/str::concat",
    "core/str::repeat",
    "core/str::join",
    "core/bytes::len",
    "core/bytes::get",
    "core/bytes::slice",
    "core/bytes::concat",
    "core/sym::eq?",
    "core/sym::to-str",
    "core/crypto::blake3",
    "core/msg::make",
    "core/msg::op",
    "core/msg::payload",
    "core/contract::make",
    "core/contract::extend",
    "core/contract::dispatch",
    "core/contract::explain",
    "core/contract::meta",
    "core/contract::proto",
    "core/contract::shape",
    "core/contract::call",
    "core/effect::pure",
    "core/effect::perform",
    "core/effect::bind",
    "core/effect::map",
    "core/effect::then",
    "core/effect::catch",
    "core/effect::catch-payload",
];

fn eval_with_prelude(src: &str) -> (EvalCtx, Env, Value) {
    let forms =
        canonicalize_module(parse_module(src).expect("parse module")).expect("canonicalize");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let value = eval_module(&mut ctx, &mut env, &forms).expect("eval module");
    (ctx, env, value)
}

fn map_value<'a>(m: &'a ValueMap, key: &str) -> &'a Value {
    m.get(&TermOrdKey(Term::symbol(key)))
        .unwrap_or_else(|| panic!("missing key {key}"))
}

fn map_value_is_int(m: &ValueMap, key: &str, expected: i64) -> bool {
    matches!(map_value(m, key).to_plain_term(), Some(Term::Int(n)) if n == expected.into())
}

#[test]
fn foundation_required_symbols_exist_in_prelude() {
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    for sym in FOUNDATION_REQUIRED_SYMBOLS {
        assert!(
            prelude.env.get(sym).is_some(),
            "missing foundation stdlib symbol: {sym}"
        );
    }
}

#[test]
fn foundation_required_behavior_conforms() {
    let src = r#"
      (def xs (quote (1 2 3)))
      (def msg ((core/msg::make 'pkg/example::op) {:n 1}))
      (def c (core/contract::make (fn (m) (core/msg::payload m)) nil {}))
      (def m0 {:a 1 :b 2})
      (def m1 (((core/map::put m0) (quote :c)) 3))
      (def v0 [1 2])
      (def v1 (((core/vec::set v0) 1) 9))
      {
        :list-len (core/list::len xs)
        :list-map ((core/list::map xs) (fn (x) ((core/int::add x) 1)))
        :list-fold (((core/list::foldl xs) 0) (fn (acc x) ((core/int::add acc) x)))
        :map-get ((core/map::get m1) (quote :c))
        :map-len (core/map::len m1)
        :map-entries (core/map::entries (quote {:b 2 :a 1}))
        :vec-get ((core/vec::get v1) 1)
        :vec-len (core/vec::len ((core/vec::push v1) 10))
        :str-join ((core/str::join ["a" "b" "c"]) "-")
        :str-roundtrip (core/str::from-utf8 (core/str::to-utf8 "hi✓"))
        :bytes-slice (((core/bytes::slice (core/bytes::concat b"\x01\x02" b"\x03\x04")) 1) 2)
        :sym-eq ((core/sym::eq? 'pkg/example::op) 'pkg/example::op)
        :sym-str (core/sym::to-str 'pkg/example::op)
        :hash-len (core/bytes::len (core/crypto::blake3 b"abc"))
        :msg-op (core/msg::op msg)
        :msg-payload (core/msg::payload msg)
        :contract-call (((core/contract::call c) 'pkg/example::op) {:ok true})
      }
    "#;
    let (mut ctx, mut env, value) = eval_with_prelude(src);
    let Value::Map(m) = value else {
        panic!("expected map result");
    };

    assert!(map_value_is_int(&m, ":list-len", 3));
    assert!(map_value_is_int(&m, ":list-fold", 6));
    assert!(map_value_is_int(&m, ":map-get", 3));
    assert!(map_value_is_int(&m, ":map-len", 3));
    assert!(map_value_is_int(&m, ":vec-get", 9));
    assert!(map_value_is_int(&m, ":vec-len", 3));
    assert!(matches!(
        map_value(&m, ":str-join").as_data(),
        Some(Term::Str(s)) if s == "a-b-c"
    ));
    assert!(matches!(
        map_value(&m, ":str-roundtrip").as_data(),
        Some(Term::Str(s)) if s == "hi✓"
    ));
    assert!(matches!(
        map_value(&m, ":bytes-slice").as_data(),
        Some(Term::Bytes(bs)) if bs.as_ref() == [2, 3]
    ));
    assert!(matches!(
        map_value(&m, ":sym-eq").as_data(),
        Some(Term::Bool(true))
    ));
    assert!(matches!(
        map_value(&m, ":sym-str").as_data(),
        Some(Term::Str(s)) if s == "pkg/example::op"
    ));
    assert!(map_value_is_int(&m, ":hash-len", 32));
    assert!(matches!(
        map_value(&m, ":msg-op").as_data(),
        Some(Term::Symbol(s)) if s == "pkg/example::op"
    ));

    let Some(Term::Map(msg_payload)) = map_value(&m, ":msg-payload").as_data() else {
        panic!(":msg-payload must be map");
    };
    assert!(matches!(
        msg_payload.get(&TermOrdKey(Term::symbol(":n"))),
        Some(Term::Int(i)) if i == &1.into()
    ));

    let Some(Term::Map(contract_payload)) = map_value(&m, ":contract-call").as_data() else {
        panic!(":contract-call must be map");
    };
    assert!(matches!(
        contract_payload.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(Term::Bool(true))
    ));

    let entries: Vec<Term> = match map_value(&m, ":map-entries") {
        Value::Data(t) if matches!(t.as_ref(), Term::Vector(_)) => {
            let Term::Vector(entries) = t.as_ref() else {
                panic!(":map-entries must be vector");
            };
            entries.clone()
        }
        Value::Vector(entries) => entries
            .iter()
            .map(|v| {
                v.as_data().cloned().unwrap_or_else(|| {
                    panic!("map entry must be data term, got {}", v.debug_repr())
                })
            })
            .collect(),
        other => panic!(":map-entries must be vector, got {}", other.debug_repr()),
    };
    assert_eq!(entries.len(), 2, "map entries must contain two pairs");
    let Term::Vector(first) = &entries[0] else {
        panic!("first map entry must be tuple vector");
    };
    assert!(matches!(first.first(), Some(Term::Symbol(s)) if s == ":a"));

    let expected_list_map = {
        let forms = canonicalize_module(parse_module("(quote (2 3 4))").expect("parse quote"))
            .expect("canonicalize quote");
        eval_module(&mut ctx, &mut env, &forms).expect("eval quote")
    };
    assert_eq!(
        map_value(&m, ":list-map").debug_repr(),
        expected_list_map.debug_repr(),
        "core/list::map output mismatch"
    );
}

const FORM_MATRIX: &str = include_str!("../../../docs/spec/NORMATIVE_FORM_MATRIX_v0.1.json");
const FORM_SPEC: &[u8] = include_bytes!("../../../docs/spec/NORMATIVE_FORM_MATRIX_v0.1.md");
const FORM_SCHEMA: &[u8] =
    include_bytes!("../../../docs/spec/NORMATIVE_FORM_MATRIX_v0.1.schema.json");
const REFERENCE_EVALUATOR: &str = include_str!("../../gc_kernel/src/eval_treewalk.rs");
const COMPILED_EVALUATOR: &str = include_str!("../../gc_kernel/src/compiled_compile.rs");
const WASM_RUNTIME: &str = include_str!("../../gc_wasm/src/lib.rs");

fn json_string<'a>(value: &'a JsonValue, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("matrix field {key} must be a string"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn content_identity(value: &JsonValue) -> String {
    let mut payload = value.clone();
    payload
        .as_object_mut()
        .expect("matrix root object")
        .remove("contentIdentitySha256");
    sha256_hex(
        serde_json::to_string(&payload)
            .expect("canonical matrix JSON")
            .as_bytes(),
    )
}

fn matrix() -> JsonValue {
    let value: JsonValue = serde_json::from_str(FORM_MATRIX).expect("valid normative form matrix");
    let object = value.as_object().expect("matrix root object");
    let expected = [
        "auditDate",
        "bindingSemantics",
        "canonicalSpec",
        "canonicalSpecSha256",
        "contentIdentitySha256",
        "errorSemantics",
        "forms",
        "kind",
        "nonclaims",
        "patternSemantics",
        "schema",
        "schemaSha256",
        "sourceBindings",
        "specialFormSymbols",
        "version",
    ];
    assert_eq!(
        object.keys().map(String::as_str).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(value["canonicalSpecSha256"], sha256_hex(FORM_SPEC));
    assert_eq!(value["schemaSha256"], sha256_hex(FORM_SCHEMA));
    assert_eq!(value["contentIdentitySha256"], content_identity(&value));
    value
}

fn match_arm_heads(source: &str, begin: &str, end: &str) -> std::collections::BTreeSet<String> {
    let body = source
        .split_once(begin)
        .unwrap_or_else(|| panic!("missing form-dispatch start marker: {begin}"))
        .1
        .split_once(end)
        .unwrap_or_else(|| panic!("missing form-dispatch end marker: {end}"))
        .0;
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix('"')?;
            let (head, suffix) = rest.split_once('"')?;
            suffix
                .trim_start()
                .starts_with("=>")
                .then(|| head.to_string())
        })
        .collect()
}

fn type_source(id: &str, source: &str) -> String {
    match id {
        "form/variable" => "(def x 42)\n(def result x)".to_string(),
        "form/def" => "(def result 42)".to_string(),
        _ => format!("(def result {source})"),
    }
}

fn inferred_type_head(term: &Term) -> &str {
    match term {
        Term::Symbol(symbol) => symbol,
        Term::Pair(_, _) => term
            .as_proper_list()
            .and_then(|items| items.first().copied())
            .and_then(|head| match head {
                Term::Symbol(symbol) => Some(symbol.as_str()),
                _ => None,
            })
            .expect("compound inferred type head"),
        other => panic!("unexpected inferred type term: {other:?}"),
    }
}

#[test]
fn normative_form_inventory_rejects_undocumented_tier_heads() {
    let value = matrix();
    let documented: std::collections::BTreeSet<String> = value["specialFormSymbols"]
        .as_array()
        .expect("special form symbols")
        .iter()
        .map(|item| item.as_str().expect("special form symbol").to_string())
        .collect();
    let exported: std::collections::BTreeSet<String> = gc_coreform::SpecialForm::ALL
        .iter()
        .map(|form| form.symbol().to_string())
        .collect();
    assert_eq!(documented, exported, "CoreForm inventory drift");

    let reference = match_arm_heads(
        REFERENCE_EVALUATOR,
        "// Special forms keyed by head symbol.",
        "// General application",
    );
    let compiled = match_arm_heads(
        COMPILED_EVALUATOR,
        "if let Term::Symbol(h) = items[0] {",
        "let f = with_child_path(path, 0",
    );
    assert_eq!(reference, documented, "reference evaluator form drift");
    assert_eq!(compiled, documented, "compiled evaluator form drift");
    assert!(WASM_RUNTIME.contains("canonicalize_module"));
    assert!(WASM_RUNTIME.contains("eval_module"));
    assert!(!WASM_RUNTIME.contains("match h.as_str()"));
}

#[test]
fn normative_form_rows_match_canonical_runtime_type_and_effect_behavior() {
    let value = matrix();
    let rows = value["forms"].as_array().expect("form rows");
    assert_eq!(rows.len(), 19);
    let mut ids = std::collections::BTreeSet::new();

    for row in rows {
        let id = json_string(row, "id");
        let source = json_string(row, "source");
        assert!(ids.insert(id), "duplicate form row {id}");

        let forms = canonicalize_module(parse_module(source).expect(id)).expect(id);
        let mut reference_ctx = EvalCtx::new();
        let mut reference_env = Env::empty();
        let reference = eval_module(&mut reference_ctx, &mut reference_env, &forms).expect(id);
        let mut compiled_ctx = EvalCtx::new();
        let mut compiled_env = Env::empty();
        let compiled =
            eval_module_compiled(&mut compiled_ctx, &mut compiled_env, &forms).expect(id);
        assert_eq!(
            compiled.debug_repr(),
            reference.debug_repr(),
            "{id} tier value"
        );
        assert_eq!(
            value_hash(&compiled),
            value_hash(&reference),
            "{id} tier hash"
        );

        let expectation = &row["evaluation"];
        match json_string(expectation, "kind") {
            "data" => {
                let expected =
                    gc_coreform::parse_term(json_string(expectation, "value")).expect(id);
                assert_eq!(reference.to_plain_term(), Some(expected), "{id} value");
            }
            "closure" => assert!(matches!(reference, Value::Closure(_)), "{id} value"),
            "seal-token" => assert!(matches!(reference, Value::SealToken(_)), "{id} value"),
            other => panic!("unknown evaluation kind {other}"),
        }

        let typed_forms = canonicalize_module(
            parse_module(&type_source(id, source)).expect("parse type witness"),
        )
        .expect("canonicalize type witness");
        let meta = gc_coreform::parse_term("{:exports [result] :types {result ?} :caps []}")
            .expect("type witness metadata");
        let report = typecheck_package(&[ModuleForTypecheck {
            path: format!("matrix/{id}.gc"),
            forms: typed_forms,
            meta: Some(meta),
        }]);
        assert!(report.ok, "{id} typecheck errors: {:?}", report.errors);
        let inferred = &report.modules[0]
            .export_types
            .iter()
            .find(|entry| entry.name == "result")
            .expect("result type")
            .inferred;
        assert_eq!(
            inferred_type_head(inferred),
            json_string(row, "typeExpectation"),
            "{id} inferred type"
        );

        let effects = infer_effects(&forms);
        let expected_ops: std::collections::BTreeSet<String> = row["effectOps"]
            .as_array()
            .expect("effect ops")
            .iter()
            .map(|item| item.as_str().expect("effect op").to_string())
            .collect();
        assert_eq!(effects.ops, expected_ops, "{id} effect ops");
        assert_eq!(
            effects.unknown,
            row["effectUnknown"].as_bool().unwrap(),
            "{id} effect tail"
        );
    }
}

#[test]
fn malformed_reserved_forms_fail_in_both_runtime_tiers() {
    let cases = [
        ("quote", "(quote)"),
        ("def", "((fn (x) x) (def x 1))"),
        ("fn", "(fn () nil)"),
        ("if", "(if true 1)"),
        ("begin", "(begin)"),
        ("let", "(let x 1)"),
        ("prim", "(prim)"),
        ("seal", "(seal 1)"),
        ("unseal", "(unseal nil)"),
    ];
    let documented: std::collections::BTreeSet<String> = matrix()["specialFormSymbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        cases
            .iter()
            .map(|(head, _)| head.to_string())
            .collect::<std::collections::BTreeSet<_>>(),
        documented
    );

    for (head, source) in cases {
        let parsed = parse_module(source).expect(head);
        if let Ok(forms) = canonicalize_module(parsed.clone()) {
            for compiled in [false, true] {
                let mut ctx = EvalCtx::new();
                let mut env = Env::empty();
                let error = if compiled {
                    eval_module_compiled(&mut ctx, &mut env, &forms).unwrap_err()
                } else {
                    eval_module(&mut ctx, &mut env, &forms).unwrap_err()
                };
                assert_eq!(
                    error.kind.to_string(),
                    KernelErrorKind::BadForm.to_string(),
                    "{head} {compiled}"
                );
            }
        }
    }
}

#[test]
fn binding_and_pattern_contract_is_closed_and_enforced() {
    let value = matrix();
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut source_ids = std::collections::BTreeSet::new();
    for binding in value["sourceBindings"]
        .as_array()
        .expect("source bindings")
    {
        let id = json_string(binding, "id");
        let path = json_string(binding, "path");
        assert!(source_ids.insert(id), "duplicate source binding {id}");
        assert!(
            !std::path::Path::new(path).is_absolute(),
            "source binding {id} must be repository-relative"
        );
        assert!(
            repository.join(path).is_file(),
            "source binding {id} does not exist: {path}"
        );
    }
    assert_eq!(
        source_ids,
        [
            "source/compiled-evaluator",
            "source/coreform-canonicalizer",
            "source/coreform-inventory",
            "source/coreform-parser",
            "source/effect-inference",
            "source/kernel-errors",
            "source/prelude",
            "source/prelude-coreform-api",
            "source/reference-evaluator",
            "source/selfhost-parser",
            "source/type-inference",
            "source/wasm-route",
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        value["bindingSemantics"]["functionParameters"],
        "symbol-only-unique-nonempty"
    );
    assert_eq!(
        value["bindingSemantics"]["letBindings"],
        "symbol-only-unique-sequential"
    );
    assert_eq!(
        value["bindingSemantics"]["topLevelDefinitions"],
        "symbol-only-sequential-rebinding"
    );
    assert_eq!(value["patternSemantics"]["status"], "unsupported");
    assert_eq!(
        value["patternSemantics"]["exhaustiveness"],
        "not-applicable"
    );
    assert_eq!(value["patternSemantics"]["guards"], "unsupported");
    assert_eq!(SpecialForm::from_symbol("match"), None);
    assert_eq!(SpecialForm::from_symbol("case"), None);
    assert_eq!(SpecialForm::from_symbol("when"), None);

    let rejected = [
        (
            "(fn (item item) item)",
            "canonicalize form 0: (fn ...) duplicate parameter: item",
        ),
        (
            "(let ((item 1) (item 2)) item)",
            "canonicalize form 0: (let ...) duplicate binding: item",
        ),
        (
            "(fn ((item)) item)",
            "canonicalize form 0: (fn ...) parameters must be symbols",
        ),
    ];
    for (source, expected) in rejected {
        let error = canonicalize_module(parse_module(source).expect("parse binder fixture"))
            .expect_err("invalid binder must fail");
        assert_eq!(error.to_string(), expected);
    }

    let rebinding = canonicalize_module(
        parse_module("(def item 1) (def item 2) item").expect("parse rebinding"),
    )
    .expect("top-level rebinding remains valid");
    for compiled in [false, true] {
        let mut ctx = EvalCtx::new();
        let mut env = Env::empty();
        let result = if compiled {
            eval_module_compiled(&mut ctx, &mut env, &rebinding).expect("compiled rebinding")
        } else {
            eval_module(&mut ctx, &mut env, &rebinding).expect("reference rebinding")
        };
        assert_eq!(result.to_plain_term(), Some(Term::Int(2.into())));
    }

    let hidden_pattern =
        canonicalize_module(parse_module("(match 1)").expect("parse ordinary application"))
            .expect("pattern-like head is ordinary application");
    let mut errors = Vec::new();
    for compiled in [false, true] {
        let mut ctx = EvalCtx::new();
        let mut env = Env::empty();
        let error = if compiled {
            eval_module_compiled(&mut ctx, &mut env, &hidden_pattern).unwrap_err()
        } else {
            eval_module(&mut ctx, &mut env, &hidden_pattern).unwrap_err()
        };
        errors.push((error.kind.to_string(), error.msg));
    }
    assert_eq!(errors[0], errors[1]);
    assert_eq!(errors[0].0, KernelErrorKind::Unbound.to_string());
    assert_eq!(errors[0].1, "unbound symbol: match");
}

#[test]
fn fatal_and_sealed_errors_match_across_runtime_tiers() {
    let malformed = parse_module("(if true 1)").expect("parse malformed CoreForm");
    let mut fatal = Vec::new();
    for compiled in [false, true] {
        let mut ctx = EvalCtx::new();
        let mut env = Env::empty();
        let error = if compiled {
            eval_module_compiled(&mut ctx, &mut env, &malformed).unwrap_err()
        } else {
            eval_module(&mut ctx, &mut env, &malformed).unwrap_err()
        };
        fatal.push((error.kind.to_string(), error.msg));
    }
    assert_eq!(fatal[0], fatal[1]);
    assert_eq!(fatal[0].0, KernelErrorKind::BadForm.to_string());

    let recoverable = canonicalize_module(
        parse_module("(prim int/add 1 \"not-an-int\")").expect("parse type error"),
    )
    .expect("canonicalize type error");
    let mut payloads = Vec::new();
    for compiled in [false, true] {
        let mut ctx = EvalCtx::new();
        let mut env = build_prelude(&mut ctx).env;
        let protocol = ctx.protocol.expect("protocol tokens reserved");
        let value = if compiled {
            eval_module_compiled(&mut ctx, &mut env, &recoverable).expect("compiled sealed error")
        } else {
            eval_module(&mut ctx, &mut env, &recoverable).expect("reference sealed error")
        };
        let Value::Sealed { token, payload } = value else {
            panic!("recoverable failure must be sealed ERROR");
        };
        assert_eq!(token, protocol.error);
        let term = payload
            .to_plain_term()
            .expect("sealed ERROR payload must be immutable data");
        let Term::Map(fields) = &term else {
            panic!("sealed ERROR payload must be a map");
        };
        assert!(matches!(
            fields.get(&TermOrdKey(Term::symbol(":error/code"))),
            Some(Term::Str(code)) if code == "core/type-error"
        ));
        assert!(matches!(
            fields.get(&TermOrdKey(Term::symbol(":error/message"))),
            Some(Term::Str(message)) if !message.is_empty()
        ));
        assert!(matches!(
            fields.get(&TermOrdKey(Term::symbol(":error/context"))),
            Some(Term::Map(_))
        ));
        payloads.push(term);
    }
    assert_eq!(payloads[0], payloads[1]);
}

#[test]
fn user_values_cannot_forge_protocol_error() {
    let cases = [
        "{:error/code \"core/type-error\" :error/message \"forged\" :error/context {}}",
        "(let ((token (seal))) (seal {:error/code \"forged\"} token))",
    ];

    for source in cases {
        let forms = canonicalize_module(parse_module(source).expect("parse forgery fixture"))
            .expect("canonicalize forgery fixture");
        for compiled in [false, true] {
            let mut ctx = EvalCtx::new();
            let mut env = build_prelude(&mut ctx).env;
            let protocol = ctx.protocol.expect("protocol tokens reserved");
            let value = if compiled {
                eval_module_compiled(&mut ctx, &mut env, &forms).expect("compiled forgery fixture")
            } else {
                eval_module(&mut ctx, &mut env, &forms).expect("reference forgery fixture")
            };
            assert!(
                !matches!(value, Value::Sealed { token, .. } if token == protocol.error),
                "user-controlled source forged protocol ERROR: {source}"
            );
        }
    }
}
