use gc_coreform::{Term, TermOrdKey, canonicalize_form, parse_module, parse_term};

#[test]
fn diag_patch_refactor_file() {
    let src = std::fs::read_to_string("../../selfhost/patch_schema_refactor_v1.gc").unwrap();
    let forms = parse_module(&src).unwrap();
    for (i, f) in forms.into_iter().enumerate() {
        if let Err(e) = canonicalize_form(f.clone()) {
            panic!("form #{i} failed: {e:#}\nraw={}", gc_coreform::print_term(&f));
        }
    }
}

#[test]
fn diag_toolchain_paths() {
    let src = std::fs::read_to_string("../../selfhost/toolchain.gc").unwrap();
    let t = parse_term(&src).unwrap();
    let Term::Map(root) = t else { panic!("root map") };
    let Term::Vector(mods) = root.get(&TermOrdKey(Term::symbol(":modules"))).unwrap() else { panic!("mods vec") };
    let mut found = false;
    let mut total = 0usize;
    for m in mods {
        let Term::Map(mm) = m else { continue };
        if let Some(Term::Str(path)) = mm.get(&TermOrdKey(Term::symbol(":path"))) {
            total += 1;
            if path == "selfhost/patch_schema_refactor_v1.gc" {
                found = true;
            }
        }
    }
    assert!(found, "missing refactor path in {total} modules");
}
