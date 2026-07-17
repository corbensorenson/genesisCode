use super::*;
use base64ct::{Base64, Encoding};
use ed25519_dalek::{Signature, Signer, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const KIND_BENCH: &str = "genesis/bench-v0.1";
const DRIVER_REL: &str = "scripts/lib/genesisbench_front_door.py";
const OPEN_AGENT_DRIVER_REL: &str = "scripts/lib/genesisbench_open_agent.py";
const REGISTRY_DRIVER_REL: &str = "scripts/lib/genesisbench_registry.py";
const MAX_CRYPTO_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const ALLOWED_PAYLOAD_TYPES: &[&str] = &[
    "application/vnd.genesiscode.genesisbench-submission.v0.1+json",
    "application/vnd.genesiscode.genesisbench-registry-event.v0.1+json",
    "application/vnd.genesiscode.genesisbench-registry-checkpoint.v0.1+json",
];

fn resolve_repo_root() -> Result<PathBuf, CliError> {
    let cwd = std::env::current_dir().map_err(|error| {
        cli_err(
            EX_IO,
            "bench/current-directory",
            format!("failed to resolve current directory: {error}"),
        )
    })?;
    for start in [cwd, PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")] {
        for candidate in start.ancestors() {
            if candidate.join(DRIVER_REL).is_file()
                && candidate.join(OPEN_AGENT_DRIVER_REL).is_file()
                && candidate.join(REGISTRY_DRIVER_REL).is_file()
                && candidate
                    .join("benchmarks/agent_tasks/v0.1/suite.json")
                    .is_file()
            {
                return Ok(candidate.to_path_buf());
            }
        }
    }
    Err(cli_err(
        EX_IO,
        "bench/authority-root-missing",
        "GenesisBench authorities are unavailable; run this command from a GenesisCode source tree",
    ))
}

pub(super) fn push_path(args: &mut Vec<String>, flag: &str, path: &Path) {
    args.push(flag.to_string());
    args.push(path.as_os_str().to_string_lossy().into_owned());
}

pub(super) fn runtime_paths(cli: &Cli, context: &str) -> Result<(PathBuf, PathBuf), CliError> {
    let genesis_bin = std::env::current_exe().map_err(|error| {
        cli_err(
            EX_IO,
            "bench/executable-unavailable",
            format!("{context}: failed to resolve genesis executable: {error}"),
        )
    })?;
    let artifact = resolved_selfhost_artifact_for_frontend(cli).ok_or_else(|| {
        cli_err(
            EX_IO,
            "bench/selfhost-artifact-required",
            format!(
                "{context}: pass --selfhost-artifact <file> or provide .genesis/selfhost/toolchain.gc"
            ),
        )
    })?;
    Ok((genesis_bin, artifact))
}

fn driver_args(cli: &Cli, cmd: &BenchCmd) -> Result<(&'static str, Vec<String>), CliError> {
    if let Some(args) = super::cmd_bench_open_agent::driver_args(cli, cmd)? {
        return Ok((OPEN_AGENT_DRIVER_REL, args));
    }
    let mut args = Vec::new();
    let mut driver = DRIVER_REL;
    match cmd {
        BenchCmd::Inspect { case, adapter } => {
            args.push("inspect".to_string());
            if let Some(case) = case {
                args.extend(["--case".to_string(), case.clone()]);
            }
            if let Some(adapter) = adapter {
                push_path(&mut args, "--adapter", adapter);
            }
        }
        BenchCmd::Run {
            case,
            adapter,
            out,
            adapter_executable,
            model_artifact,
            ablation,
        } => {
            args.extend(["run".to_string(), "--case".to_string(), case.clone()]);
            push_path(&mut args, "--adapter", adapter);
            push_path(&mut args, "--out", out);
            args.extend(["--ablation".to_string(), ablation.clone()]);
            if let Some(executable) = adapter_executable {
                push_path(&mut args, "--adapter-executable", executable);
            }
            if let Some(model_artifact) = model_artifact {
                push_path(&mut args, "--model-artifact", model_artifact);
            }
            let (genesis_bin, artifact) = runtime_paths(cli, "bench run")?;
            push_path(&mut args, "--genesis-bin", &genesis_bin);
            push_path(&mut args, "--selfhost-artifact", &artifact);
        }
        BenchCmd::ValidateRun { run } => {
            args.push("validate-run".to_string());
            push_path(&mut args, "--run", run);
        }
        BenchCmd::Score {
            case,
            candidate,
            out,
        } => {
            args.extend(["score".to_string(), "--case".to_string(), case.clone()]);
            push_path(&mut args, "--candidate", candidate);
            if let Some(out) = out {
                push_path(&mut args, "--out", out);
            }
            let (genesis_bin, artifact) = runtime_paths(cli, "bench score")?;
            push_path(&mut args, "--genesis-bin", &genesis_bin);
            push_path(&mut args, "--selfhost-artifact", &artifact);
        }
        BenchCmd::Replay { run } => {
            args.push("replay".to_string());
            push_path(&mut args, "--run", run);
            let (genesis_bin, artifact) = runtime_paths(cli, "bench replay")?;
            push_path(&mut args, "--genesis-bin", &genesis_bin);
            push_path(&mut args, "--selfhost-artifact", &artifact);
        }
        BenchCmd::Bundle { run, out } => {
            args.push("bundle".to_string());
            push_path(&mut args, "--run", run);
            push_path(&mut args, "--out", out);
        }
        BenchCmd::Submit {
            bundle,
            claim,
            outbox,
            submitter,
            key,
        } => {
            driver = REGISTRY_DRIVER_REL;
            args.push("submit".to_string());
            push_path(&mut args, "--bundle", bundle);
            push_path(&mut args, "--claim", claim);
            push_path(&mut args, "--outbox", outbox);
            args.extend(["--submitter".to_string(), submitter.clone()]);
            push_path(&mut args, "--key", key);
            let helper = std::env::current_exe().map_err(|error| {
                cli_err(
                    EX_IO,
                    "bench/executable-unavailable",
                    format!("bench submit: failed to resolve genesis executable: {error}"),
                )
            })?;
            push_path(&mut args, "--crypto-helper", &helper);
        }
        BenchCmd::RegistryInit {
            registry,
            policy,
            operator_key,
        } => {
            driver = REGISTRY_DRIVER_REL;
            args.push("init".to_string());
            push_path(&mut args, "--registry", registry);
            push_path(&mut args, "--policy", policy);
            push_path(&mut args, "--operator-key", operator_key);
            push_path(
                &mut args,
                "--crypto-helper",
                &std::env::current_exe().map_err(|error| {
                    cli_err(
                        EX_IO,
                        "bench/executable-unavailable",
                        format!("registry init: {error}"),
                    )
                })?,
            );
        }
        BenchCmd::RegistryAdmit {
            registry,
            submission,
            bundle,
            operator_key,
        } => {
            driver = REGISTRY_DRIVER_REL;
            args.push("admit".to_string());
            push_path(&mut args, "--registry", registry);
            push_path(&mut args, "--submission", submission);
            push_path(&mut args, "--bundle", bundle);
            push_path(&mut args, "--operator-key", operator_key);
            let (genesis_bin, artifact) = runtime_paths(cli, "bench registry-admit")?;
            push_path(&mut args, "--genesis-bin", &genesis_bin);
            push_path(&mut args, "--selfhost-artifact", &artifact);
            push_path(&mut args, "--crypto-helper", &genesis_bin);
        }
        BenchCmd::RegistryVerify { registry } => {
            driver = REGISTRY_DRIVER_REL;
            args.push("verify".to_string());
            push_path(&mut args, "--registry", registry);
            push_path(
                &mut args,
                "--crypto-helper",
                &std::env::current_exe().map_err(|error| {
                    cli_err(
                        EX_IO,
                        "bench/executable-unavailable",
                        format!("registry verify: {error}"),
                    )
                })?,
            );
            let (genesis_bin, artifact) = runtime_paths(cli, "bench registry-verify")?;
            push_path(&mut args, "--genesis-bin", &genesis_bin);
            push_path(&mut args, "--selfhost-artifact", &artifact);
        }
        BenchCmd::RegistryBuild { registry, out } => {
            driver = REGISTRY_DRIVER_REL;
            args.push("build".to_string());
            push_path(&mut args, "--registry", registry);
            push_path(&mut args, "--out", out);
            push_path(
                &mut args,
                "--crypto-helper",
                &std::env::current_exe().map_err(|error| {
                    cli_err(
                        EX_IO,
                        "bench/executable-unavailable",
                        format!("registry build: {error}"),
                    )
                })?,
            );
            let (genesis_bin, artifact) = runtime_paths(cli, "bench registry-build")?;
            push_path(&mut args, "--genesis-bin", &genesis_bin);
            push_path(&mut args, "--selfhost-artifact", &artifact);
        }
        BenchCmd::AgentCampaignPlan { .. }
        | BenchCmd::AgentPlan { .. }
        | BenchCmd::AgentRun { .. }
        | BenchCmd::AgentValidate { .. }
        | BenchCmd::AgentReplay { .. } => {
            return Err(cli_err(
                EX_INTERNAL,
                "bench/open-agent-dispatch",
                "Open Agent command reached the fixed-adapter driver",
            ));
        }
        BenchCmd::CryptoSign { .. } | BenchCmd::CryptoVerify { .. } => {
            return Err(cli_err(
                EX_INTERNAL,
                "bench/crypto-dispatch",
                "internal crypto command reached external driver",
            ));
        }
    }
    Ok((driver, args))
}

fn crypto_error(code: &'static str, message: impl Into<String>) -> CliError {
    cli_err(EX_VERIFY, code, message.into())
}

fn crypto_payload(path: &Path, payload_type: &str) -> Result<Vec<u8>, CliError> {
    if !ALLOWED_PAYLOAD_TYPES.contains(&payload_type) {
        return Err(crypto_error(
            "bench/crypto-payload-type",
            "unsupported DSSE payload type",
        ));
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        crypto_error(
            "bench/crypto-payload",
            format!("read payload metadata: {error}"),
        )
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(crypto_error(
            "bench/crypto-payload",
            "payload must be a regular non-symlink file",
        ));
    }
    let bytes = fs::read(path)
        .map_err(|error| crypto_error("bench/crypto-payload", format!("read payload: {error}")))?;
    if bytes.len() > MAX_CRYPTO_PAYLOAD_BYTES || !bytes.is_ascii() || !bytes.ends_with(b"\n") {
        return Err(crypto_error(
            "bench/crypto-payload",
            "payload must be bounded canonical ASCII JSON with one trailing newline",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| crypto_error("bench/crypto-payload", format!("parse payload: {error}")))?;
    if !value.is_object() {
        return Err(crypto_error(
            "bench/crypto-payload",
            "payload must be a JSON object",
        ));
    }
    let mut canonical = serde_json::to_vec(&value).map_err(|error| {
        crypto_error(
            "bench/crypto-payload",
            format!("canonicalize payload: {error}"),
        )
    })?;
    canonical.push(b'\n');
    if canonical != bytes {
        return Err(crypto_error(
            "bench/crypto-payload",
            "payload is not exact canonical JSON",
        ));
    }
    Ok(bytes)
}

fn pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    format!(
        "DSSEv1 {} {} {} ",
        payload_type.len(),
        payload_type,
        payload.len()
    )
    .into_bytes()
    .into_iter()
    .chain(payload.iter().copied())
    .collect()
}

fn key_id(public: &[u8; 32]) -> String {
    format!("sha256:{:x}", Sha256::digest(public))
}

fn crypto_output(cli: &Cli, data: serde_json::Value) -> Result<CmdOut, CliError> {
    let json = json_envelope_value(JsonEnvelope {
        ok: true,
        kind: KIND_BENCH,
        data: Some(data),
        error: None,
    })?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}

fn crypto_sign(
    cli: &Cli,
    payload: &Path,
    key: &Path,
    payload_type: &str,
) -> Result<CmdOut, CliError> {
    let payload = crypto_payload(payload, payload_type)?;
    let metadata = fs::symlink_metadata(key).map_err(|error| {
        crypto_error(
            "bench/crypto-key",
            format!("read signing key metadata: {error}"),
        )
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(crypto_error(
            "bench/crypto-key",
            "signing key must be a regular non-symlink file",
        ));
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(crypto_error(
            "bench/crypto-key",
            "signing key permissions must deny group and other access",
        ));
    }
    let key_file = gc_obligations::KeyFile::load(key)
        .map_err(|error| crypto_error("bench/crypto-key", format!("load signing key: {error}")))?;
    let signing = key_file.signing_key().map_err(|error| {
        crypto_error("bench/crypto-key", format!("decode signing key: {error}"))
    })?;
    let verifying = key_file
        .verifying_key()
        .map_err(|error| crypto_error("bench/crypto-key", format!("decode public key: {error}")))?;
    if signing.verifying_key() != verifying {
        return Err(crypto_error(
            "bench/crypto-key",
            "signing and public key material do not match",
        ));
    }
    let public = verifying.to_bytes();
    let keyid = key_id(&public);
    let signature = signing.sign(&pae(payload_type, &payload)).to_bytes();
    crypto_output(
        cli,
        serde_json::json!({
            "kind": "genesis/genesisbench-dsse-signature-v0.1",
            "version": "0.1.0",
            "keyId": keyid,
            "publicKeyBase64": Base64::encode_string(&public),
            "payloadSha256": format!("{:x}", Sha256::digest(&payload)),
            "envelope": {
                "payloadType": payload_type,
                "payload": Base64::encode_string(&payload),
                "signatures": [{"keyid": keyid, "sig": Base64::encode_string(&signature)}],
            },
        }),
    )
}

fn crypto_verify(
    cli: &Cli,
    envelope_path: &Path,
    public_b64: &str,
    expected_keyid: &str,
    payload_type: &str,
) -> Result<CmdOut, CliError> {
    let metadata = fs::symlink_metadata(envelope_path).map_err(|error| {
        crypto_error(
            "bench/crypto-envelope",
            format!("read envelope metadata: {error}"),
        )
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(crypto_error(
            "bench/crypto-envelope",
            "envelope must be a regular non-symlink file",
        ));
    }
    let bytes = fs::read(envelope_path).map_err(|error| {
        crypto_error("bench/crypto-envelope", format!("read envelope: {error}"))
    })?;
    if bytes.len() > MAX_CRYPTO_PAYLOAD_BYTES * 2 {
        return Err(crypto_error(
            "bench/crypto-envelope",
            "envelope exceeds finite size limit",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        crypto_error("bench/crypto-envelope", format!("parse envelope: {error}"))
    })?;
    let object = value
        .as_object()
        .ok_or_else(|| crypto_error("bench/crypto-envelope", "envelope must be an object"))?;
    let expected_fields = ["payload", "payloadType", "signatures"]
        .into_iter()
        .collect::<BTreeSet<_>>();
    if object.keys().map(String::as_str).collect::<BTreeSet<_>>() != expected_fields {
        return Err(crypto_error(
            "bench/crypto-envelope",
            "envelope fields are not closed",
        ));
    }
    if object
        .get("payloadType")
        .and_then(serde_json::Value::as_str)
        != Some(payload_type)
    {
        return Err(crypto_error(
            "bench/crypto-envelope",
            "envelope payload type mismatch",
        ));
    }
    let public_vec = Base64::decode_vec(public_b64)
        .map_err(|error| crypto_error("bench/crypto-key", format!("decode public key: {error}")))?;
    let public: [u8; 32] = public_vec.try_into().map_err(|_| {
        crypto_error(
            "bench/crypto-key",
            "public key must contain exactly 32 bytes",
        )
    })?;
    if key_id(&public) != expected_keyid {
        return Err(crypto_error(
            "bench/crypto-key",
            "public key identity mismatch",
        ));
    }
    let signatures = object
        .get("signatures")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| crypto_error("bench/crypto-envelope", "signatures must be an array"))?;
    if signatures.len() != 1 {
        return Err(crypto_error(
            "bench/crypto-envelope",
            "envelope must carry exactly one signature",
        ));
    }
    let signature_object = signatures[0]
        .as_object()
        .ok_or_else(|| crypto_error("bench/crypto-envelope", "signature must be an object"))?;
    if signature_object
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != ["keyid", "sig"].into_iter().collect()
        || signature_object
            .get("keyid")
            .and_then(serde_json::Value::as_str)
            != Some(expected_keyid)
    {
        return Err(crypto_error(
            "bench/crypto-envelope",
            "signature fields or key identity mismatch",
        ));
    }
    let payload = Base64::decode_vec(
        object
            .get("payload")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| crypto_error("bench/crypto-envelope", "payload must be base64"))?,
    )
    .map_err(|error| crypto_error("bench/crypto-envelope", format!("decode payload: {error}")))?;
    if payload.len() > MAX_CRYPTO_PAYLOAD_BYTES {
        return Err(crypto_error(
            "bench/crypto-envelope",
            "payload exceeds finite size limit",
        ));
    }
    let signature_bytes = Base64::decode_vec(
        signature_object
            .get("sig")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| crypto_error("bench/crypto-envelope", "signature must be base64"))?,
    )
    .map_err(|error| {
        crypto_error(
            "bench/crypto-envelope",
            format!("decode signature: {error}"),
        )
    })?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|error| {
        crypto_error(
            "bench/crypto-envelope",
            format!("decode Ed25519 signature: {error}"),
        )
    })?;
    VerifyingKey::from_bytes(&public)
        .map_err(|error| {
            crypto_error(
                "bench/crypto-key",
                format!("decode Ed25519 public key: {error}"),
            )
        })?
        .verify_strict(&pae(payload_type, &payload), &signature)
        .map_err(|_| {
            crypto_error(
                "bench/crypto-signature",
                "Ed25519 signature verification failed",
            )
        })?;
    crypto_output(
        cli,
        serde_json::json!({
            "kind": "genesis/genesisbench-dsse-verification-v0.1",
            "version": "0.1.0",
            "verified": true,
            "keyId": expected_keyid,
            "payloadType": payload_type,
            "payloadSha256": format!("{:x}", Sha256::digest(&payload)),
        }),
    )
}

pub(super) fn cmd_bench(cli: &Cli, cmd: &BenchCmd) -> Result<CmdOut, CliError> {
    match cmd {
        BenchCmd::CryptoSign {
            payload,
            key,
            payload_type,
        } => {
            return crypto_sign(cli, payload, key, payload_type);
        }
        BenchCmd::CryptoVerify {
            envelope,
            public_key_base64,
            expected_keyid,
            payload_type,
        } => {
            return crypto_verify(
                cli,
                envelope,
                public_key_base64,
                expected_keyid,
                payload_type,
            );
        }
        _ => {}
    }
    let root = resolve_repo_root()?;
    let (driver_rel, args) = driver_args(cli, cmd)?;
    let driver = root.join(driver_rel);
    if driver.is_symlink() || !driver.is_file() {
        return Err(cli_err(
            EX_IO,
            "bench/driver-invalid",
            format!(
                "benchmark front-door driver is not a regular file: {}",
                driver.display()
            ),
        ));
    }
    let output = Command::new("python3")
        .arg(&driver)
        .args(&args)
        .current_dir(&root)
        .output()
        .map_err(|error| {
            cli_err(
                EX_IO,
                "bench/driver-spawn",
                format!("failed to execute canonical benchmark front door: {error}"),
            )
        })?;
    if output.stdout.len() > 16 * 1024 * 1024 || output.stderr.len() > 16 * 1024 * 1024 {
        return Err(cli_err(
            EX_VERIFY,
            "bench/output-limit",
            "benchmark front door exceeded the 16 MiB command output ceiling",
        ));
    }
    if !output.status.success() {
        let message = serde_json::from_slice::<serde_json::Value>(&output.stderr)
            .ok()
            .and_then(|value| {
                value
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| String::from_utf8_lossy(&output.stderr).trim().to_string());
        return Err(cli_err(
            EX_VERIFY,
            "bench/front-door-failed",
            if message.is_empty() {
                format!("benchmark front door exited with {}", output.status)
            } else {
                message
            },
        ));
    }
    let data: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        cli_err(
            EX_VERIFY,
            "bench/output-invalid",
            format!("benchmark front door emitted invalid JSON: {error}"),
        )
    })?;
    let json = json_envelope_value(JsonEnvelope {
        ok: true,
        kind: KIND_BENCH,
        data: Some(data),
        error: None,
    })?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}
