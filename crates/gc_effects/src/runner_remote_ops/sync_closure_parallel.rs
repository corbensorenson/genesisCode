pub(super) struct SyncPullStats<'a> {
    pub(super) pulled: &'a mut u64,
    pub(super) already: &'a mut u64,
    pub(super) store_written_bytes: &'a mut usize,
    pub(super) store_max_run_bytes: Option<usize>,
    pub(super) error_tok: SealId,
    pub(super) op: &'a str,
    pub(super) transfer_workers: usize,
    pub(super) max_artifact_bytes: usize,
    pub(super) max_batch_bytes: usize,
}

pub(super) fn sync_pull_closure(
    client: &gc_registry::RegistryClient,
    store: &ArtifactStore,
    root: &str,
    depth: u64,
    stats: &mut SyncPullStats<'_>,
) -> Result<(), Value> {
    use std::collections::{HashSet, VecDeque};

    let mut q: VecDeque<(String, u64)> = VecDeque::new();
    q.push_back((root.to_string(), depth));
    let mut seen: HashSet<String> = HashSet::new();
    let mut obj_count: u64 = 0;
    let base_batch_cap = (stats.transfer_workers.max(1) * 8).max(8);
    let by_budget = (stats.max_batch_bytes / stats.max_artifact_bytes.max(1)).max(1);
    let batch_cap = base_batch_cap.min(by_budget);

    while !q.is_empty() {
        let mut batch: Vec<(String, u64)> = Vec::new();
        while batch.len() < batch_cap {
            let Some((h, dleft)) = q.pop_front() else {
                break;
            };
            if !seen.insert(h.clone()) {
                continue;
            }
            obj_count = obj_count.saturating_add(1);
            if obj_count > 50_000 {
                return Err(mk_error(
                    stats.error_tok,
                    "core/sync/too-many-objects",
                    "closure exceeded 50k objects".to_string(),
                    Some(stats.op),
                ));
            }
            batch.push((h, dleft));
        }
        if batch.is_empty() {
            continue;
        }

        let mut missing_hashes: Vec<String> = Vec::new();
        for (h, _) in &batch {
            if store.path_for(h).exists() {
                if store.verify_hex(h).is_err() {
                    return Err(mk_error(
                        stats.error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {h}"),
                        Some(stats.op),
                    ));
                }
                *stats.already = stats.already.saturating_add(1);
            } else {
                missing_hashes.push(h.clone());
            }
        }

        if !missing_hashes.is_empty() {
            let dl_results = sync_parallel_store_get_bytes(
                client,
                &missing_hashes,
                stats.transfer_workers,
                stats.max_artifact_bytes,
                stats.max_batch_bytes,
            );
            for (i, h) in missing_hashes.iter().enumerate() {
                let bytes = match &dl_results[i] {
                    Ok(b) => b,
                    Err(e) => {
                        if let gc_registry::RegistryError::Protocol(msg) = e
                            && msg.contains("resource-limit:")
                        {
                            return Err(mk_error(
                                stats.error_tok,
                                "core/caps/resource-limit",
                                msg.split("resource-limit:")
                                    .nth(1)
                                    .unwrap_or(msg)
                                    .trim()
                                    .to_string(),
                                Some(stats.op),
                            ));
                        }
                        let code = registry_error_code(e, "core/sync/remote-auth");
                        return Err(mk_error(
                            stats.error_tok,
                            code,
                            format!("{e}"),
                            Some(stats.op),
                        ));
                    }
                };
                if let Some(limit) = stats.store_max_run_bytes {
                    let observed = (*stats.store_written_bytes).saturating_add(bytes.len());
                    if observed > limit {
                        return Err(mk_resource_limit_error(
                            stats.error_tok,
                            stats.op,
                            "store artifact bytes",
                            observed,
                            limit,
                        ));
                    }
                }
                let got = store.put_bytes(bytes).map_err(|e| {
                    mk_error(
                        stats.error_tok,
                        "core/store/io-error",
                        e.to_string(),
                        Some(stats.op),
                    )
                })?;
                if got != *h {
                    return Err(mk_error(
                        stats.error_tok,
                        "core/sync/hash-mismatch",
                        "remote bytes hash mismatch".to_string(),
                        Some(stats.op),
                    ));
                }
                *stats.store_written_bytes =
                    (*stats.store_written_bytes).saturating_add(bytes.len());
                *stats.pulled = stats.pulled.saturating_add(1);
            }
        }

        for (h, dleft) in batch {
            let t = match store_get_term(store, &h) {
                Ok(t) => t,
                Err(_) => continue,
            };

            // Commit closure: commit, base, patch, result snapshot, evidence, attestations, parents.
            if let Ok(c) = gc_vcs::Commit::from_term(&t) {
                if let Some(b) = c.base {
                    q.push_back((b, dleft));
                }
                q.push_back((c.patch, dleft));
                q.push_back((c.result, dleft));
                for x in c.evidence {
                    q.push_back((x, dleft));
                }
                for x in c.attestations {
                    q.push_back((x, dleft));
                }
                if dleft > 0 {
                    for p in c.parents {
                        q.push_back((p, dleft - 1));
                    }
                }
                continue;
            }

            // Patch closure: follow referenced values.
            if let Ok(p) = gc_vcs::Patch::from_term(&t) {
                for x in p.refs() {
                    q.push_back((x, dleft));
                }
                continue;
            }

            // Evidence closure: follow any referenced inputs/outputs/data.
            if let Ok(e) = gc_vcs::Evidence::from_term(&t) {
                for x in e.refs() {
                    q.push_back((x, dleft));
                }
                continue;
            }

            // Conflict closure: follow referenced snapshots and referenced handler/value hashes.
            if let Ok(c) = gc_vcs::Conflict::from_term(&t) {
                for x in c.refs() {
                    q.push_back((x, dleft));
                }
                continue;
            }

            // Snapshot closure: shallow refs.
            if let Ok(s) = gc_vcs::Snapshot::from_term(&t) {
                for x in s.shallow_refs() {
                    q.push_back((x, dleft));
                }
            }
        }
    }

    Ok(())
}

