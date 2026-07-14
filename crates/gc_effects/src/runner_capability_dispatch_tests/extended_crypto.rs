use super::*;

#[test]
fn core_crypto_hash_policy_enforces_algorithm_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["core/crypto::hash"]

[op."core/crypto::hash"]
allow_algorithms = ["blake3"]
max_input_bytes = 1024
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :algorithm \"blake3\" :digest b\"abcd\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let out = call_capability(
        "core/crypto::hash",
        &term_map([
            (
                Term::symbol(":algorithm"),
                Term::Symbol("sha256".to_string()),
            ),
            (Term::symbol(":data"), Term::Bytes(b"abc".to_vec().into())),
        ]),
        policy.op_policy("core/crypto::hash"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(24),
    )
    .expect("hash");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn core_crypto_sign_enforces_key_allowlist() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["core/crypto::sign"]

[op."core/crypto::sign"]
allow_algorithms = ["ed25519"]
allow_key_ids = ["key-main"]
max_message_bytes = 2048
max_context_bytes = 256
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :signature b\"sig\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();
    let out = call_capability(
        "core/crypto::sign",
        &term_map([
            (
                Term::symbol(":algorithm"),
                Term::Symbol("ed25519".to_string()),
            ),
            (Term::symbol(":key-id"), Term::Str("key-other".to_string())),
            (
                Term::symbol(":message"),
                Term::Bytes(b"msg".to_vec().into()),
            ),
        ]),
        policy.op_policy("core/crypto::sign"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(25),
    )
    .expect("sign");
    assert_eq!(code_from_error(out), "core/caps/policy-error");
}

#[test]
fn core_crypto_family_wasi_bridge_profile_returns_data() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = [
  "core/crypto::hash",
  "core/crypto::sign",
  "core/crypto::verify",
  "core/crypto::kdf",
  "core/crypto::aead-seal",
  "core/crypto::aead-open"
]

[op."core/crypto::hash"]
allow_algorithms = ["blake3"]
max_input_bytes = 4096
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :digest b\"hash\"}"

[op."core/crypto::sign"]
allow_algorithms = ["ed25519"]
allow_key_ids = ["key-main"]
max_message_bytes = 4096
max_context_bytes = 1024
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :signature b\"sig\"}"

[op."core/crypto::verify"]
allow_algorithms = ["ed25519"]
allow_key_ids = ["key-main"]
max_message_bytes = 4096
max_signature_bytes = 4096
max_context_bytes = 1024
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :valid true}"

[op."core/crypto::kdf"]
allow_algorithms = ["hkdf-sha256"]
allow_key_ids = ["key-main"]
max_info_bytes = 1024
max_salt_bytes = 1024
max_output_bytes = 128
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :material b\"kdf\" :length 32}"

[op."core/crypto::aead-seal"]
allow_algorithms = ["aes-256-gcm"]
allow_key_ids = ["key-main"]
max_plaintext_bytes = 8192
max_aad_bytes = 2048
max_nonce_bytes = 64
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :ciphertext b\"ct\" :nonce b\"nn\" :tag b\"tt\"}"

