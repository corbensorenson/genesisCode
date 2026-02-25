use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use base64ct::{Base64, Encoding};
use ed25519_dalek::SigningKey;
use gc_coreform::{Term, TermOrdKey};

use super::*;

fn term_map(entries: Vec<(&str, Term)>) -> Term {
    let mut mm = BTreeMap::new();
    for (k, v) in entries {
        mm.insert(TermOrdKey(Term::symbol(k)), v);
    }
    Term::Map(mm)
}

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn next_temp_root() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "genesis_host_bridge_runtime_tests_{}_{}",
        std::process::id(),
        n
    ))
}

fn with_test_workspace<F>(f: F)
where
    F: FnOnce(&Path),
{
    let _guard = test_lock().lock().expect("lock test cwd");
    let old = std::env::current_dir().expect("current dir");
    let root = next_temp_root();
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("cleanup previous temp dir");
    }
    std::fs::create_dir_all(&root).expect("create temp root");
    std::env::set_current_dir(&root).expect("set current dir");
    f(&root);
    std::env::set_current_dir(&old).expect("restore current dir");
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove temp dir");
    }
}

fn write_key_file(root: &Path, key_id: &str, body: &str) {
    let keys_dir = root
        .join(".genesis")
        .join("runtime")
        .join("backend")
        .join("keys");
    std::fs::create_dir_all(&keys_dir).expect("create keys dir");
    std::fs::write(keys_dir.join(format!("{key_id}.toml")), body).expect("write key file");
}

#[test]
fn crypto_ed25519_sign_verify_roundtrip_uses_key_provider_file() {
    with_test_workspace(|root| {
        let signing = SigningKey::from_bytes(&[9u8; 32]);
        let sk_b64 = Base64::encode_string(&signing.to_bytes());
        let pk_b64 = Base64::encode_string(&signing.verifying_key().to_bytes());
        write_key_file(
            root,
            "key-main",
            &format!("alg = \"ed25519\"\nsk_b64 = \"{sk_b64}\"\npk_b64 = \"{pk_b64}\"\n"),
        );

        let sign_payload = term_map(vec![
            (":algorithm", Term::Str("ed25519".to_string())),
            (":key-id", Term::Str("key-main".to_string())),
            (":message", Term::Bytes(b"hello".to_vec().into())),
            (":context", Term::Bytes(b"ctx".to_vec().into())),
        ]);
        let signed = crypto_sign(&sign_payload).expect("sign");
        let Term::Map(signed_map) = signed else {
            panic!("expected sign response map");
        };
        let Some(Term::Bytes(signature)) = signed_map.get(&TermOrdKey(Term::symbol(":signature")))
        else {
            panic!("missing :signature bytes");
        };
        assert_eq!(signature.len(), 64);

        let verify_payload = term_map(vec![
            (":algorithm", Term::Str("ed25519".to_string())),
            (":key-id", Term::Str("key-main".to_string())),
            (":message", Term::Bytes(b"hello".to_vec().into())),
            (":context", Term::Bytes(b"ctx".to_vec().into())),
            (":signature", Term::Bytes(signature.to_vec().into())),
        ]);
        let verified = crypto_verify(&verify_payload).expect("verify");
        let Term::Map(verified_map) = verified else {
            panic!("expected verify response map");
        };
        assert_eq!(
            verified_map.get(&TermOrdKey(Term::symbol(":valid"))),
            Some(&Term::Bool(true))
        );
    });
}

#[test]
fn crypto_hkdf_and_aead_roundtrip_with_packed_ciphertext() {
    with_test_workspace(|root| {
        let key_b64 = Base64::encode_string(b"this-is-a-test-symmetric-key-material");
        write_key_file(
            root,
            "sym-main",
            &format!("alg = \"symmetric\"\nkey_b64 = \"{key_b64}\"\n"),
        );

        let kdf_payload = term_map(vec![
            (":algorithm", Term::Str("hkdf-sha256".to_string())),
            (":key-id", Term::Str("sym-main".to_string())),
            (":info", Term::Bytes(b"context".to_vec().into())),
            (":salt", Term::Bytes(b"salt".to_vec().into())),
            (":length", Term::Int(32_i64.into())),
        ]);
        let kdf = crypto_kdf(&kdf_payload).expect("kdf");
        let Term::Map(kdf_map) = kdf else {
            panic!("expected kdf response map");
        };
        let Some(Term::Bytes(material)) = kdf_map.get(&TermOrdKey(Term::symbol(":key"))) else {
            panic!("missing :key bytes");
        };
        assert_eq!(material.len(), 32);

        let seal_payload = term_map(vec![
            (":algorithm", Term::Str("aes-256-gcm".to_string())),
            (":key-id", Term::Str("sym-main".to_string())),
            (":plaintext", Term::Bytes(b"payload-data".to_vec().into())),
            (":aad", Term::Bytes(b"aad".to_vec().into())),
            (":nonce", Term::Bytes(vec![7u8; 12].into())),
        ]);
        let sealed = crypto_aead_seal(&seal_payload).expect("seal");
        let Term::Map(sealed_map) = sealed else {
            panic!("expected seal response map");
        };
        let Some(Term::Bytes(packed_ciphertext)) =
            sealed_map.get(&TermOrdKey(Term::symbol(":ciphertext")))
        else {
            panic!("missing :ciphertext bytes");
        };
        assert!(packed_ciphertext.len() > 28);

        let open_payload = term_map(vec![
            (":algorithm", Term::Str("aes-256-gcm".to_string())),
            (":key-id", Term::Str("sym-main".to_string())),
            (
                ":ciphertext",
                Term::Bytes(packed_ciphertext.to_vec().into()),
            ),
            (":aad", Term::Bytes(b"aad".to_vec().into())),
        ]);
        let opened = crypto_aead_open(&open_payload).expect("open");
        let Term::Map(opened_map) = opened else {
            panic!("expected open response map");
        };
        assert_eq!(
            opened_map.get(&TermOrdKey(Term::symbol(":plaintext"))),
            Some(&Term::Bytes(b"payload-data".to_vec().into()))
        );

        let mut tampered = packed_ciphertext.to_vec();
        *tampered.last_mut().expect("non-empty ciphertext") ^= 0x01;
        let tampered_open_payload = term_map(vec![
            (":algorithm", Term::Str("aes-256-gcm".to_string())),
            (":key-id", Term::Str("sym-main".to_string())),
            (":ciphertext", Term::Bytes(tampered.into())),
            (":aad", Term::Bytes(b"aad".to_vec().into())),
        ]);
        let opened_tampered = crypto_aead_open(&tampered_open_payload).expect("open tampered");
        let Term::Map(opened_tampered_map) = opened_tampered else {
            panic!("expected tampered open response map");
        };
        assert_eq!(
            opened_tampered_map.get(&TermOrdKey(Term::symbol(":ok"))),
            Some(&Term::Bool(false))
        );
    });
}
