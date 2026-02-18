use gc_coreform::{canonicalize_form, parse_module, print_term};

fn collect_bad_ifs(t: &gc_coreform::Term, out: &mut Vec<String>) {
    if let Some(items) = t.as_proper_list() {
        if let Some(gc_coreform::Term::Symbol(h)) = items.first()
            && h == "if"
            && items.len() != 4
        {
            out.push(format!(
                "bad (if ...) len={} term={}",
                items.len(),
                print_term(t)
            ));
        }
        for it in items {
            collect_bad_ifs(it, out);
        }
        return;
    }
    match t {
        gc_coreform::Term::Vector(xs) => {
            for x in xs {
                collect_bad_ifs(x, out);
            }
        }
        gc_coreform::Term::Map(m) => {
            for (k, v) in m.iter() {
                collect_bad_ifs(&k.0, out);
                collect_bad_ifs(v, out);
            }
        }
        _ => {}
    }
}

#[test]
fn selfhost_patch_schema_module_canonicalizes() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../selfhost/patch_schema_v1.gc"
    ))
    .unwrap();
    let forms = parse_module(&src).expect("parse module");
    for (i, f) in forms.into_iter().enumerate() {
        canonicalize_form(f.clone()).unwrap_or_else(|e| {
            let mut bad_ifs = Vec::new();
            collect_bad_ifs(&f, &mut bad_ifs);
            if !bad_ifs.is_empty() {
                panic!(
                    "form {i} failed: {e}\n\nbad ifs:\n{}\n\nform:\n{}\n",
                    bad_ifs.join("\n"),
                    print_term(&f)
                );
            }
            panic!("form {i} failed: {e}\n\nform:\n{}\n", print_term(&f));
        });
    }
}
