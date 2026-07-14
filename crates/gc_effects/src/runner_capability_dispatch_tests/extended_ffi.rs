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
    let Some(Term::Map(mm)) = out.as_data() else {
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

#[test]
fn host_ffi_signed_policy_profile_requires_policy_artifact_fields() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/ffi::call"]

[op."host/ffi::call"]
signed_policy_required = true
policy_artifact_h = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
policy_key_id = "ops-root-ed25519"
evidence_mode = "deterministic"
allow_abi_ids = ["abi.math.v1"]
allow_libraries = ["libmath.so"]
allow_symbols = ["sum_f64"]
max_call_payload_bytes = 1024
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
        SealId(74),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn host_ffi_signed_policy_profile_enforces_call_payload_budget() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/ffi::call"]

[op."host/ffi::call"]
signed_policy_required = true
policy_artifact_h = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
policy_signature_h = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
policy_key_id = "ops-root-ed25519"
evidence_mode = "deterministic"
allow_abi_ids = ["abi.math.v1"]
allow_libraries = ["libmath.so"]
allow_symbols = ["sum_f64"]
max_call_payload_bytes = 8
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
            Term::symbol(":payload"),
            term_map([(Term::symbol(":args"), Term::Str("0123456789".to_string()))]),
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
        SealId(75),
    )
    .expect("call capability");
    assert_eq!(code_from_error(out), "core/caps/resource-limit");
}

#[test]
fn host_ffi_signed_policy_profile_emits_provenance_envelope() {
    let policy_artifact_h = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let policy_signature_h = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let policy_key_id = "ops-root-ed25519";
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/ffi::call"]

[op."host/ffi::call"]
signed_policy_required = true
policy_artifact_h = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
policy_signature_h = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
policy_key_id = "ops-root-ed25519"
evidence_mode = "deterministic"
allow_abi_ids = ["abi.math.v1"]
allow_libraries = ["libmath.so"]
allow_symbols = ["sum_f64"]
max_call_payload_bytes = 1024
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
        SealId(76),
    )
    .expect("call capability");
    let Some(Term::Map(mm)) = out.as_data() else {
        panic!("expected data map");
    };
    let Some(Term::Map(provenance)) = mm.get(&TermOrdKey(Term::symbol(":ffi-provenance"))) else {
        panic!("missing ffi provenance map");
    };
    assert_eq!(
        provenance.get(&TermOrdKey(Term::symbol(":policy-artifact-h"))),
        Some(&Term::Str(policy_artifact_h.to_string()))
    );
    assert_eq!(
        provenance.get(&TermOrdKey(Term::symbol(":policy-signature-h"))),
        Some(&Term::Str(policy_signature_h.to_string()))
    );
    assert_eq!(
        provenance.get(&TermOrdKey(Term::symbol(":policy-key-id"))),
        Some(&Term::Str(policy_key_id.to_string()))
    );
    assert_eq!(
        provenance.get(&TermOrdKey(Term::symbol(":evidence-mode"))),
        Some(&Term::Str("deterministic".to_string()))
    );
    let Some(Term::Str(request_h)) = mm.get(&TermOrdKey(Term::symbol(":request-h"))) else {
        panic!("missing :request-h");
    };
    let Some(Term::Str(result_h)) = mm.get(&TermOrdKey(Term::symbol(":result-h"))) else {
        panic!("missing :result-h");
    };
    assert_eq!(
        provenance.get(&TermOrdKey(Term::symbol(":request-h"))),
        Some(&Term::Str(request_h.clone()))
    );
    assert_eq!(
        provenance.get(&TermOrdKey(Term::symbol(":result-h"))),
        Some(&Term::Str(result_h.clone()))
    );
}
