use crate::{
    FixedDecimal, Term, TermOrdKey, canonicalize_module, hash_term, parse_module, parse_term,
    print_module, print_term, print_term_compact,
};
use bytes::Bytes;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[test]
fn parse_print_idempotent_simple() {
    let src = r#"
      (def add (fn (x) (fn (y) (prim int/add x y))))
      (add 1 2)
    "#;

    let m1 = parse_module(src).expect("parse");
    let p1 = print_module(&m1);
    let m2 = parse_module(&p1).expect("parse printed");
    let p2 = print_module(&m2);
    assert_eq!(p1, p2);
}

#[test]
fn bytes_and_strings_roundtrip() {
    let t = parse_term(r#"{"s" "a\nb" "b" b"\x00\xFF"}"#).expect("parse");
    let s = print_term(&t);
    let t2 = parse_term(&s).expect("parse printed");
    assert_eq!(t, t2);
}

#[test]
fn quote_sugar_becomes_quote_form() {
    let t = parse_term("'x").expect("parse");
    let s = print_term(&t);
    assert_eq!(s.trim(), "(quote x)");
}

#[test]
fn proper_list_recognition() {
    let t = Term::list(vec![Term::symbol("a"), Term::symbol("b")]);
    let xs = t.as_proper_list().expect("proper list");
    assert_eq!(xs.len(), 2);
}

#[test]
fn golden_coreform_canonicalization_and_printing() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/spec/coreform");

    for case in ["app_sugar", "map_order"] {
        let inp = std::fs::read_to_string(root.join(format!("{case}.in.gc"))).unwrap();
        let want = std::fs::read_to_string(root.join(format!("{case}.out.gc"))).unwrap();

        let forms = parse_module(&inp).unwrap();
        let canon = canonicalize_module(forms).unwrap();
        let got = print_module(&canon);

        assert_eq!(
            normalize(&got),
            normalize(&want),
            "golden mismatch for {case}"
        );
    }
}

#[test]
fn deep_nesting_parse_print_roundtrip_is_stable() {
    // This is a regression guard against stack overflows in parser/printer.
    // Keep this comfortably below pathological runtime for the current printer heuristic.
    let depth = 1000usize;
    let mut src = String::new();
    src.extend(std::iter::repeat_n('(', depth));
    src.push_str("nil");
    src.extend(std::iter::repeat_n(')', depth));

    let t1 = parse_term(&src).expect("parse deep term");
    let s1 = print_term(&t1);
    let t2 = parse_term(&s1).expect("parse printed deep term");
    let s2 = print_term(&t2);
    assert_eq!(s1, s2);
}

#[test]
fn term_key_order_covers_every_tag_and_compound_shape() {
    let mut one_entry = BTreeMap::new();
    one_entry.insert(TermOrdKey(Term::Nil), Term::Nil);
    let ordered = vec![
        Term::Nil,
        Term::Bool(false),
        Term::Bool(true),
        Term::Int((-1).into()),
        Term::Int(0.into()),
        Term::Str("a".to_string()),
        Term::Str("b".to_string()),
        Term::Bytes(Bytes::from_static(b"a")),
        Term::Bytes(Bytes::from_static(b"b")),
        Term::symbol("a"),
        Term::symbol("b"),
        Term::Pair(Box::new(Term::Nil), Box::new(Term::Nil)),
        Term::Pair(Box::new(Term::Nil), Box::new(Term::Bool(false))),
        Term::Vector(vec![]),
        Term::Vector(vec![Term::Nil]),
        Term::Map(BTreeMap::new()),
        Term::Map(one_entry),
    ];

    for (left_index, left) in ordered.iter().enumerate() {
        assert_eq!(TermOrdKey(left.clone()), TermOrdKey(left.clone()));
        for right in &ordered[left_index + 1..] {
            assert!(
                TermOrdKey(left.clone()) < TermOrdKey(right.clone()),
                "{} must sort before {}",
                print_term(left),
                print_term(right)
            );
        }
    }
}

#[test]
fn deeply_nested_compound_keys_compare_without_losing_total_order() {
    let mut left = Term::Nil;
    let mut right = Term::Bool(false);
    for _ in 0..2048 {
        left = Term::Pair(Box::new(Term::Nil), Box::new(left));
        right = Term::Pair(Box::new(Term::Nil), Box::new(right));
    }
    assert!(TermOrdKey(left) < TermOrdKey(right));
}

#[test]
fn singleton_list_is_just_grouping_in_canonical_form() {
    let src = r#"
      (  y   )
    "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let out = print_module(&forms);
    assert_eq!(out, "y\n");
}

#[test]
fn fixed_decimal_term_hash_is_normalized() {
    let a = FixedDecimal::parse("1.2300").expect("a").to_term();
    let b = FixedDecimal::parse("1.23").expect("b").to_term();
    assert_eq!(print_term(&a), print_term(&b));
    assert_eq!(hash_term(&a), hash_term(&b));
}

#[test]
fn compact_term_printer_roundtrips_equivalently() {
    let t = parse_term(
        r#"{:kind "artifact" :modules [{:path "selfhost/parse.gc" :forms [(def a 1) (def b [1 2 3])]}]}"#,
    )
    .expect("parse");
    let compact = print_term_compact(&t);
    let roundtrip = parse_term(&compact).expect("parse compact");
    assert_eq!(roundtrip, t);
}

#[test]
fn compact_term_printer_avoids_multiline_output_for_nested_forms() {
    let t = parse_term(
        r#"(def nested (fn (x) (fn (y) (if true {:a [1 2 3] :b (quote (alpha beta gamma))} nil))))"#,
    )
    .expect("parse");
    let compact = print_term_compact(&t);
    assert!(
        !compact.contains('\n'),
        "compact printer should avoid newlines, got:\n{compact}"
    );
}

fn normalize(s: &str) -> String {
    s.replace("\r\n", "\n").trim().to_string()
}
