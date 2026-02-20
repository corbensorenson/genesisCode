use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "capability dispatch signatures are explicit by design"
)]
pub(super) fn dispatch_lock_io(
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
    let _ = (policy, store, refs, budget, timeout_ms);
    match op_eff {
        "core/pkg-low::init" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let workspace = match payload_pkg_workspace(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let policy_s =
                payload_pkg_policy(payload).unwrap_or_else(|| "policy:default-v0.1".to_string());
            let reg_default = payload_pkg_registry_default(payload);

            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.map(|p| p.create_dirs).unwrap_or(false);
            let lock_path = match sandbox_path_write(&base_dir, &lock_s, create_dirs) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut l = gc_pkg::GenesisLock::empty(workspace);
            l.policy = policy_s;
            if let Some(rd) = reg_default {
                l.registries.insert("default".to_string(), rd);
            }
            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            if let Err(e) = atomic_write_text(&lock_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":lock-h")),
                Term::Str(lock_h.clone()),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::add" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let name = match payload_pkg_name(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let selector = match payload_pkg_selector(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let update_policy = match payload_pkg_update_policy(payload) {
                Ok(Some(p)) => p,
                Ok(None) => gc_pkg::UpdatePolicy::Manual,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let strategy = match payload_pkg_strategy(payload) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let tag_policy = match payload_pkg_tag_policy(payload) {
                Ok(x) => x,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let registry = payload_pkg_registry(payload);

            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let mut l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            l.set_requirement_with_metadata(
                &name,
                &selector,
                update_policy,
                registry,
                strategy,
                tag_policy,
            );
            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let lock_write_path = match sandbox_path_write(&base_dir, &lock_s, false) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if let Err(e) = atomic_write_text(&lock_write_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":lock-h")),
                Term::Str(lock_h.clone()),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::list" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut reqs = Vec::new();
            for (name, r) in &l.requirements {
                let mut mm = BTreeMap::new();
                mm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
                mm.insert(
                    TermOrdKey(Term::symbol(":selector")),
                    Term::Str(r.selector.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":update-policy")),
                    Term::Symbol(match r.update_policy {
                        gc_pkg::UpdatePolicy::Manual => ":manual".to_string(),
                        gc_pkg::UpdatePolicy::Auto => ":auto".to_string(),
                    }),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":registry")),
                    r.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":strategy")),
                    Term::Symbol(format!(":{}", r.strategy.as_str())),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":tag-policy")),
                    r.tag_policy.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                reqs.push(Term::Map(mm));
            }

            let mut locks = Vec::new();
            for (name, le) in &l.locked {
                let mut mm = BTreeMap::new();
                mm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
                mm.insert(
                    TermOrdKey(Term::symbol(":commit")),
                    le.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":snapshot")),
                    Term::Str(le.snapshot.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":environment-fingerprint")),
                    le.environment_fingerprint
                        .clone()
                        .map(Term::Str)
                        .unwrap_or(Term::Nil),
                );
                locks.push(Term::Map(mm));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":requirements")),
                Term::Vector(reqs),
            );
            m.insert(TermOrdKey(Term::symbol(":locked")), Term::Vector(locks));
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::load-lock" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_read(&base_dir, &lock_s) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let l = match gc_pkg::GenesisLock::load(&lock_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-lock",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let mut reqs: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
            for (name, r) in &l.requirements {
                let mut mm = BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::symbol(":selector")),
                    Term::Str(r.selector.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":update-policy")),
                    Term::Symbol(match r.update_policy {
                        gc_pkg::UpdatePolicy::Manual => ":manual".to_string(),
                        gc_pkg::UpdatePolicy::Auto => ":auto".to_string(),
                    }),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":registry")),
                    r.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                reqs.insert(TermOrdKey(Term::Str(name.clone())), Term::Map(mm));
            }

            let mut locked: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
            for (name, le) in &l.locked {
                let mut mm = BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::symbol(":commit")),
                    le.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":snapshot")),
                    Term::Str(le.snapshot.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":registry")),
                    le.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":source_selector")),
                    if le.source_selector.is_empty() {
                        Term::Nil
                    } else {
                        Term::Str(le.source_selector.clone())
                    },
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":resolved-ref")),
                    le.resolved_ref.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":exports_hash")),
                    le.exports_hash.clone().map(Term::Str).unwrap_or(Term::Nil),
                );
                locked.insert(TermOrdKey(Term::Str(name.clone())), Term::Map(mm));
            }

            let mut registries: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
            for (name, url) in &l.registries {
                registries.insert(TermOrdKey(Term::Str(name.clone())), Term::Str(url.clone()));
            }
            let mut artifacts: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
            for (name, h) in &l.artifacts {
                artifacts.insert(TermOrdKey(Term::Str(name.clone())), Term::Str(h.clone()));
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(l.workspace),
            );
            m.insert(TermOrdKey(Term::symbol(":policy")), Term::Str(l.policy));
            m.insert(TermOrdKey(Term::symbol(":requirements")), Term::Map(reqs));
            m.insert(TermOrdKey(Term::symbol(":locked")), Term::Map(locked));
            m.insert(
                TermOrdKey(Term::symbol(":registries")),
                Term::Map(registries),
            );
            m.insert(TermOrdKey(Term::symbol(":artifacts")), Term::Map(artifacts));
            Ok(Value::Data(Term::Map(m)))
        }
        "core/pkg-low::load-package" => handle_load_package(payload, pol, error_tok, op),
        "core/pkg-low::save-lock" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let Term::Map(m) = payload else {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-payload",
                    "payload must be a map".to_string(),
                    Some(op),
                ));
            };
            let workspace = match m.get(&TermOrdKey(Term::symbol(":workspace"))) {
                Some(Term::Str(s)) => s.clone(),
                _ => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        ":workspace must be string".to_string(),
                        Some(op),
                    ));
                }
            };
            let policy_s = match m.get(&TermOrdKey(Term::symbol(":policy"))) {
                Some(Term::Str(s)) => s.clone(),
                Some(Term::Nil) | None => "policy:default-v0.1".to_string(),
                _ => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        ":policy must be string or nil".to_string(),
                        Some(op),
                    ));
                }
            };
            let version = match m.get(&TermOrdKey(Term::symbol(":version"))) {
                None | Some(Term::Nil) => 2u64,
                Some(Term::Int(i)) => i.to_u64().unwrap_or(2),
                _ => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        ":version must be int or nil".to_string(),
                        Some(op),
                    ));
                }
            };
            let as_str_map =
                |v: Option<&Term>, field: &str| -> Result<BTreeMap<String, String>, Value> {
                    let mut out = BTreeMap::new();
                    let Some(term) = v else { return Ok(out) };
                    if matches!(term, Term::Nil) {
                        return Ok(out);
                    }
                    let Term::Map(mm) = term else {
                        return Err(mk_error(
                            error_tok,
                            "core/pkg/bad-payload",
                            format!("{field} must be map"),
                            Some(op),
                        ));
                    };
                    for (k, vv) in mm {
                        let key = match &k.0 {
                            Term::Str(s) => s.clone(),
                            _ => {
                                return Err(mk_error(
                                    error_tok,
                                    "core/pkg/bad-payload",
                                    format!("{field} keys must be strings"),
                                    Some(op),
                                ));
                            }
                        };
                        let val = match vv {
                            Term::Str(s) => s.clone(),
                            _ => {
                                return Err(mk_error(
                                    error_tok,
                                    "core/pkg/bad-payload",
                                    format!("{field}/{key} must be string"),
                                    Some(op),
                                ));
                            }
                        };
                        out.insert(key, val);
                    }
                    Ok(out)
                };
            let mut requirements: BTreeMap<String, gc_pkg::Requirement> = BTreeMap::new();
            if let Some(term) = m.get(&TermOrdKey(Term::symbol(":requirements")))
                && !matches!(term, Term::Nil)
            {
                let Term::Map(mm) = term else {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        ":requirements must be map".to_string(),
                        Some(op),
                    ));
                };
                for (k, vv) in mm {
                    let name = match &k.0 {
                        Term::Str(s) => s.clone(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                ":requirements keys must be strings".to_string(),
                                Some(op),
                            ));
                        }
                    };
                    let Term::Map(rm) = vv else {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-payload",
                            format!(":requirements/{name} must be map"),
                            Some(op),
                        ));
                    };
                    let selector = match rm.get(&TermOrdKey(Term::symbol(":selector"))) {
                        Some(Term::Str(s)) => s.clone(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":requirements/{name}/:selector must be string"),
                                Some(op),
                            ));
                        }
                    };
                    let update_policy = match rm.get(&TermOrdKey(Term::symbol(":update-policy"))) {
                        None | Some(Term::Nil) => gc_pkg::UpdatePolicy::Manual,
                        Some(Term::Symbol(s)) | Some(Term::Str(s))
                            if s == ":manual" || s == "manual" =>
                        {
                            gc_pkg::UpdatePolicy::Manual
                        }
                        Some(Term::Symbol(s)) | Some(Term::Str(s))
                            if s == ":auto" || s == "auto" =>
                        {
                            gc_pkg::UpdatePolicy::Auto
                        }
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(
                                    ":requirements/{name}/:update-policy must be :manual or :auto"
                                ),
                                Some(op),
                            ));
                        }
                    };
                    let registry = match rm.get(&TermOrdKey(Term::symbol(":registry"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":requirements/{name}/:registry must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let strategy = match rm.get(&TermOrdKey(Term::symbol(":strategy"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Symbol(s)) => s
                            .trim_start_matches(':')
                            .parse::<gc_pkg::ResolutionStrategy>()
                            .ok(),
                        Some(Term::Str(s)) => s.parse::<gc_pkg::ResolutionStrategy>().ok(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(
                                    ":requirements/{name}/:strategy must be :pinned, :track-ref, :tag-policy, string, or nil"
                                ),
                                Some(op),
                            ));
                        }
                    };
                    if rm.contains_key(&TermOrdKey(Term::symbol(":strategy"))) && strategy.is_none()
                    {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-payload",
                            format!(
                                ":requirements/{name}/:strategy must be :pinned, :track-ref, or :tag-policy"
                            ),
                            Some(op),
                        ));
                    }
                    let tag_policy = match rm.get(&TermOrdKey(Term::symbol(":tag-policy"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":requirements/{name}/:tag-policy must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let inferred = gc_pkg::infer_strategy(&selector);
                    requirements.insert(
                        name,
                        gc_pkg::Requirement {
                            selector,
                            update_policy,
                            registry,
                            strategy: strategy.unwrap_or(inferred),
                            tag_policy,
                        },
                    );
                }
            }

            let mut locked: BTreeMap<String, gc_pkg::LockedEntry> = BTreeMap::new();
            if let Some(term) = m.get(&TermOrdKey(Term::symbol(":locked")))
                && !matches!(term, Term::Nil)
            {
                let Term::Map(mm) = term else {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        ":locked must be map".to_string(),
                        Some(op),
                    ));
                };
                for (k, vv) in mm {
                    let name = match &k.0 {
                        Term::Str(s) => s.clone(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                ":locked keys must be strings".to_string(),
                                Some(op),
                            ));
                        }
                    };
                    let Term::Map(lm) = vv else {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-payload",
                            format!(":locked/{name} must be map"),
                            Some(op),
                        ));
                    };
                    let snapshot = match lm.get(&TermOrdKey(Term::symbol(":snapshot"))) {
                        Some(Term::Str(s)) => s.clone(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:snapshot must be string"),
                                Some(op),
                            ));
                        }
                    };
                    let commit = match lm.get(&TermOrdKey(Term::symbol(":commit"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:commit must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let registry = match lm.get(&TermOrdKey(Term::symbol(":registry"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:registry must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let source_selector = match lm
                        .get(&TermOrdKey(Term::symbol(":source_selector")))
                    {
                        None | Some(Term::Nil) => String::new(),
                        Some(Term::Str(s)) => s.clone(),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:source_selector must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let resolved_ref = match lm.get(&TermOrdKey(Term::symbol(":resolved-ref"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:resolved-ref must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let exports_hash = match lm.get(&TermOrdKey(Term::symbol(":exports_hash"))) {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(":locked/{name}/:exports_hash must be string or nil"),
                                Some(op),
                            ));
                        }
                    };
                    let environment_fingerprint = match lm
                        .get(&TermOrdKey(Term::symbol(":environment-fingerprint")))
                    {
                        None | Some(Term::Nil) => None,
                        Some(Term::Str(s)) => Some(s.clone()),
                        _ => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-payload",
                                format!(
                                    ":locked/{name}/:environment-fingerprint must be string or nil"
                                ),
                                Some(op),
                            ));
                        }
                    };
                    locked.insert(
                        name,
                        gc_pkg::LockedEntry {
                            commit,
                            snapshot,
                            registry,
                            source_selector,
                            resolved_ref,
                            exports_hash,
                            environment_fingerprint,
                        },
                    );
                }
            }
            let registries = match as_str_map(
                m.get(&TermOrdKey(Term::symbol(":registries"))),
                ":registries",
            ) {
                Ok(x) => x,
                Err(v) => return Ok(v),
            };
            let artifacts =
                match as_str_map(m.get(&TermOrdKey(Term::symbol(":artifacts"))), ":artifacts") {
                    Ok(x) => x,
                    Err(v) => return Ok(v),
                };

            let mut l = gc_pkg::GenesisLock::empty(workspace);
            l.version = version;
            l.policy = policy_s;
            l.registries = registries;
            l.requirements = requirements;
            l.locked = locked;
            l.artifacts = artifacts;

            let bytes = l.to_toml_canonical();
            let lock_h = blake3::hash(bytes.as_bytes()).to_hex().to_string();
            let base_dir = effective_base_dir(pol)?;
            let lock_path = match sandbox_path_write(
                &base_dir,
                &lock_s,
                pol.map(|p| p.create_dirs).unwrap_or(false),
            ) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            if let Err(e) = atomic_write_text(&lock_path, bytes.as_bytes()) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/io-error",
                    e.to_string(),
                    Some(op),
                ));
            }
            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            out.insert(TermOrdKey(Term::symbol(":lock-h")), Term::Str(lock_h));
            Ok(Value::Data(Term::Map(out)))
        }

        _ => unreachable!("dispatch_lock_io called with unsupported op: {op_eff}"),
    }
}
