use gc_coreform::{parse_module, parse_term};
use gc_types::{ModuleForTypecheck, typecheck_package};

fn typecheck(source: &str) -> gc_types::TypecheckReport {
    typecheck_package(&[ModuleForTypecheck {
        path: "pattern/control.gc".to_string(),
        forms: parse_module(source).expect("parse typecheck fixture"),
        meta: Some(
            parse_term("{:exports [result] :types {result ?} :caps []}").expect("parse metadata"),
        ),
    }])
}

#[test]
fn typechecker_rejects_duplicate_function_parameters() {
    let report = typecheck("(def result (fn (item item) item))");
    assert!(!report.ok);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error == "pattern/control.gc: (fn ...) duplicate parameter: item"),
        "unexpected errors: {:?}",
        report.errors
    );
}

#[test]
fn typechecker_rejects_duplicate_let_bindings() {
    let report = typecheck("(def result (let ((item 1) (item 2)) item))");
    assert!(!report.ok);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error == "pattern/control.gc: (let ...) duplicate binding: item"),
        "unexpected errors: {:?}",
        report.errors
    );
}

#[test]
fn typechecker_rejects_destructuring_binders() {
    let report = typecheck("(def result (fn ((item)) item))");
    assert!(!report.ok);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error == "pattern/control.gc: (fn ...) parameters must be symbols"),
        "unexpected errors: {:?}",
        report.errors
    );
}
