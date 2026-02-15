use std::path::PathBuf;

use gc_coreform::{canonicalize_module, parse_module, print_module};

#[test]
fn selfhost_canon_gc_is_canonicalizable_and_idempotent() {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "selfhost",
        "canon.gc",
    ]
    .iter()
    .collect();
    let src = std::fs::read_to_string(&path).expect("read selfhost/canon.gc");

    let forms = parse_module(&src).expect("parse selfhost/canon.gc");
    let canon = match canonicalize_module(forms.clone()) {
        Ok(c) => c,
        Err(e) => {
            // Pinpoint the failing top-level form for fast iteration.
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

    // Idempotence: parse->canon->print->parse->canon->print stabilizes.
    let forms2 = parse_module(&printed).expect("parse printed module");
    let canon2 = canonicalize_module(forms2).expect("canonicalize printed module");
    assert_eq!(print_module(&canon2), printed);
}
