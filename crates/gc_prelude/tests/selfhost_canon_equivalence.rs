use std::path::Path;

use gc_coreform::{Term, canonicalize_module, parse_module, print_module};
use gc_kernel::{Apply, EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn value_to_term_vec(v: &Value) -> Vec<Term> {
    let Some(Term::Vector(xs)) = v.to_plain_term() else {
        panic!("expected vector value, got {}", v.debug_repr());
    };
    xs
}

#[test]
fn selfhost_canonicalize_module_matches_rust() {
    let canon_path = Path::new("/Users/corbensorenson/Documents/genesisCode/selfhost/canon.gc");
    let src = std::fs::read_to_string(canon_path).expect("read selfhost/canon.gc");
    let canon_forms = canonicalize_module(parse_module(&src).unwrap()).unwrap();

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let _ = eval_module(&mut ctx, &mut env, &canon_forms).unwrap();

    let canon_mod_fn = env
        .get("selfhost/canon::canonicalize-module")
        .expect("selfhost/canon::canonicalize-module bound");

    let module_cases = [
        r#"(def x   1) (def y (fn (a b) a)) (y x 2)"#,
        r#"(def f (fn (a b c) (prim int/add a (prim int/add b c))))"#,
        r#"(let ((x 1) (y 2)) (prim int/add x y) (prim int/sub y x))"#,
        r#"(quote (a (b c) [1 2] {:a 1 :b 2}))"#,
        r#"{:k1 (f 1 2 3) :k2 (if true 1 2)}"#,
        r#"(begin (def z 1) z)"#,
    ];

    for m_src in module_cases {
        let raw = parse_module(m_src).unwrap();
        let want = canonicalize_module(raw.clone()).unwrap();
        let want_s = print_module(&want);

        let got_v = canon_mod_fn
            .clone()
            .apply(&mut ctx, Value::data(Term::Vector(raw)))
            .unwrap();

        // If the self-host pass returns an ERROR, surface it as a hard failure.
        if let Value::Sealed { .. } = got_v {
            panic!(
                "selfhost canonicalize-module returned a sealed value for input:\n{}\nvalue={}",
                m_src,
                got_v.debug_repr()
            );
        }

        let got_forms = value_to_term_vec(&got_v);
        let got_s = print_module(&got_forms);
        assert_eq!(got_s, want_s, "module case:\n{m_src}");

        // Idempotence: canonicalizing already-canonical code is a no-op.
        let got2_v = canon_mod_fn
            .clone()
            .apply(&mut ctx, Value::data(Term::Vector(got_forms.clone())))
            .unwrap();
        let got2_forms = value_to_term_vec(&got2_v);
        assert_eq!(
            print_module(&got2_forms),
            got_s,
            "idempotence failed for module:\n{m_src}"
        );
    }
}
