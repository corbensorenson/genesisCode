use std::path::PathBuf;

use gc_coreform::{canonicalize_module, parse_module, print_module};

#[test]
fn selfhost_cli_coreform_v1_is_canonicalizable_and_idempotent() {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "selfhost",
        "cli_coreform_v1.gc",
    ]
    .iter()
    .collect();
    let src = std::fs::read_to_string(&path).expect("read selfhost/cli_coreform_v1.gc");

    let forms = parse_module(&src).expect("parse selfhost/cli_coreform_v1.gc");
    let canon = match canonicalize_module(forms.clone()) {
        Ok(c) => c,
        Err(e) => {
            for (i, f) in forms.into_iter().enumerate() {
                if let Err(e2) = gc_coreform::canonicalize_form(f.clone()) {
                    panic!(
                        "canonicalize form {i} failed:\n{e2:#}\nform:\n{}",
                        gc_coreform::print_term(&f)
                    );
                }
            }
            panic!("{e:#}");
        }
    };
    let printed = print_module(&canon);

    let forms2 = parse_module(&printed).expect("parse printed module");
    let canon2 = canonicalize_module(forms2).expect("canonicalize printed module");
    assert_eq!(print_module(&canon2), printed);
}
