use gc_coreform::{canonicalize_form, parse_module, print_term};

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
            panic!("form {i} failed: {e}\n\nform:\n{}\n", print_term(&f));
        });
    }
}
