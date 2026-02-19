use anyhow::{Context, Result, bail};
use gc_effects::ArtifactStore;
use gc_registry::RegistryClient;

use crate::config::BenchConfig;
use crate::measure::best_of;

fn remote_file_url(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}

pub fn run_store_sync(cfg: &BenchConfig) -> Result<(u128, u128)> {
    let temp = tempfile::tempdir().context("create store/sync benchmark tempdir")?;
    let remote_root = temp.path().join("remote-registry");
    std::fs::create_dir_all(&remote_root)
        .with_context(|| format!("create {}", remote_root.display()))?;
    let remote = RegistryClient::new(&remote_file_url(&remote_root), None)
        .context("open remote file registry")?;

    let mut hashes: Vec<String> = Vec::new();
    for i in 0..64u32 {
        let mut bytes = vec![0u8; 4096];
        for (idx, b) in bytes.iter_mut().enumerate() {
            *b = ((i as usize + idx) % 251) as u8;
        }
        let hash = blake3::hash(&bytes).to_hex().to_string();
        remote
            .store_put(&hash, &bytes)
            .with_context(|| format!("seed remote artifact {hash}"))?;
        hashes.push(hash);
    }

    let store_root = temp.path().join("store-local");
    let store = ArtifactStore::open(&store_root).context("open local artifact store")?;
    let store_cycle_bytes = vec![7u8; 16 * 1024];

    let store_cycle_ms = best_of(cfg.warmups, cfg.repeats, || {
        let hex = store
            .put_bytes(&store_cycle_bytes)
            .context("store-cycle put_bytes")?;
        let out = store.get_bytes(&hex).context("store-cycle get_bytes")?;
        if out.len() != store_cycle_bytes.len() {
            bail!(
                "store-cycle length mismatch: {} != {}",
                out.len(),
                store_cycle_bytes.len()
            );
        }
        Ok(())
    })?;

    let sync_pull_ms = best_of(cfg.warmups, cfg.repeats, || {
        let sync_store_root = temp.path().join("store-sync");
        if sync_store_root.exists() {
            std::fs::remove_dir_all(&sync_store_root)
                .with_context(|| format!("remove {}", sync_store_root.display()))?;
        }
        let sync_store = ArtifactStore::open(&sync_store_root).context("open sync store")?;

        let presence = remote
            .store_has(&hashes)
            .context("sync store_has against remote")?;
        for hash in &hashes {
            if !presence.get(hash).copied().unwrap_or(false) {
                bail!("remote missing seeded artifact {hash}");
            }
            let bytes = remote
                .store_get(hash)
                .with_context(|| format!("sync store_get {hash}"))?;
            let got = sync_store
                .put_bytes(&bytes)
                .context("sync local put_bytes")?;
            if got != *hash {
                bail!("sync hash mismatch: expected {hash}, got {got}");
            }
        }
        Ok(())
    })?;

    Ok((store_cycle_ms, sync_pull_ms))
}
