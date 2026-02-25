use super::*;
use gc_coreform::hash_term;

#[test]
fn host_ffi_policy_gate_requires_allowlisted_abi_id() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/ffi::call"]

[op."host/ffi::call"]
allow_abi_ids = ["abi.math.v1"]
allow_libraries = ["libmath.so"]
allow_symbols = ["sum_f64"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :result {:sum 3}}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (
            Term::symbol(":abi-id"),
            Term::Str("abi.other.v1".to_string()),
        ),
        (
            Term::symbol(":library"),
            Term::Str("libmath.so".to_string()),
        ),
        (Term::symbol(":symbol"), Term::Str("sum_f64".to_string())),
    ]);
    let out = call_capability(
        "host/ffi::call",
        &payload,
        policy.op_policy("host/ffi::call"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(70),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_ffi_typed_schema_requires_schema_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/ffi::call"]

[op."host/ffi::call"]
allow_abi_ids = ["abi.math.v1"]
allow_libraries = ["libmath.so"]
allow_symbols = ["sum_f64"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :result {:sum 3}}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (
            Term::symbol(":abi-id"),
            Term::Str("abi.math.v1".to_string()),
        ),
        (
            Term::symbol(":library"),
            Term::Str("libmath.so".to_string()),
        ),
        (Term::symbol(":symbol"), Term::Str("sum_f64".to_string())),
        (
            Term::symbol(":request-schema-id"),
            Term::Str("genesis/ffi.request.call.v1".to_string()),
        ),
        (
            Term::symbol(":response-schema-id"),
            Term::Str("genesis/ffi.response.call.v1".to_string()),
        ),
        (
            Term::symbol(":payload"),
            term_map([(Term::symbol(":args"), Term::Vector(vec![]))]),
        ),
    ]);
    let out = call_capability(
        "host/ffi::call",
        &payload,
        policy.op_policy("host/ffi::call"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(71),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_ffi_call_returns_deterministic_boundary_hashes() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/ffi::call"]

[op."host/ffi::call"]
allow_abi_ids = ["abi.math.v1"]
allow_libraries = ["libmath.so"]
allow_symbols = ["sum_f64"]
allow_schema_ids = ["genesis/ffi.request.call.v1", "genesis/ffi.response.call.v1"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :result {:sum 3}}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (
            Term::symbol(":abi-id"),
            Term::Str("abi.math.v1".to_string()),
        ),
        (
            Term::symbol(":library"),
            Term::Str("libmath.so".to_string()),
        ),
        (Term::symbol(":symbol"), Term::Str("sum_f64".to_string())),
        (
            Term::symbol(":request-schema-id"),
            Term::Str("genesis/ffi.request.call.v1".to_string()),
        ),
        (
            Term::symbol(":response-schema-id"),
            Term::Str("genesis/ffi.response.call.v1".to_string()),
        ),
        (Term::symbol(":payload"), Term::Map(Default::default())),
    ]);
    let out = call_capability(
        "host/ffi::call",
        &payload,
        policy.op_policy("host/ffi::call"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(72),
    )
    .expect("call capability");
    let Value::Data(Term::Map(mm)) = out else {
        panic!("expected data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    let Some(Term::Str(request_h)) = mm.get(&TermOrdKey(Term::symbol(":request-h"))) else {
        panic!("missing :request-h");
    };
    let Some(Term::Str(result_h)) = mm.get(&TermOrdKey(Term::symbol(":result-h"))) else {
        panic!("missing :result-h");
    };
    assert_eq!(request_h.len(), 64);
    assert_eq!(result_h.len(), 64);

    let request_term = term_map([
        (
            Term::symbol(":op"),
            Term::Symbol("host/ffi::call".to_string()),
        ),
        (Term::symbol(":payload"), payload.clone()),
    ]);
    let expected_request_h = blake3::Hash::from_bytes(hash_term(&request_term))
        .to_hex()
        .to_string();
    assert_eq!(request_h, &expected_request_h);

    let result = mm
        .get(&TermOrdKey(Term::symbol(":result")))
        .expect("missing :result");
    let expected_result_h = blake3::Hash::from_bytes(hash_term(result))
        .to_hex()
        .to_string();
    assert_eq!(result_h, &expected_result_h);
}

#[test]
fn host_ffi_buffer_pin_enforces_max_buffer_bytes() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/ffi::buffer-pin"]

[op."host/ffi::buffer-pin"]
allow_abi_ids = ["abi.math.v1"]
max_buffer_bytes = 4
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :handle \"buf-1\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let payload = term_map([
        (
            Term::symbol(":abi-id"),
            Term::Str("abi.math.v1".to_string()),
        ),
        (
            Term::symbol(":bytes"),
            Term::Bytes(b"12345".to_vec().into()),
        ),
    ]);
    let out = call_capability(
        "host/ffi::buffer-pin",
        &payload,
        policy.op_policy("host/ffi::buffer-pin"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(73),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/resource-limit");
}
