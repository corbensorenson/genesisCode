use super::*;

#[test]
fn inline_slot_segment_mutates_in_place_until_runtime_is_shared() {
    let mut runtime = RuntimeEnv::new(
        Env::empty(),
        CompiledModuleCells::empty(),
        Arc::new(CompiledCoverageSites::default()),
        None,
    );
    let allocation = runtime.inline_slots.as_ptr();
    for _ in 0..512 {
        runtime.push_slot(Value::data(Term::Nil));
        assert_eq!(runtime.inline_slots.as_ptr(), allocation);
    }

    let mut fork = runtime.clone();
    fork.push_slot(Value::data(Term::Nil));
    assert!(!Shared::ptr_eq(&runtime.inline_slots, &fork.inline_slots));
    assert_eq!(runtime.inline_slots.len(), 512);
    assert_eq!(fork.inline_slots.len(), 513);
}
