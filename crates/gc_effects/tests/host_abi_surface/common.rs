use gc_coreform::{Term, TermOrdKey, hash_module, parse_module};
use gc_effects::CapsPolicy;
use gc_kernel::Value;

pub(super) fn documented_host_abi_ops() -> Vec<String> {
    let spec = include_str!("../../../../docs/spec/HOST_ABI.md");
    let mut in_block = false;
    let mut ops = Vec::new();
    for line in spec.lines() {
        if line.contains("HOST_ABI_OPS_BEGIN") {
            in_block = true;
            continue;
        }
        if line.contains("HOST_ABI_OPS_END") {
            in_block = false;
            continue;
        }
        if !in_block {
            continue;
        }
        let Some(start) = line.find('`') else {
            continue;
        };
        let rest = &line[start + 1..];
        let Some(end) = rest.find('`') else {
            continue;
        };
        let op = &rest[..end];
        if op.contains("::") {
            ops.push(op.to_string());
        }
    }
    ops
}

pub(super) fn allow_policy_for(ops: &[String]) -> CapsPolicy {
    let allow = ops
        .iter()
        .map(|op| format!("\"{op}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let toml = format!("allow = [{allow}]");
    CapsPolicy::from_toml_str(&toml).expect("parse policy")
}

pub(super) fn mk_single_effect_program(op: &str) -> (Vec<Term>, [u8; 32]) {
    let src = format!(
        "
        (def prog
          (core/effect::perform
            '{op}
            {{}}
            (fn (x) (core/effect::pure x))))
        prog
    "
    );
    let forms = parse_module(&src).expect("parse module");
    let h = hash_module(&forms);
    (forms, h)
}

pub(super) fn sealed_error_code(value: &Value, error_tok: gc_kernel::SealId) -> Option<String> {
    let Value::Sealed { token, payload } = value else {
        return None;
    };
    if *token != error_tok {
        return None;
    }
    let Value::Data(Term::Map(m)) = payload.as_ref() else {
        return None;
    };
    match m.get(&TermOrdKey(Term::symbol(":error/code"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}
