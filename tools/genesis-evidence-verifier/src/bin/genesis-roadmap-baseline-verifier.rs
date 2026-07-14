#[path = "../json.rs"]
mod json;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::PathBuf;

const VERSION: &str = "0.1.0";
const PAYLOAD_TYPE: &str = "application/vnd.genesiscode.roadmap-baseline.v0.1+json";

struct Args {
    bundle: PathBuf,
    public_key: PathBuf,
    expected_keyid: String,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut bundle = None;
        let mut public_key = None;
        let mut expected_keyid = None;
        let mut args = env::args().skip(1);
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| format!("missing value for {flag}"))?;
            match flag.as_str() {
                "--bundle" => set_once(&mut bundle, PathBuf::from(value), &flag)?,
                "--public-key" => set_once(&mut public_key, PathBuf::from(value), &flag)?,
                "--expected-keyid" => set_once(&mut expected_keyid, value, &flag)?,
                _ => return Err(format!("unknown option: {flag}")),
            }
        }
        Ok(Self {
            bundle: bundle.ok_or_else(|| "missing --bundle".to_owned())?,
            public_key: public_key.ok_or_else(|| "missing --public-key".to_owned())?,
            expected_keyid: expected_keyid.ok_or_else(|| "missing --expected-keyid".to_owned())?,
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
struct Report {
    kind: &'static str,
    ok: bool,
    verifier_version: &'static str,
    evidence_class: &'static str,
    signature_grants_authority: bool,
    keyid: String,
    baseline_identity_sha256: String,
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a serde_json::Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn string<'a>(object: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be a string"))
}

fn integer(object: &serde_json::Map<String, Value>, key: &str) -> Result<u64, String> {
    object
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{key} must be a nonnegative integer"))
}

fn exact_fields(
    object: &serde_json::Map<String, Value>,
    fields: &[&str],
    label: &str,
) -> Result<(), String> {
    let observed: std::collections::BTreeSet<_> = object.keys().map(String::as_str).collect();
    let expected: std::collections::BTreeSet<_> = fields.iter().copied().collect();
    if observed != expected {
        return Err(format!("{label} fields mismatch"));
    }
    Ok(())
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

fn run(args: Args) -> Result<Report, String> {
    let bundle_bytes = fs::read(&args.bundle).map_err(|error| format!("read bundle: {error}"))?;
    let bundle_value = json::parse_unique(&bundle_bytes, "baseline bundle")?;
    let bundle = object(&bundle_value, "baseline bundle")?;
    exact_fields(
        bundle,
        &[
            "authority",
            "envelope",
            "evidenceClass",
            "kind",
            "signing",
            "statement",
            "version",
        ],
        "baseline bundle",
    )?;
    if string(bundle, "kind")? != "genesis/roadmap-baseline-bundle-v0.1"
        || string(bundle, "version")? != "0.1"
        || string(bundle, "evidenceClass")? != "E0"
        || string(bundle, "authority")? != "observation"
    {
        return Err("baseline bundle authority/identity drift".to_owned());
    }
    let signing = object(bundle.get("signing").ok_or("missing signing")?, "signing")?;
    exact_fields(
        signing,
        &[
            "keyId",
            "publicKeySha256",
            "signatureGrantsAuthority",
            "trust",
        ],
        "signing",
    )?;
    if signing
        .get("signatureGrantsAuthority")
        .and_then(Value::as_bool)
        != Some(false)
        || string(signing, "trust")? != "externally-pinned-fixture-integrity-only"
    {
        return Err("baseline signature attempted authority escalation".to_owned());
    }

    let public = fs::read(&args.public_key).map_err(|error| format!("read public key: {error}"))?;
    let public_bytes: [u8; 32] = public
        .try_into()
        .map_err(|_| "public key must contain exactly 32 raw bytes")?;
    let public_sha = hex(&Sha256::digest(public_bytes));
    let keyid = format!("sha256:{public_sha}");
    if keyid != args.expected_keyid
        || string(signing, "keyId")? != keyid
        || string(signing, "publicKeySha256")? != public_sha
    {
        return Err("externally pinned public key identity mismatch".to_owned());
    }

    let envelope = object(
        bundle.get("envelope").ok_or("missing envelope")?,
        "envelope",
    )?;
    exact_fields(
        envelope,
        &["payload", "payloadType", "signatures"],
        "envelope",
    )?;
    if string(envelope, "payloadType")? != PAYLOAD_TYPE {
        return Err("baseline DSSE payload type mismatch".to_owned());
    }
    let payload = STANDARD
        .decode(string(envelope, "payload")?)
        .map_err(|_| "baseline payload is not canonical base64")?;
    let statement_value = json::parse_unique(&payload, "baseline payload")?;
    let mut expected_payload = json::canonical_bytes(&statement_value)?;
    expected_payload.push(b'\n');
    if payload != expected_payload || bundle.get("statement") != Some(&statement_value) {
        return Err("baseline payload/statement canonical bytes mismatch".to_owned());
    }
    let statement = object(&statement_value, "statement")?;
    exact_fields(
        statement,
        &[
            "authoritative",
            "baselineIdentitySha256",
            "build",
            "captureDate",
            "evidenceClass",
            "hostObservation",
            "kind",
            "overall",
            "version",
            "workloadPolicyIdentitySha256",
            "workloads",
        ],
        "statement",
    )?;
    if string(statement, "kind")? != "genesis/roadmap-baseline-statement-v0.1"
        || string(statement, "evidenceClass")? != "E0"
        || statement.get("authoritative").and_then(Value::as_bool) != Some(false)
    {
        return Err("baseline statement authority escalation".to_owned());
    }
    let baseline_identity = string(statement, "baselineIdentitySha256")?.to_owned();
    let mut identity_value = statement_value.clone();
    object_mut(&mut identity_value, "statement identity")?.remove("baselineIdentitySha256");
    let mut identity_bytes = json::canonical_bytes(&identity_value)?;
    identity_bytes.push(b'\n');
    if hex(&Sha256::digest(identity_bytes)) != baseline_identity {
        return Err("baseline statement identity mismatch".to_owned());
    }
    verify_observation_inventory(statement)?;

    let signatures = envelope
        .get("signatures")
        .and_then(Value::as_array)
        .ok_or("signatures must be an array")?;
    if signatures.len() != 1 {
        return Err("baseline requires exactly one fixture-integrity signature".to_owned());
    }
    let signature_row = object(&signatures[0], "signature")?;
    exact_fields(signature_row, &["keyid", "sig"], "signature")?;
    if string(signature_row, "keyid")? != keyid {
        return Err("signature keyid mismatch".to_owned());
    }
    let signature_bytes = STANDARD
        .decode(string(signature_row, "sig")?)
        .map_err(|_| "signature is not canonical base64")?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| "signature must be 64 bytes")?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_bytes).map_err(|_| "public key is invalid")?;
    verifying_key
        .verify(&pae(&payload), &signature)
        .map_err(|_| "baseline DSSE signature verification failed")?;
    Ok(Report {
        kind: "genesis/roadmap-baseline-verification-result-v0.1",
        ok: true,
        verifier_version: VERSION,
        evidence_class: "E0",
        signature_grants_authority: false,
        keyid,
        baseline_identity_sha256: baseline_identity,
    })
}

fn object_mut<'a>(
    value: &'a mut Value,
    label: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, String> {
    value
        .as_object_mut()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn verify_observation_inventory(statement: &serde_json::Map<String, Value>) -> Result<(), String> {
    let workloads = statement
        .get("workloads")
        .and_then(Value::as_array)
        .ok_or("workloads must be an array")?;
    if workloads.len() != 10 {
        return Err("baseline must contain exactly ten workloads".to_owned());
    }
    let expected_ids = [
        "PB-1", "PB-2", "PB-3", "PB-4", "PB-5", "PB-6", "PB-7", "PB-8", "PB-9", "PB-10",
    ];
    let expected_failures = std::collections::BTreeMap::from([
        ("PB-2", "runner-unavailable"),
        ("PB-3", "decision-not-approved"),
        ("PB-5", "budget-miss"),
        ("PB-6", "runner-unavailable"),
        ("PB-7", "budget-miss"),
        ("PB-8", "runner-unavailable"),
        ("PB-9", "runner-unavailable"),
        ("PB-10", "runner-unavailable"),
    ]);
    let mut observed_failures = std::collections::BTreeMap::new();
    let mut raw_samples = 0_usize;
    let mut warmups = 0_usize;
    for (index, value) in workloads.iter().enumerate() {
        let workload = object(value, "workload")?;
        let workload_id = string(workload, "id")?;
        if workload_id != expected_ids[index] {
            return Err("baseline workload order/identity drift".to_owned());
        }
        let samples = workload
            .get("samples")
            .and_then(Value::as_array)
            .ok_or("samples must be an array")?;
        let workload_warmups = workload
            .get("warmupSamples")
            .and_then(Value::as_array)
            .ok_or("warmupSamples must be an array")?;
        if matches!(workload_id, "PB-1" | "PB-4" | "PB-5" | "PB-7") {
            if samples.len() != 30 || workload_warmups.len() != 5 {
                return Err(format!("{workload_id} raw sample count drift"));
            }
        } else if !samples.is_empty() || !workload_warmups.is_empty() {
            return Err(format!("{workload_id} unavailable runner carries samples"));
        }
        raw_samples += samples.len();
        warmups += workload_warmups.len();
        let failures = workload
            .get("failures")
            .and_then(Value::as_array)
            .ok_or("failures must be an array")?;
        if let Some(failure) = failures.first() {
            if failures.len() != 1 {
                return Err(format!("{workload_id} failure cardinality drift"));
            }
            observed_failures.insert(workload_id, string(object(failure, "failure")?, "code")?);
        }
    }
    if raw_samples != 120 || warmups != 20 || observed_failures != expected_failures {
        return Err("baseline raw-sample/warmup/failure inventory drift".to_owned());
    }
    let overall = object(
        statement.get("overall").ok_or("missing overall")?,
        "overall",
    )?;
    if integer(overall, "budgetFailing")? != 2
        || integer(overall, "budgetPassing")? != 2
        || integer(overall, "decisionGated")? != 1
        || integer(overall, "observed")? != 4
        || integer(overall, "runnerUnavailable")? != 5
        || string(overall, "status")? != "observed-with-failures"
    {
        return Err("baseline overall summary drift".to_owned());
    }
    Ok(())
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
            eprintln!("genesis-roadmap-baseline-verifier: {error}");
            std::process::exit(1);
        }
    }
}
