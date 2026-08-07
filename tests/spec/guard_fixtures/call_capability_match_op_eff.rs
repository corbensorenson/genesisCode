pub(super) fn call_capability_with_runtime(op_eff: &str) {
    match op_eff {
        "gfx/window::open" => {}
        "gpu/compute::create-buffer"
        | "gpu/compute::dispatch" => {}
        "editor/doc::open"
        | "editor/doc::apply-edit"
        | "editor/doc::save" => {}
        _ => {}
    }
}
