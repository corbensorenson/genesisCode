use crate::{parse_module, parse_term, print_module, print_term, Term};

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

