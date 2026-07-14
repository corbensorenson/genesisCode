use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signer, SigningKey};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use zeroize::Zeroize;

const PAYLOAD_TYPE: &str = "application/vnd.genesiscode.roadmap-baseline.v0.1+json";

#[derive(Debug)]
struct Args {
    statement: PathBuf,
    secret_key: PathBuf,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut statement = None;
        let mut secret_key = None;
        let mut args = env::args().skip(1);
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| format!("missing value for {flag}"))?;
            match flag.as_str() {
                "--statement" => set_once(&mut statement, PathBuf::from(value), &flag)?,
                "--secret-key" => set_once(&mut secret_key, PathBuf::from(value), &flag)?,
                _ => return Err(format!("unknown option: {flag}")),
            }
        }
        Ok(Self {
            statement: statement.ok_or_else(|| "missing --statement".to_owned())?,
            secret_key: secret_key.ok_or_else(|| "missing --secret-key".to_owned())?,
        })
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("duplicate option: {flag}"));
    }
    *slot = Some(value);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignatureRow {
    keyid: String,
    sig: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    payload_type: &'static str,
    payload: String,
    signatures: Vec<SignatureRow>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SignedOutput {
    kind: &'static str,
    version: &'static str,
    keyid: String,
    public_key_base64: String,
    public_key_sha256: String,
    envelope: Envelope,
}

fn canonical_statement(path: &PathBuf) -> Result<Vec<u8>, String> {
    let bytes = fs::read(path).map_err(|error| format!("read statement: {error}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|error| format!("parse statement JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "statement must be an object".to_owned())?;
    if object.get("kind").and_then(Value::as_str) != Some("genesis/roadmap-baseline-statement-v0.1")
        || object.get("evidenceClass").and_then(Value::as_str) != Some("E0")
        || object.get("authoritative").and_then(Value::as_bool) != Some(false)
    {
        return Err(
            "producer only signs non-authoritative E0 roadmap baseline statements".to_owned(),
        );
    }
    let mut canonical =
        serde_json::to_vec(&value).map_err(|error| format!("canonicalize statement: {error}"))?;
    canonical.push(b'\n');
    Ok(canonical)
}

fn pae(payload: &[u8]) -> Vec<u8> {
    format!(
        "DSSEv1 {} {} {} ",
        PAYLOAD_TYPE.len(),
        PAYLOAD_TYPE,
        payload.len()
    )
    .into_bytes()
    .into_iter()
    .chain(payload.iter().copied())
    .collect()
}

fn run(args: Args) -> Result<SignedOutput, String> {
    let metadata = fs::symlink_metadata(&args.secret_key)
        .map_err(|error| format!("read secret key metadata: {error}"))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err("secret key must be a regular non-symlink file".to_owned());
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err("secret key permissions must deny group/other access".to_owned());
    }
    let mut secret =
        fs::read(&args.secret_key).map_err(|error| format!("read secret key: {error}"))?;
    if secret.len() != 32 {
        secret.zeroize();
        return Err("secret key must contain exactly 32 raw bytes".to_owned());
    }
    let mut seed = [0_u8; 32];
    seed.copy_from_slice(&secret);
    secret.zeroize();
    let signing_key = SigningKey::from_bytes(&seed);
    seed.zeroize();
    let public = signing_key.verifying_key().to_bytes();
    let public_sha256 = hex(&Sha256::digest(public));
    let keyid = format!("sha256:{public_sha256}");
    let payload = canonical_statement(&args.statement)?;
    let signature = signing_key.sign(&pae(&payload)).to_bytes();
    Ok(SignedOutput {
        kind: "genesis/roadmap-baseline-signature-v0.1",
        version: "0.1",
        keyid: keyid.clone(),
        public_key_base64: STANDARD.encode(public),
        public_key_sha256: public_sha256,
        envelope: Envelope {
            payload_type: PAYLOAD_TYPE,
            payload: STANDARD.encode(payload),
            signatures: vec![SignatureRow {
                keyid,
                sig: STANDARD.encode(signature),
            }],
        },
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn main() {
    let result = Args::parse().and_then(run);
    match result.and_then(|value| serde_json::to_string(&value).map_err(|error| error.to_string()))
    {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("genesis-evidence-producer: {error}");
            std::process::exit(1);
        }
    }
}
