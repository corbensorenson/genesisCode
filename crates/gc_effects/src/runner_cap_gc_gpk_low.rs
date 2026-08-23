use super::*;

#[path = "runner_cap_gc_gpk_low/gpk_ops.rs"]
mod gpk_ops;

#[expect(
    clippy::too_many_arguments,
    reason = "host capability dispatch wiring keeps explicit context parameters visible"
)]
pub(super) fn capability_gc_gpk_low(
    op_eff: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    refs_authority: Option<&mut RefsAuthority>,
    gc_authority: Option<&mut GcAuthority>,
    pkg_lock_read_authority: Option<&mut PkgLockReadAuthority>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    let _ = timeout_ms;
    let mut gc_authority = gc_authority;
    let mut refs_authority = refs_authority;
    match op_eff {
        "core/gc-low::plan" => {
            let authority = gc_authority.as_deref_mut().ok_or_else(|| {
                EffectsError::Log(
                    "core/gc-low::plan requires the artifact-loaded GenesisCode GC authority"
                        .to_string(),
                )
            })?;
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gc-low::plan".to_string())
            })?;

            let base_dir = effective_base_dir(pol)?;
            let lock_s = payload_gc_lock(payload).unwrap_or_else(|| "genesis.lock".to_string());
            let pins_s =
                payload_gc_pins(payload).unwrap_or_else(|| ".genesis/pins.toml".to_string());
            let depth = payload_gc_depth(payload).unwrap_or(200);
            let include_lock = payload_gc_include_lock(payload).unwrap_or(true);
            let include_refs = payload_gc_include_refs(payload).unwrap_or(true);

            let (refs_entries, lock_info, pins_document) = match gc_build_sources(
                refs,
                &base_dir,
                &lock_s,
                &pins_s,
                include_lock,
                pkg_lock_read_authority,
                error_tok,
                op,
            ) {
                Ok(v) => v,
                Err(v) => return Ok(v),
            };
            let roots_plan = authority
                .roots(
                    refs_entries,
                    lock_info,
                    pins_document,
                    include_lock,
                    include_refs,
                )
                .map_err(|error| EffectsError::Log(error.to_string()))?;

            let mut live: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for hash in &roots_plan.roots {
                match gc_closure_local(store, authority, hash, depth, &mut live, error_tok, op) {
                    Ok(()) => {}
                    Err(v) => return Ok(v),
                }
            }

            let store_dir = store.root_dir();
            let _lk = gc_store_lock(store_dir)?;
            let inventory = gc_store_inventory(store)?;
            let dead_plan = authority.dead_plan(live.iter().cloned().collect(), inventory)?;

            let largest_term: Vec<Term> = dead_plan
                .largest
                .into_iter()
                .map(|(h, b)| {
                    Term::Map(
                        [
                            (TermOrdKey(Term::symbol(":hash")), Term::Str(h)),
                            (
                                TermOrdKey(Term::symbol(":bytes")),
                                Term::Int((b as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    )
                })
                .collect();

            let dead_sample: Vec<Term> = dead_plan
                .dead
                .iter()
                .take(50)
                .cloned()
                .map(Term::Str)
                .collect();

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(
                TermOrdKey(Term::symbol(":live")),
                Term::Int((live.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":dead")),
                Term::Int((dead_plan.dead.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":reclaim-bytes")),
                Term::Int(dead_plan.reclaim_bytes.into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":roots")),
                Term::Vector(roots_plan.metadata),
            );
            m.insert(
                TermOrdKey(Term::symbol(":largest")),
                Term::Vector(largest_term),
            );
            m.insert(
                TermOrdKey(Term::symbol(":dead-sample")),
                Term::Vector(dead_sample),
            );
            Ok(Value::data(Term::Map(m)))
        }
        "core/gc-low::run" => {
            let authority = gc_authority.as_deref_mut().ok_or_else(|| {
                EffectsError::Log(
                    "core/gc-low::run requires the artifact-loaded GenesisCode GC authority"
                        .to_string(),
                )
            })?;
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/gc-low::run".to_string())
            })?;

            let base_dir = effective_base_dir(pol)?;
            let lock_s = payload_gc_lock(payload).unwrap_or_else(|| "genesis.lock".to_string());
            let pins_s =
                payload_gc_pins(payload).unwrap_or_else(|| ".genesis/pins.toml".to_string());
            let depth = payload_gc_depth(payload).unwrap_or(200);
            let include_lock = payload_gc_include_lock(payload).unwrap_or(true);
            let include_refs = payload_gc_include_refs(payload).unwrap_or(true);
            let quarantine = payload_gc_quarantine(payload).unwrap_or(false);
            let quarantine_dir_s = payload_gc_quarantine_dir(payload);

            let (refs_entries, lock_info, pins_document) = match gc_build_sources(
                refs,
                &base_dir,
                &lock_s,
                &pins_s,
                include_lock,
                pkg_lock_read_authority,
                error_tok,
                op,
            ) {
                Ok(v) => v,
                Err(v) => return Ok(v),
            };
            let roots_plan = authority
                .roots(
                    refs_entries,
                    lock_info,
                    pins_document,
                    include_lock,
                    include_refs,
                )
                .map_err(|error| EffectsError::Log(error.to_string()))?;

            let mut live: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for hash in &roots_plan.roots {
                match gc_closure_local(store, authority, hash, depth, &mut live, error_tok, op) {
                    Ok(()) => {}
                    Err(v) => return Ok(v),
                }
            }

            let store_dir = store.root_dir();
            let _lk = gc_store_lock(store_dir)?;
            let inventory = gc_store_inventory(store)?;
            let dead_plan = authority.dead_plan(live.iter().cloned().collect(), inventory)?;

            let quarantine_dir = if quarantine {
                Some(match quarantine_dir_s {
                    Some(s) => sandbox_path_write(&base_dir, &s, true).map_err(|e| {
                        EffectsError::Log(format!("quarantine dir path error: {e}"))
                    })?,
                    None => store_dir.parent().unwrap_or(store_dir).join("quarantine"),
                })
            } else {
                None
            };
            if let Some(qd) = &quarantine_dir {
                std::fs::create_dir_all(qd)?;
            }

            let mut deleted: u64 = 0;
            let mut quarantined: u64 = 0;
            for hash in &dead_plan.dead {
                let path = store_dir.join(hash);
                let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
                    EffectsError::Log(format!(
                        "authorized GC object changed before mutation ({hash}): {error}"
                    ))
                })?;
                if !metadata.file_type().is_file() {
                    return Err(EffectsError::Log(format!(
                        "authorized GC object is no longer a regular file: {hash}"
                    )));
                }
                store.verify_hex(hash).map_err(|error| {
                    EffectsError::Log(format!(
                        "authorized GC object identity changed before mutation ({hash}): {error}"
                    ))
                })?;
                if let Some(qd) = &quarantine_dir {
                    let qp = qd.join(hash);
                    if qp.exists() {
                        return Err(EffectsError::Log(format!(
                            "authorized GC quarantine destination already exists: {hash}"
                        )));
                    }
                    std::fs::rename(&path, &qp)?;
                    quarantined = quarantined.saturating_add(1);
                } else {
                    std::fs::remove_file(&path)?;
                    deleted = deleted.saturating_add(1);
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(
                TermOrdKey(Term::symbol(":dead")),
                Term::Int((dead_plan.dead.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":deleted")),
                Term::Int((deleted as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":quarantined")),
                Term::Int((quarantined as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":reclaimed-bytes")),
                Term::Int(dead_plan.reclaim_bytes.into()),
            );
            Ok(Value::data(Term::Map(m)))
        }
        "core/gc-low::pin" => {
            let authority = gc_authority.as_deref_mut().ok_or_else(|| {
                EffectsError::Log(
                    "core/gc-low::pin requires the artifact-loaded GenesisCode GC authority"
                        .to_string(),
                )
            })?;
            let base_dir = effective_base_dir(pol)?;
            let pins_s =
                payload_gc_pins(payload).unwrap_or_else(|| ".genesis/pins.toml".to_string());
            let target = payload_gc_target(payload)?;

            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let pins_path = sandbox_path_write(&base_dir, &pins_s, create_dirs)?;
            let _pins_lock = gc_path_lock(&pins_path)?;
            let document = gc_pins_document_at(&pins_path).map_err(EffectsError::Log)?;
            let plan = authority.update_pins(":pin", &target, document)?;
            atomic_write_text(&pins_path, &plan.body)?;

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":pins")), Term::Str(pins_s));
            m.insert(
                TermOrdKey(Term::symbol(":keep")),
                Term::Vector(plan.keep.into_iter().map(Term::Str).collect()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":keep-refs")),
                Term::Vector(plan.keep_refs.into_iter().map(Term::Str).collect()),
            );
            Ok(Value::data(Term::Map(m)))
        }
        "core/gc-low::unpin" => {
            let authority = gc_authority.as_deref_mut().ok_or_else(|| {
                EffectsError::Log(
                    "core/gc-low::unpin requires the artifact-loaded GenesisCode GC authority"
                        .to_string(),
                )
            })?;
            let base_dir = effective_base_dir(pol)?;
            let pins_s =
                payload_gc_pins(payload).unwrap_or_else(|| ".genesis/pins.toml".to_string());
            let target = payload_gc_target(payload)?;

            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let pins_path = sandbox_path_write(&base_dir, &pins_s, create_dirs)?;
            let _pins_lock = gc_path_lock(&pins_path)?;
            let document = gc_pins_document_at(&pins_path).map_err(EffectsError::Log)?;
            let plan = authority.update_pins(":unpin", &target, document)?;
            atomic_write_text(&pins_path, &plan.body)?;

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":pins")), Term::Str(pins_s));
            m.insert(
                TermOrdKey(Term::symbol(":keep")),
                Term::Vector(plan.keep.into_iter().map(Term::Str).collect()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":keep-refs")),
                Term::Vector(plan.keep_refs.into_iter().map(Term::Str).collect()),
            );
            Ok(Value::data(Term::Map(m)))
        }
        "core/gc-low::purge" => {
            let authority = gc_authority.as_deref_mut().ok_or_else(|| {
                EffectsError::Log(
                    "core/gc-low::purge requires the artifact-loaded GenesisCode GC authority"
                        .to_string(),
                )
            })?;
            let base_dir = effective_base_dir(pol)?;
            let ttl_days = payload_gc_ttl_days(payload)
                .ok_or_else(|| EffectsError::BadPayload("missing :ttl-days int".to_string()))?;
            let quarantine_dir_s = payload_gc_quarantine_dir(payload);

            let qd = match quarantine_dir_s {
                Some(s) => sandbox_path_allow_missing(&base_dir, &s, false)?,
                None => {
                    let store = store.ok_or_else(|| {
                        EffectsError::Log(
                            "missing artifact store for core/gc-low::purge".to_string(),
                        )
                    })?;
                    store
                        .root_dir()
                        .parent()
                        .unwrap_or(store.root_dir())
                        .join("quarantine")
                }
            };
            let now = std::time::SystemTime::now();
            let _quarantine_lock = if qd.exists() {
                Some(gc_path_lock(&qd.join(".gc-purge"))?)
            } else {
                None
            };
            let inventory = gc_quarantine_inventory(&qd, now)?;
            let purge = authority.purge_plan(ttl_days.saturating_mul(86_400), inventory)?;
            let quarantine_store = if purge.is_empty() {
                None
            } else {
                Some(ArtifactStore::open_with_integrity_cache(&qd, false)?)
            };
            let mut purged: u64 = 0;
            for hash in purge {
                let path = qd.join(&hash);
                let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
                    EffectsError::Log(format!(
                        "authorized purge object changed before mutation ({hash}): {error}"
                    ))
                })?;
                if !metadata.file_type().is_file() {
                    return Err(EffectsError::Log(format!(
                        "authorized purge object is no longer a regular file: {hash}"
                    )));
                }
                quarantine_store
                    .as_ref()
                    .ok_or_else(|| {
                        EffectsError::Log("purge authority returned an inconsistent plan".to_string())
                    })?
                    .verify_hex(&hash)
                    .map_err(|error| {
                        EffectsError::Log(format!(
                            "authorized purge object identity changed before mutation ({hash}): {error}"
                        ))
                    })?;
                std::fs::remove_file(path)?;
                purged = purged.saturating_add(1);
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(
                TermOrdKey(Term::symbol(":purged")),
                Term::Int((purged as i64).into()),
            );
            Ok(Value::data(Term::Map(m)))
        }
        "core/gpk-low::export" => {
            let mut ctx = gpk_ops::GpkDispatchCtx {
                pol,
                policy,
                store,
                refs,
                refs_authority: refs_authority.as_deref_mut(),
                budget,
                error_tok,
                op,
            };
            gpk_ops::handle_gpk_export(payload, &mut ctx)
        }
        "core/gpk-low::import" => {
            let mut ctx = gpk_ops::GpkDispatchCtx {
                pol,
                policy,
                store,
                refs,
                refs_authority: refs_authority.as_deref_mut(),
                budget,
                error_tok,
                op,
            };
            gpk_ops::handle_gpk_import(payload, &mut ctx)
        }
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op",
            format!("unknown capability op: {op}"),
            Some(op),
        )),
    }
}
