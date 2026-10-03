pub(super) struct SyncPullStats<'a, 'store> {
    pub(super) import: &'a mut crate::store::ArtifactImport<'store>,
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
    policy: &CapsPolicy,
    commit_authority: &mut Option<CommitAuthority>,
    stats: &mut SyncPullStats<'_, '_>,
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
            let staged = stats
                .import
                .contains(h)
                .map_err(|error| sync_import_error(error, stats.error_tok, stats.op))?;
            if staged {
                *stats.already = stats.already.saturating_add(1);
            } else if store.path_for(h).exists() {
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
            // Admit each batch into the request-owned overlay. Destination writes
            // wait for every requested root/ref closure to finish admission.
            let mut planned_bytes = stats
                .store_written_bytes
                .checked_add(stats.import.staged_bytes())
                .ok_or_else(|| {
                    mk_error(
                        stats.error_tok,
                        "core/caps/resource-limit",
                        "store artifact byte accounting overflow".to_string(),
                        Some(stats.op),
                    )
                })?;
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
                gc_registry::verify_store_object("store/get", h, bytes).map_err(|error| {
                    mk_error(
                        stats.error_tok,
                        registry_error_code(&error, "core/sync/remote-auth"),
                        error.to_string(),
                        Some(stats.op),
                    )
                })?;
                planned_bytes = planned_bytes.checked_add(bytes.len()).ok_or_else(|| {
                    mk_error(
                        stats.error_tok,
                        "core/caps/resource-limit",
                        "store artifact byte accounting overflow".to_string(),
                        Some(stats.op),
                    )
                })?;
                if let Some(limit) = stats.store_max_run_bytes {
                    let observed = planned_bytes;
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
            }
            for (h, result) in missing_hashes.iter().zip(&dl_results) {
                // Every result was admitted above; keep a fallible path rather
                // than assuming that a remote cannot return an error.
                let bytes = result.as_ref().map_err(|error| {
                    mk_error(
                        stats.error_tok,
                        registry_error_code(error, "core/sync/remote-auth"),
                        error.to_string(),
                        Some(stats.op),
                    )
                })?;
                stats
                    .import
                    .stage(h, bytes)
                    .map_err(|error| sync_import_error(error, stats.error_tok, stats.op))?;
            }
        }

        for (h, dleft) in batch {
            let bytes = stats
                .import
                .get_bytes(&h)
                .map_err(|error| sync_import_error(error, stats.error_tok, stats.op))?;
            let t = match std::str::from_utf8(&bytes)
                .ok()
                .and_then(|text| gc_coreform::parse_term(text).ok())
            {
                Some(term) => term,
                None => continue,
            };

            // Typed commits are admitted by GenesisCode before their references affect traversal.
            let commit = match CommitAuthority::validate_typed_commit(policy, commit_authority, &t)
            {
                Ok(commit) => commit,
                Err(error) => {
                    return Err(mk_error(
                        stats.error_tok,
                        "core/sync/bad-commit",
                        format!("commit authority rejected {h}: {error}"),
                        Some(stats.op),
                    ));
                }
            };
            if let Some(c) = commit {
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

pub(super) fn sync_import_error(
    error: crate::store::ImportError,
    error_tok: SealId,
    op: &str,
) -> Value {
    let code = match &error {
        crate::store::ImportError::ResourceLimit(_) => "core/caps/resource-limit",
        crate::store::ImportError::Identity(error) => {
            registry_error_code(error, "core/sync/remote-auth")
        }
        crate::store::ImportError::Store(EffectsError::Log(message))
            if message.contains("artifact store corruption") =>
        {
            "core/store/corruption"
        }
        _ => "core/store/io-error",
    };
    mk_error(error_tok, code, error.to_string(), Some(op))
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