pub(super) fn sync_parallel_store_get_bytes(
    client: &gc_registry::RegistryClient,
    hashes: &[String],
    workers: usize,
    max_artifact_bytes: usize,
    max_batch_bytes: usize,
) -> Vec<Result<Vec<u8>, gc_registry::RegistryError>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    if hashes.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, 64).min(hashes.len());
    if workers <= 1 {
        let mut total: usize = 0;
        return hashes
            .iter()
            .map(|h| {
                client
                    .store_get_bounded(h, Some(max_artifact_bytes))
                    .and_then(|b| {
                        total = total.saturating_add(b.len());
                        if total > max_batch_bytes {
                            return Err(gc_registry::RegistryError::Protocol(format!(
                                "resource-limit: sync pull batch exceeded limit ({total} > {max_batch_bytes} bytes)"
                            )));
                        }
                        Ok(b)
                    })
            })
            .collect();
    }

    let next = Arc::new(AtomicUsize::new(0));
    let out: Arc<Mutex<Vec<Option<SyncBytesResult>>>> =
        Arc::new(Mutex::new((0..hashes.len()).map(|_| None).collect()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let out = Arc::clone(&out);
            let next = Arc::clone(&next);
            let c = client.clone();
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= hashes.len() {
                        break;
                    }
                    let res = c.store_get_bounded(&hashes[i], Some(max_artifact_bytes));
                    if let Ok(mut g) = out.lock() {
                        g[i] = Some(res);
                    } else {
                        return;
                    }
                }
            });
        }
    });
    let mut g = match out.lock() {
        Ok(g) => g,
        Err(_) => {
            return (0..hashes.len())
                .map(|_| {
                    Err(gc_registry::RegistryError::Protocol(
                        "sync get results lock poisoned".to_string(),
                    ))
                })
                .collect();
        }
    };
    let mut total: usize = 0;
    g.drain(..)
        .map(|x| {
            x.unwrap_or_else(|| {
                Err(gc_registry::RegistryError::Protocol(
                    "sync get worker produced no result".to_string(),
                ))
            })
                .and_then(|b| {
                    total = total.saturating_add(b.len());
                    if total > max_batch_bytes {
                        return Err(gc_registry::RegistryError::Protocol(format!(
                            "resource-limit: sync pull batch exceeded limit ({total} > {max_batch_bytes} bytes)"
                        )));
                    }
                    Ok(b)
                })
        })
        .collect()
}

