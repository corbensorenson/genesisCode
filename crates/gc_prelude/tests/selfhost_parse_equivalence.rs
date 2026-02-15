use std::path::Path;

use gc_coreform::{
    ParseError, Term, TermOrdKey, canonicalize_form, canonicalize_module, parse_module, parse_term,
};
use gc_kernel::{Apply, Env, EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn load_selfhost_parse(env: &mut Env, ctx: &mut EvalCtx) {
    let parse_path = Path::new("/Users/corbensorenson/Documents/genesisCode/selfhost/parse.gc");
    let src = std::fs::read_to_string(parse_path).expect("read selfhost/parse.gc");
    let raw_forms = parse_module(&src).expect("parse selfhost/parse.gc");
    for (i, f) in raw_forms.iter().enumerate() {
        if let Err(e) = canonicalize_form(f.clone()) {
            panic!("selfhost/parse.gc canonicalize failed at form {i}: {e}");
        }
    }
    let forms = canonicalize_module(raw_forms).expect("canonicalize selfhost/parse.gc");
    let _ = eval_module(ctx, env, &forms).expect("eval selfhost/parse.gc");
}

fn value_to_term(v: &Value) -> Option<Term> {
    match v {
        Value::Data(t) => Some(t.clone()),
        Value::Vector(xs) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(value_to_term(x)?);
            }
            Some(Term::Vector(out))
        }
        Value::Map(m) => {
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in m {
                out.insert(TermOrdKey(k.0.clone()), value_to_term(v)?);
            }
            Some(Term::Map(out))
        }
        _ => None,
    }
}

fn unerror_payload(ctx: &mut EvalCtx, env: &Env, v: Value) -> Option<Term> {
    let unerror = env
        .get("core/error::payload")
        .expect("core/error::payload bound");
    let out = unerror
        .clone()
        .apply(ctx, v)
        .expect("apply core/error::payload");
    let t = value_to_term(&out).unwrap_or_else(|| {
        panic!(
            "core/error::payload must return data, got {}",
            out.debug_repr()
        )
    });
    match t {
        Term::Nil => None,
        other => Some(other),
    }
}

fn expect_parse_error_matches_rust(
    ctx: &mut EvalCtx,
    env: &Env,
    got: Value,
    want: ParseError,
    src: &str,
) {
    let (want_code, want_at) = match want {
        ParseError::Eof => ("core/parse/eof", 0usize),
        ParseError::Unexpected { at, .. } => ("core/parse/unexpected", at),
        ParseError::Escape { at, .. } => ("core/parse/escape", at),
        ParseError::Int { at, .. } => ("core/parse/int", at),
    };

    let payload = unerror_payload(ctx, env, got).expect("expected ERROR payload");
    let Term::Map(m) = payload else {
        panic!("ERROR payload must be map, got {payload:?}");
    };

    let code = match m.get(&TermOrdKey(Term::symbol(":error/code"))) {
        Some(Term::Str(s)) => s.as_str(),
        other => panic!("expected :error/code string, got {other:?}"),
    };
    assert_eq!(code, want_code, "src: {src}");

    let msg = match m.get(&TermOrdKey(Term::symbol(":error/message"))) {
        Some(Term::Str(s)) => s.as_str(),
        other => panic!("expected :error/message string, got {other:?}"),
    };
    assert!(!msg.is_empty(), "error message must be non-empty");

    let at = match m.get(&TermOrdKey(Term::symbol(":error/context"))) {
        Some(Term::Map(ctxm)) => match ctxm.get(&TermOrdKey(Term::symbol(":at"))) {
            Some(Term::Int(i)) => i
                .to_string()
                .parse::<usize>()
                .expect(":at must fit usize in tests"),
            other => panic!("expected :at int, got {other:?}"),
        },
        other => panic!("expected :error/context map, got {other:?}"),
    };
    assert_eq!(at, want_at, "src: {src}");
}

#[test]
fn selfhost_parse_term_and_module_match_rust_parser_for_terms_and_errors() {
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    load_selfhost_parse(&mut env, &mut ctx);

    let parse_term_fn = env
        .get("selfhost/parse::parse-term")
        .expect("selfhost/parse::parse-term bound");
    let parse_module_fn = env
        .get("selfhost/parse::parse-module")
        .expect("selfhost/parse::parse-module bound");

    let term_cases = [
        "nil",
        "true",
        "false",
        "0",
        "123",
        "-1",
        "\"a\\n\\\"b\\t\"",
        "\"\\u0001\"",
        "\"\\x41\\xFF\"",
        "b\"\\x00\\xFF\"",
        "b\"\\u00A9\"",
        "foo/bar::x",
        "(a b)",
        "[1 2 3]",
        "[1 [2 3] 4]",
        "{:b 2 :a 1}",
        "{:a {:b 2}}",
        "'x",
    ];

    for t_src in term_cases {
        let want =
            parse_term(t_src).unwrap_or_else(|e| panic!("rust parse_term failed for {t_src}: {e}"));
        let got = parse_term_fn
            .clone()
            .apply(&mut ctx, Value::Data(Term::Str(t_src.to_string())))
            .unwrap_or_else(|e| panic!("selfhost parse-term apply failed for {t_src}: {e}"));
        let got_t = value_to_term(&got).unwrap_or_else(|| {
            panic!(
                "selfhost parse-term must return data for {t_src}, got {}",
                got.debug_repr()
            )
        });
        assert_eq!(got_t, want, "term case: {t_src}");
    }

    let module_src = r#"
      (def x   1)
      ; comment
      (def y (fn (a b) a))
      (y x 2)
    "#;
    let want_forms = parse_module(module_src).expect("rust parse_module");
    let got = parse_module_fn
        .clone()
        .apply(&mut ctx, Value::Data(Term::Str(module_src.to_string())))
        .expect("selfhost parse-module apply");
    let got_t = value_to_term(&got).unwrap_or_else(|| {
        panic!(
            "selfhost parse-module must return data, got {}",
            got.debug_repr()
        )
    });
    assert_eq!(got_t, Term::Vector(want_forms));

    let error_cases = [
        "",            // eof
        ")",           // unexpected delimiter
        "(",           // unterminated list
        "\"abc",       // unterminated string
        "\"\\q\"",     // unknown escape
        "\"\\uD800\"", // invalid unicode codepoint
        "b\"\\x0G\"",  // invalid hex digit in bytes literal
        "{:a 1 :b}",   // odd map arity
        "nil true",    // trailing tokens
    ];

    for src in error_cases {
        let want_err = parse_term(src).expect_err("rust parser must error");
        let got = parse_term_fn
            .clone()
            .apply(&mut ctx, Value::Data(Term::Str(src.to_string())))
            .expect("selfhost parse-term apply");
        expect_parse_error_matches_rust(&mut ctx, &env, got, want_err, src);
    }
}
