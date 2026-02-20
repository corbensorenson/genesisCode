pub(super) fn call_capability(op: &str) {
    match op {
        "core/pkg-low::init" => {}
        "core/pkg-low::lock"
        | "core/pkg-low::install" => {}
        "core/vcs-low::diff"
        | "core/vcs-low::apply"
        | "core/vcs-low::log" => {}
        _ => {}
    }
}
