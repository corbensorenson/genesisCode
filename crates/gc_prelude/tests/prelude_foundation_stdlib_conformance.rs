use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{Env, EvalCtx, Value, ValueMap, eval_module};
use gc_prelude::build_prelude;

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
