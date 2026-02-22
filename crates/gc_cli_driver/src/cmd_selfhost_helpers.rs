use super::*;
use sha2::{Digest, Sha256};

pub(super) fn percent_basis_points(part: usize, total: usize) -> u64 {
    if total == 0 {
        return 0;
    }
    ((part as u128 * 10_000u128) / total as u128) as u64
}

pub(super) fn percent_string_from_bps(bps: u64) -> String {
    format!("{}.{:02}%", bps / 100, bps % 100)
}

pub(super) fn write_content_addressed_artifact(
    store_dir: &Path,
    bytes: &[u8],
) -> Result<(String, PathBuf), CliError> {
    std::fs::create_dir_all(store_dir)
        .with_context(|| format!("create {}", store_dir.display()))
        .map_err(|e| cli_err(EX_IO, "io/mkdir", format!("{e}")))?;

    let hex = blake3::hash(bytes).to_hex().to_string();
    let path = store_dir.join(&hex);
    if !path.is_file() {
        std::fs::write(&path, bytes)
            .with_context(|| format!("write {}", path.display()))
            .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    }
    Ok((hex, path))
}

pub(super) fn extract_manifest_module_paths(
    manifest_text: &str,
) -> std::collections::BTreeSet<String> {
    manifest_text
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|candidate| {
            candidate.starts_with("selfhost/")
                && candidate.ends_with(".gc")
                && candidate
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/'))
        })
        .map(str::to_string)
        .collect()
}

pub(super) fn maybe_update_selfhost_freshness_metadata(
    out: &Path,
    artifact_bytes: &[u8],
) -> Result<(), CliError> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workspace_artifact = workspace_root.join(WORKSPACE_SELFHOST_TOOLCHAIN_ARTIFACT_REL);
    if normalize_path_for_compare(out) != normalize_path_for_compare(&workspace_artifact) {
        return Ok(());
    }

    let manifest_rel = "selfhost/toolchain_manifest.gc";
    let freshness_rel = "selfhost/toolchain.freshness.json";
    let manifest_path = workspace_root.join(manifest_rel);
    let freshness_path = workspace_root.join(freshness_rel);
    let manifest_bytes = std::fs::read(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let manifest_text = std::str::from_utf8(&manifest_bytes).map_err(|e| {
        cli_err(
            EX_PARSE,
            "selfhost/freshness",
            format!(
                "manifest is not valid utf-8 ({}): {e}",
                manifest_path.display()
            ),
        )
    })?;
    let module_paths = extract_manifest_module_paths(manifest_text);
    if module_paths.is_empty() {
        return Err(cli_err(
            EX_PARSE,
            "selfhost/freshness",
            format!(
                "no selfhost module paths found in manifest {}",
                manifest_path.display()
            ),
        ));
    }

    let mut source_hasher = Sha256::new();
    source_hasher.update(b"manifest\0");
    source_hasher.update(manifest_rel.as_bytes());
    source_hasher.update(b"\0");
    source_hasher.update(&manifest_bytes);

    for rel in module_paths {
        let module_path = workspace_root.join(&rel);
        let module_bytes = std::fs::read(&module_path)
            .with_context(|| format!("read {}", module_path.display()))
            .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
        source_hasher.update(b"\0module\0");
        source_hasher.update(rel.as_bytes());
        source_hasher.update(b"\0");
        source_hasher.update(module_bytes);
    }
    let source_hash_sha256 = format!("{:x}", source_hasher.finalize());
    let artifact_hash_sha256 = format!("{:x}", Sha256::digest(artifact_bytes));

    let payload = serde_json::json!({
        "kind": "genesis/selfhost-freshness-v0.1",
        "manifest": manifest_rel,
        "artifact": WORKSPACE_SELFHOST_TOOLCHAIN_ARTIFACT_REL,
        "source_hash_sha256": source_hash_sha256,
        "artifact_hash_sha256": artifact_hash_sha256,
    });
    let payload_bytes = serde_json::to_vec_pretty(&payload).map_err(|e| {
        cli_err(
            EX_INTERNAL,
            "selfhost/freshness",
            format!("json encode failed: {e}"),
        )
    })?;
    std::fs::write(&freshness_path, payload_bytes)
        .with_context(|| format!("write {}", freshness_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;
    Ok(())
}

fn normalize_path_for_compare(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        }
    })
}