pub(super) fn sync_parallel_store_has_chunks(
    client: &gc_registry::RegistryClient,
    chunks: &[Vec<String>],
    workers: usize,
) -> Vec<Result<BTreeMap<String, bool>, gc_registry::RegistryError>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    if chunks.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, 64).min(chunks.len());
    if workers <= 1 {
        return chunks.iter().map(|chunk| client.store_has(chunk)).collect();
    }

    let next = Arc::new(AtomicUsize::new(0));
    let out: Arc<Mutex<Vec<Option<SyncHasResult>>>> =
        Arc::new(Mutex::new((0..chunks.len()).map(|_| None).collect()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let out = Arc::clone(&out);
            let next = Arc::clone(&next);
            let c = client.clone();
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= chunks.len() {
                        break;
                    }
                    let res = c.store_has(&chunks[i]);
                    if let Ok(mut g) = out.lock() {
                        g[i] = Some(res);
                    } else {
                        return;
                    }
                }
            });
        }
    });
    let mut g = match out.lock() {
        Ok(g) => g,
        Err(_) => {
            return (0..chunks.len())
                .map(|_| {
                    Err(gc_registry::RegistryError::Protocol(
                        "sync has results lock poisoned".to_string(),
                    ))
                })
                .collect();
        }
    };
    g.drain(..)
        .map(|x| {
            x.unwrap_or_else(|| {
                Err(gc_registry::RegistryError::Protocol(
                    "sync has worker produced no result".to_string(),
                ))
            })
        })
        .collect()
}

pub(super) fn sync_parallel_upload_missing(
    client: &gc_registry::RegistryClient,
    store: &ArtifactStore,
    missing: &[String],
    workers: usize,
    max_chunk_bytes: Option<usize>,
) -> Vec<Result<(), String>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    if missing.is_empty() {
        return Vec::new();
    }
    let workers = workers.clamp(1, 64).min(missing.len());
    if workers <= 1 {
        return missing
            .iter()
            .map(|h| {
                let bytes = store.get_bytes(h).map_err(|e| format!("store-read:{e}"))?;
                client
                    .store_put_auto(h, &bytes, max_chunk_bytes)
                    .map_err(|e| format!("{e}"))
            })
            .collect();
    }

    let next = Arc::new(AtomicUsize::new(0));
    let out: Arc<Mutex<Vec<Option<SyncUploadResult>>>> =
        Arc::new(Mutex::new((0..missing.len()).map(|_| None).collect()));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let out = Arc::clone(&out);
            let next = Arc::clone(&next);
            let c = client.clone();
            let s = store.clone();
            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= missing.len() {
                        break;
                    }
                    let h = &missing[i];
                    let res = s
                        .get_bytes(h)
                        .map_err(|e| format!("store-read:{e}"))
                        .and_then(|bytes| {
                            c.store_put_auto(h, &bytes, max_chunk_bytes)
                                .map_err(|e| format!("{e}"))
                        });
                    if let Ok(mut g) = out.lock() {
                        g[i] = Some(res);
                    } else {
                        return;
                    }
                }
            });
        }
    });
    let mut g = match out.lock() {
        Ok(g) => g,
        Err(_) => {
            return (0..missing.len())
                .map(|_| Err("sync put results lock poisoned".to_string()))
                .collect();
        }
    };
    g.drain(..)
        .map(|x| x.unwrap_or_else(|| Err("sync put worker produced no result".to_string())))
        .collect()
}