[op."core/crypto::aead-open"]
allow_algorithms = ["aes-256-gcm"]
allow_key_ids = ["key-main"]
max_ciphertext_bytes = 8192
max_aad_bytes = 2048
max_nonce_bytes = 64
max_tag_bytes = 64
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :plaintext b\"pt\"}"
"#,
    )
    .expect("caps");
    let mut budget = ArtifactBudgetState::default();

    let hash_out = call_capability(
        "core/crypto::hash",
        &term_map([
            (
                Term::symbol(":algorithm"),
                Term::Symbol("blake3".to_string()),
            ),
            (Term::symbol(":data"), Term::Bytes(b"abc".to_vec().into())),
        ]),
        policy.op_policy("core/crypto::hash"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(26),
    )
    .expect("hash");
    let Some(Term::Map(hash_mm)) = hash_out.as_data() else {
        panic!("expected hash map");
    };
    assert_eq!(
        hash_mm.get(&TermOrdKey(Term::symbol(":digest"))),
        Some(&Term::Bytes(b"hash".to_vec().into()))
    );

    let sign_out = call_capability(
        "core/crypto::sign",
        &term_map([
            (
                Term::symbol(":algorithm"),
                Term::Symbol("ed25519".to_string()),
            ),
            (Term::symbol(":key-id"), Term::Str("key-main".to_string())),
            (
                Term::symbol(":message"),
                Term::Bytes(b"message".to_vec().into()),
            ),
        ]),
        policy.op_policy("core/crypto::sign"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(27),
    )
    .expect("sign");
    let Some(Term::Map(sign_mm)) = sign_out.as_data() else {
        panic!("expected sign map");
    };
    assert_eq!(
        sign_mm.get(&TermOrdKey(Term::symbol(":signature"))),
        Some(&Term::Bytes(b"sig".to_vec().into()))
    );

    let verify_out = call_capability(
        "core/crypto::verify",
        &term_map([
            (
                Term::symbol(":algorithm"),
                Term::Symbol("ed25519".to_string()),
            ),
            (Term::symbol(":key-id"), Term::Str("key-main".to_string())),
            (
                Term::symbol(":message"),
                Term::Bytes(b"message".to_vec().into()),
            ),
            (
                Term::symbol(":signature"),
                Term::Bytes(b"sig".to_vec().into()),
            ),
        ]),
        policy.op_policy("core/crypto::verify"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(28),
    )
    .expect("verify");
    let Some(Term::Map(verify_mm)) = verify_out.as_data() else {
        panic!("expected verify map");
    };
    assert_eq!(
        verify_mm.get(&TermOrdKey(Term::symbol(":valid"))),
        Some(&Term::Bool(true))
    );

    let kdf_out = call_capability(
        "core/crypto::kdf",
        &term_map([
            (
                Term::symbol(":algorithm"),
                Term::Symbol("hkdf-sha256".to_string()),
            ),
            (Term::symbol(":key-id"), Term::Str("key-main".to_string())),
            (Term::symbol(":info"), Term::Bytes(b"info".to_vec().into())),
            (Term::symbol(":length"), Term::Int(32_i64.into())),
        ]),
        policy.op_policy("core/crypto::kdf"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(29),
    )
    .expect("kdf");
    let Some(Term::Map(kdf_mm)) = kdf_out.as_data() else {
        panic!("expected kdf map");
    };
    assert_eq!(
        kdf_mm.get(&TermOrdKey(Term::symbol(":length"))),
        Some(&Term::Int(32_i64.into()))
    );

    let seal_out = call_capability(
        "core/crypto::aead-seal",
        &term_map([
            (
                Term::symbol(":algorithm"),
                Term::Symbol("aes-256-gcm".to_string()),
            ),
            (Term::symbol(":key-id"), Term::Str("key-main".to_string())),
            (
                Term::symbol(":plaintext"),
                Term::Bytes(b"plaintext".to_vec().into()),
            ),
            (Term::symbol(":aad"), Term::Bytes(b"aad".to_vec().into())),
            (
                Term::symbol(":nonce"),
                Term::Bytes(b"nonce".to_vec().into()),
            ),
        ]),
        policy.op_policy("core/crypto::aead-seal"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(30),
    )
    .expect("aead-seal");
    let Some(Term::Map(seal_mm)) = seal_out.as_data() else {
        panic!("expected aead-seal map");
    };
    assert_eq!(
        seal_mm.get(&TermOrdKey(Term::symbol(":ciphertext"))),
        Some(&Term::Bytes(b"ct".to_vec().into()))
    );

    let open_out = call_capability(
        "core/crypto::aead-open",
        &term_map([
            (
                Term::symbol(":algorithm"),
                Term::Symbol("aes-256-gcm".to_string()),
            ),
            (Term::symbol(":key-id"), Term::Str("key-main".to_string())),
            (
                Term::symbol(":ciphertext"),
                Term::Bytes(b"ciphertext".to_vec().into()),
            ),
            (Term::symbol(":aad"), Term::Bytes(b"aad".to_vec().into())),
            (
                Term::symbol(":nonce"),
                Term::Bytes(b"nonce".to_vec().into()),
            ),
            (Term::symbol(":tag"), Term::Bytes(b"tag".to_vec().into())),
        ]),
        policy.op_policy("core/crypto::aead-open"),
        &policy,
        None,
        None,
        &mut budget,
        SealId(31),
    )
    .expect("aead-open");
    let Some(Term::Map(open_mm)) = open_out.as_data() else {
        panic!("expected aead-open map");
    };
    assert_eq!(
        open_mm.get(&TermOrdKey(Term::symbol(":plaintext"))),
        Some(&Term::Bytes(b"pt".to_vec().into()))
    );
}
