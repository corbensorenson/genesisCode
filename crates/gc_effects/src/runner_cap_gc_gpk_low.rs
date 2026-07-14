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
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
    timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
    let _ = timeout_ms;
    match op_eff {
        "core/gc-low::plan" => {
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

            let (refs_entries, lock_info, pins_info) = match gc_build_sources(
                refs,
                &base_dir,
                &lock_s,
                &pins_s,
                include_lock,
                include_refs,
                error_tok,
                op,
            ) {
                Ok(v) => v,
                Err(v) => return Ok(v),
            };
            let (mut roots, roots_kind) = match gc_roots_plan_from_sources(
                &refs_entries,
                &lock_info,
                &pins_info,
                include_lock,
                include_refs,
                error_tok,
                op,
            ) {
                Ok(v) => v,
                Err(v) => return Ok(v),
            };
            roots.sort();
            roots.dedup();

            let mut live: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for h in &roots {
                match sync_closure_local(store, h, depth, &mut live, error_tok, op) {
                    Ok(()) => {}
                    Err(v) => return Ok(v),
                }
            }

            let store_dir = store.root_dir();
            let _lk = gc_store_lock(store_dir)?;
            let (dead, dead_bytes, largest) = gc_store_dead_set(store_dir, &live)?;

            let largest_term: Vec<Term> = largest
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

            let dead_sample: Vec<Term> = dead.iter().take(50).cloned().map(Term::Str).collect();

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(
                TermOrdKey(Term::symbol(":live")),
                Term::Int((live.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":dead")),
                Term::Int((dead.len() as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":reclaim-bytes")),
                Term::Int((dead_bytes as i64).into()),
            );
            m.insert(TermOrdKey(Term::symbol(":roots")), Term::Vector(roots_kind));
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

            let (refs_entries, lock_info, pins_info) = match gc_build_sources(
                refs,
                &base_dir,
                &lock_s,
                &pins_s,
                include_lock,
                include_refs,
                error_tok,
                op,
            ) {
                Ok(v) => v,
                Err(v) => return Ok(v),
            };
            let (mut roots, _) = match gc_roots_plan_from_sources(
                &refs_entries,
                &lock_info,
                &pins_info,
                include_lock,
                include_refs,
                error_tok,
                op,
            ) {
                Ok(v) => v,
                Err(v) => return Ok(v),
            };
            roots.sort();
            roots.dedup();

            let mut live: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            for h in &roots {
                match sync_closure_local(store, h, depth, &mut live, error_tok, op) {
                    Ok(()) => {}
                    Err(v) => return Ok(v),
                }
            }

            let store_dir = store.root_dir();
            let _lk = gc_store_lock(store_dir)?;
            let (dead, dead_bytes, _largest) = gc_store_dead_set(store_dir, &live)?;

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
            for h in &dead {
                let p = store_dir.join(h);
                if !p.exists() {
                    continue;
                }
                if let Some(qd) = &quarantine_dir {
                    let qp = qd.join(h);
                    if qp.exists() {
                        continue;
                    }
                    std::fs::rename(&p, &qp)?;
                    quarantined = quarantined.saturating_add(1);
                } else {
                    std::fs::remove_file(&p)?;
                    deleted = deleted.saturating_add(1);
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(
                TermOrdKey(Term::symbol(":dead")),
                Term::Int((dead.len() as i64).into()),
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
                Term::Int((dead_bytes as i64).into()),
            );
            Ok(Value::data(Term::Map(m)))
        }
        "core/gc-low::pin" => {
            let base_dir = effective_base_dir(pol)?;
            let pins_s =
                payload_gc_pins(payload).unwrap_or_else(|| ".genesis/pins.toml".to_string());
            let target = payload_gc_target(payload)?;

            let mut pins = gc_pins_load(&base_dir, &pins_s).unwrap_or_else(|_| GcPins::empty());
            if target.starts_with("refs/") {
                if !pins.keep_refs.iter().any(|r| r == &target) {
                    pins.keep_refs.push(target);
                }
            } else {
                let h = gc_normalize_hash(&target).ok_or_else(|| {
                    EffectsError::BadPayload("pin target must be hex hash or refs/...".to_string())
                })?;
                if !pins.keep.iter().any(|x| x == &h) {
                    pins.keep.push(h);
                }
            }
            pins.keep.sort();
            pins.keep.dedup();
            pins.keep_refs.sort();
            pins.keep_refs.dedup();

            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let pins_path = sandbox_path_write(&base_dir, &pins_s, create_dirs)?;
            gc_pins_write(&pins_path, &pins)?;

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":pins")), Term::Str(pins_s));
            m.insert(
                TermOrdKey(Term::symbol(":keep")),
                Term::Vector(pins.keep.iter().cloned().map(Term::Str).collect()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":keep-refs")),
                Term::Vector(pins.keep_refs.iter().cloned().map(Term::Str).collect()),
            );
            Ok(Value::data(Term::Map(m)))
        }
        "core/gc-low::unpin" => {
            let base_dir = effective_base_dir(pol)?;
            let pins_s =
                payload_gc_pins(payload).unwrap_or_else(|| ".genesis/pins.toml".to_string());
            let target = payload_gc_target(payload)?;

            let mut pins = gc_pins_load(&base_dir, &pins_s).unwrap_or_else(|_| GcPins::empty());
            if target.starts_with("refs/") {
                pins.keep_refs.retain(|r| r != &target);
            } else if let Some(h) = gc_normalize_hash(&target) {
                pins.keep.retain(|x| x != &h);
            } else {
                return Ok(mk_error(
                    error_tok,
                    "core/gc/bad-payload",
                    "unpin target must be hex hash or refs/...".to_string(),
                    Some(op),
                ));
            }
            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let pins_path = sandbox_path_write(&base_dir, &pins_s, create_dirs)?;
            gc_pins_write(&pins_path, &pins)?;

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":pins")), Term::Str(pins_s));
            m.insert(
                TermOrdKey(Term::symbol(":keep")),
                Term::Vector(pins.keep.iter().cloned().map(Term::Str).collect()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":keep-refs")),
                Term::Vector(pins.keep_refs.iter().cloned().map(Term::Str).collect()),
            );
            Ok(Value::data(Term::Map(m)))
        }
        "core/gc-low::purge" => {
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
            if !qd.exists() {
                let mut m = BTreeMap::new();
                m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
                m.insert(TermOrdKey(Term::symbol(":purged")), Term::Int(0.into()));
                return Ok(Value::data(Term::Map(m)));
            }

            let now = std::time::SystemTime::now();
            let ttl = std::time::Duration::from_secs(ttl_days.saturating_mul(86_400));

            let mut purged: u64 = 0;
            for ent in std::fs::read_dir(&qd)? {
                let ent = ent?;
                let p = ent.path();
                let ft = ent.file_type()?;
                if !ft.is_file() {
                    continue;
                }
                let name = ent.file_name().to_string_lossy().to_string();
                if gc_vcs::validate_hex_hash(&name).is_err() {
                    continue;
                }
                let meta = ent.metadata()?;
                if let Ok(mtime) = meta.modified()
                    && let Ok(age) = now.duration_since(mtime)
                    && age >= ttl
                {
                    let _ = std::fs::remove_file(&p);
                    purged = purged.saturating_add(1);
                }
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
