use super::*;

pub(super) fn visit(
    closure: &Shared<crate::value::CompiledClosureData>,
    active: &mut HashSet<CycleKey>,
    stack: &mut Vec<Work>,
    units: &mut u64,
) {
    let Some(lexical) = closure
        .compiled_env
        .as_ref()
        .filter(|env| env.has_captures())
    else {
        // The incoming environment edge was already charged by the caller.
        stack.push(Work::Env(closure.env.clone()));
        return;
    };
    let key = CycleKey::CompiledEnvironment(closure.identity_ptr());
    if !enter_cycle(active, stack, key) {
        return;
    }
    *units = units.saturating_add(1);
    let root = closure.env.module_anchor();
    push_edge(stack, Work::Env(root.clone()), units);
    // Env::capture has already flattened selected external locals above this
    // module anchor. Merge lexical slots into that one semantic frame; neither
    // sparse slot holes nor immutable compiler metadata are language charges.
    if !Shared::ptr_eq(&root.0, &closure.env.0) {
        let bindings = closure.env.0.binds.borrow();
        for (name, value) in bindings.iter().rev() {
            if !lexical.has_named_binding(name) {
                *units = units.saturating_add(name.len() as u64);
                push_edge(stack, Work::Value(value.clone()), units);
            }
        }
    }
    for (name, value) in lexical.named_bindings().rev() {
        *units = units.saturating_add(name.len() as u64);
        push_edge(stack, Work::Value(value.clone()), units);
    }
}
