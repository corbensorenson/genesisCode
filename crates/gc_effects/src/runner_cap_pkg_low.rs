use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "host capability dispatch wiring keeps explicit context parameters visible"
)]
pub(super) fn capability_pkg_low(
    op_eff: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
    op: &str,
    _timeout_ms: Option<u64>,
) -> Result<Value, EffectsError> {
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
        "core/pkg-low::load-package" => {
            let pkg_path_s = match payload_pkg_path(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let base_dir = effective_base_dir(pol)?;
            let pkg_path = sandbox_path_read(&base_dir, &pkg_path_s)?;

            let (manifest, pkg_dir) = match gc_pkg::PackageManifest::load(&pkg_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/manifest-error",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            let base = std::fs::canonicalize(&base_dir)?;
            let pkg_dir_resolved = std::fs::canonicalize(&pkg_dir)?;
            if !pkg_dir_resolved.starts_with(&base) {
                return Ok(mk_error(
                    error_tok,
                    "core/caps/path-escape",
                    "package directory escapes base dir".to_string(),
                    Some(op),
                ));
            }

            let mut modules_out: Vec<Term> = Vec::new();
            for me in &manifest.modules {
                let module_fs_path = pkg_dir.join(&me.path);
                let resolved = match std::fs::canonicalize(&module_fs_path) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/pkg/io-error",
                            e.to_string(),
                            Some(op),
                            Term::Map(
                                [(
                                    TermOrdKey(Term::symbol(":path")),
                                    Term::Str(me.path.clone()),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                };
                if !resolved.starts_with(&base) {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("module escapes base dir: {}", me.path),
                        Some(op),
                    ));
                }
                let src = match std::fs::read_to_string(&resolved) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/pkg/io-error",
                            e.to_string(),
                            Some(op),
                            Term::Map(
                                [(
                                    TermOrdKey(Term::symbol(":path")),
                                    Term::Str(me.path.clone()),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                };
                let forms = match gc_coreform::parse_module(&src) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/parse-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let forms = match gc_coreform::canonicalize_module(forms) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/canon-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let module_h = gc_coreform::hash_module(&forms);
                if let Some(want_hex) = &me.hash {
                    if want_hex.len() != 64 || !want_hex.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-hash",
                            format!("manifest module hash is not 64-hex: {}", me.path),
                            Some(op),
                        ));
                    }
                    let got_hex = blake3::Hash::from_bytes(module_h).to_hex().to_string();
                    if &got_hex != want_hex {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/hash-mismatch",
                            format!("module hash mismatch: {}", me.path),
                            Some(op),
                        ));
                    }
                }

                let mut mm = BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::symbol(":path")),
                    Term::Str(me.path.clone()),
                );
                mm.insert(TermOrdKey(Term::symbol(":module")), Term::Vector(forms));
                mm.insert(
                    TermOrdKey(Term::symbol(":module-h")),
                    Term::Bytes(module_h.to_vec().into()),
                );
                modules_out.push(Term::Map(mm));
            }

            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":pkg")), Term::Str(pkg_path_s));
            out.insert(
                TermOrdKey(Term::symbol(":name")),
                Term::Str(manifest.name.clone()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":version")),
                Term::Str(manifest.version.clone()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(
                    manifest
                        .obligations
                        .iter()
                        .cloned()
                        .map(Term::Symbol)
                        .collect(),
                ),
            );
            out.insert(
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(modules_out),
            );
            Ok(Value::Data(Term::Map(out)))
        }
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
                        Some(Term::Symbol(s)) => {
                            s.trim_start_matches(':').parse::<gc_pkg::ResolutionStrategy>().ok()
                        }
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

        "core/pkg-low::info" => {
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let name = match payload_pkg_name(payload) {
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

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            m.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
            m.insert(
                TermOrdKey(Term::symbol(":requirement")),
                l.requirements
                    .get(&name)
                    .map(|r| {
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":selector")),
                                    Term::Str(r.selector.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":update-policy")),
                                    Term::Symbol(match r.update_policy {
                                        gc_pkg::UpdatePolicy::Manual => ":manual".to_string(),
                                        gc_pkg::UpdatePolicy::Auto => ":auto".to_string(),
                                    }),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":registry")),
                                    r.registry.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":strategy")),
                                    Term::Symbol(format!(":{}", r.strategy.as_str())),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":tag-policy")),
                                    r.tag_policy.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .unwrap_or(Term::Nil),
            );
            m.insert(
                TermOrdKey(Term::symbol(":locked")),
                l.locked
                    .get(&name)
                    .map(|le| {
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":commit")),
                                    le.commit.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":snapshot")),
                                    Term::Str(le.snapshot.clone()),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":resolved-ref")),
                                    le.resolved_ref.clone().map(Term::Str).unwrap_or(Term::Nil),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":environment-fingerprint")),
                                    le.environment_fingerprint
                                        .clone()
                                        .map(Term::Str)
                                        .unwrap_or(Term::Nil),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        )
                    })
                    .unwrap_or(Term::Nil),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::lock" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::lock".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg-low::lock".to_string())
            })?;
            let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);
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

            let mut out_locked: BTreeMap<String, gc_pkg::LockedEntry> = BTreeMap::new();
            for (name, req) in &l.requirements {
                match resolve_requirement(store, refs, name, req, error_tok, op) {
                    Ok(le) => {
                        out_locked.insert(name.clone(), le);
                    }
                    Err(err_val) => return Ok(err_val),
                }
            }

            if strict
                && let Err(v) = validate_locked_entries_strict(
                    store,
                    &l.requirements,
                    &out_locked,
                    true,
                    error_tok,
                    op,
                )
            {
                return Ok(v);
            }
            l.locked = out_locked;
            let workspace_root = match persist_workspace_root_snapshot(store, &l, error_tok, op) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };
            l.artifacts.insert(
                "root_workspace_snapshot".to_string(),
                workspace_root.clone(),
            );
            let deps_provenance =
                match locked_dependency_provenance(store, &l.locked, strict, error_tok, op) {
                    Ok(v) => v,
                    Err(v) => return Ok(v),
                };

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
            m.insert(
                TermOrdKey(Term::symbol(":locked-count")),
                Term::Int((l.locked.len() as i64).into()),
            );
            m.insert(TermOrdKey(Term::symbol(":strict")), Term::Bool(strict));
            m.insert(
                TermOrdKey(Term::symbol(":workspace-root")),
                Term::Str(workspace_root.clone()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":workspace-root")),
                            Term::Str(workspace_root),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock-h")),
                            Term::Str(lock_h.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":deps")),
                            Term::Vector(deps_provenance),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::update" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::update".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg-low::update".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);
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

            let mut updated: u64 = 0;
            for (name, req) in &l.requirements {
                let should_update = req.update_policy == gc_pkg::UpdatePolicy::Auto
                    && matches!(
                        req.strategy,
                        gc_pkg::ResolutionStrategy::TrackRef
                            | gc_pkg::ResolutionStrategy::TagPolicy
                    );
                if !should_update && l.locked.contains_key(name) {
                    continue;
                }
                match resolve_requirement(store, refs, name, req, error_tok, op) {
                    Ok(le) => {
                        l.locked.insert(name.clone(), le);
                        updated = updated.saturating_add(1);
                    }
                    Err(err_val) => return Ok(err_val),
                }
            }
            if strict
                && let Err(v) = validate_locked_entries_strict(
                    store,
                    &l.requirements,
                    &l.locked,
                    true,
                    error_tok,
                    op,
                )
            {
                return Ok(v);
            }
            let workspace_root = match persist_workspace_root_snapshot(store, &l, error_tok, op) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };
            l.artifacts.insert(
                "root_workspace_snapshot".to_string(),
                workspace_root.clone(),
            );
            let deps_provenance =
                match locked_dependency_provenance(store, &l.locked, strict, error_tok, op) {
                    Ok(v) => v,
                    Err(v) => return Ok(v),
                };

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
            m.insert(
                TermOrdKey(Term::symbol(":updated")),
                Term::Int((updated as i64).into()),
            );
            m.insert(TermOrdKey(Term::symbol(":strict")), Term::Bool(strict));
            m.insert(
                TermOrdKey(Term::symbol(":workspace-root")),
                Term::Str(workspace_root.clone()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":workspace-root")),
                            Term::Str(workspace_root),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock-h")),
                            Term::Str(lock_h.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":deps")),
                            Term::Vector(deps_provenance),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::install" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::install".to_string())
            })?;
            let lock_s = match payload_pkg_lock(payload) {
                Ok(s) => s,
                Err(e) => return Ok(mk_error(error_tok, "core/pkg/bad-payload", e, Some(op))),
            };
            let frozen = payload_pkg_bool(payload, ":frozen").unwrap_or(false);
            let strict = payload_pkg_bool(payload, ":strict").unwrap_or(false);

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
            if frozen {
                let missing = l.requirements_missing_locks();
                if !missing.is_empty() {
                    return Ok(mk_error_with_ctx(
                        error_tok,
                        "core/pkg/not-locked",
                        "lock is missing locked entries".to_string(),
                        Some(op),
                        Term::Map(
                            [(
                                TermOrdKey(Term::symbol(":missing")),
                                Term::Vector(missing.into_iter().map(Term::Str).collect()),
                            )]
                            .into_iter()
                            .collect(),
                        ),
                    ));
                }
            }

            let mut ok = true;
            let mut missing_hashes: Vec<Term> = Vec::new();
            let mut checked: u64 = 0;
            let deps_provenance =
                match locked_dependency_provenance(store, &l.locked, false, error_tok, op) {
                    Ok(v) => v,
                    Err(v) => return Ok(v),
                };
            let workspace_root = l.artifacts.get("root_workspace_snapshot").cloned();

            for (name, le) in &l.locked {
                // Snapshot must exist and be well-formed.
                let snapshot_hex = &le.snapshot;
                if !store.path_for(snapshot_hex).exists() {
                    ok = false;
                    missing_hashes.push(Term::Str(snapshot_hex.clone()));
                    continue;
                }
                if store.verify_hex(snapshot_hex).is_err() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {snapshot_hex}"),
                        Some(op),
                    ));
                }
                let snap_term = match store_get_term(store, snapshot_hex) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let snap = match gc_vcs::Snapshot::from_term(&snap_term) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };

                // Shallow closure: snapshot + module artifacts.
                let mut hashes: Vec<String> = Vec::new();
                hashes.push(snapshot_hex.clone());
                hashes.extend(snap.shallow_refs());
                hashes.sort();
                hashes.dedup();

                for h in hashes {
                    if !store.path_for(&h).exists() {
                        ok = false;
                        missing_hashes.push(Term::Str(h));
                        continue;
                    }
                    if store.verify_hex(&h).is_err() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("artifact store corruption: {h}"),
                            Some(op),
                        ));
                    }
                    checked = checked.saturating_add(1);
                }

                if strict && let Some(commit_hex) = &le.commit {
                    match validate_commit_artifact_closure(
                        store,
                        name,
                        snapshot_hex,
                        commit_hex,
                        true,
                        error_tok,
                        op,
                    ) {
                        Ok(n) => checked = checked.saturating_add(n),
                        Err(v) => return Ok(v),
                    }
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":checked")),
                Term::Int((checked as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":missing")),
                Term::Vector(missing_hashes),
            );
            m.insert(
                TermOrdKey(Term::symbol(":workspace-root")),
                workspace_root.clone().map(Term::Str).unwrap_or(Term::Nil),
            );
            m.insert(
                TermOrdKey(Term::symbol(":provenance")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":workspace-root")),
                            workspace_root.map(Term::Str).unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":deps")),
                            Term::Vector(deps_provenance),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::verify" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::verify".to_string())
            })?;
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

            let mut ok = true;
            let mut missing_hashes: Vec<Term> = Vec::new();
            let mut checked: u64 = 0;

            for (name, le) in &l.locked {
                let snapshot_hex = &le.snapshot;
                if !store.path_for(snapshot_hex).exists() {
                    ok = false;
                    missing_hashes.push(Term::Str(snapshot_hex.clone()));
                    continue;
                }
                if store.verify_hex(snapshot_hex).is_err() {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/corruption",
                        format!("artifact store corruption: {snapshot_hex}"),
                        Some(op),
                    ));
                }
                let snap_term = match store_get_term(store, snapshot_hex) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let snap = match gc_vcs::Snapshot::from_term(&snap_term) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-snapshot",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let mut hashes: Vec<String> = Vec::new();
                hashes.push(snapshot_hex.clone());
                hashes.extend(snap.shallow_refs());
                hashes.sort();
                hashes.dedup();
                for h in hashes {
                    if !store.path_for(&h).exists() {
                        ok = false;
                        missing_hashes.push(Term::Str(h));
                        continue;
                    }
                    if store.verify_hex(&h).is_err() {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/corruption",
                            format!("artifact store corruption: {h}"),
                            Some(op),
                        ));
                    }
                    checked = checked.saturating_add(1);
                }

                if let Some(commit_hex) = &le.commit {
                    match validate_commit_artifact_closure(
                        store,
                        name,
                        snapshot_hex,
                        commit_hex,
                        true,
                        error_tok,
                        op,
                    ) {
                        Ok(n) => checked = checked.saturating_add(n),
                        Err(v) => return Ok(v),
                    }
                }
            }

            let mut m = BTreeMap::new();
            m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
            m.insert(TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s));
            m.insert(
                TermOrdKey(Term::symbol(":checked")),
                Term::Int((checked as i64).into()),
            );
            m.insert(
                TermOrdKey(Term::symbol(":missing")),
                Term::Vector(missing_hashes),
            );
            Ok(Value::Data(Term::Map(m)))
        }

        "core/pkg-low::snapshot" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::snapshot".to_string())
            })?;
            let pkg_path_s = match payload_pkg_path(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };
            let base_dir = effective_base_dir(pol)?;
            let pkg_path = sandbox_path_read(&base_dir, &pkg_path_s)?;

            let (manifest, pkg_dir) = match gc_pkg::PackageManifest::load(&pkg_path) {
                Ok(x) => x,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/manifest-error",
                        format!("{e}"),
                        Some(op),
                    ));
                }
            };

            // Ensure the package directory is within the sandbox base.
            let base = std::fs::canonicalize(&base_dir)?;
            let pkg_dir_resolved = std::fs::canonicalize(&pkg_dir)?;
            if !pkg_dir_resolved.starts_with(&base) {
                return Ok(mk_error(
                    error_tok,
                    "core/caps/path-escape",
                    "package directory escapes base dir".to_string(),
                    Some(op),
                ));
            }

            let mut modules_out: Vec<Term> = Vec::new();
            for me in &manifest.modules {
                let module_fs_path = pkg_dir.join(&me.path);
                let resolved = match std::fs::canonicalize(&module_fs_path) {
                    Ok(p) => p,
                    Err(e) => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/pkg/io-error",
                            e.to_string(),
                            Some(op),
                            Term::Map(
                                [(
                                    TermOrdKey(Term::symbol(":path")),
                                    Term::Str(me.path.clone()),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                };
                if !resolved.starts_with(&base) {
                    return Ok(mk_error(
                        error_tok,
                        "core/caps/path-escape",
                        format!("module escapes base dir: {}", me.path),
                        Some(op),
                    ));
                }
                let src = match std::fs::read_to_string(&resolved) {
                    Ok(s) => s,
                    Err(e) => {
                        return Ok(mk_error_with_ctx(
                            error_tok,
                            "core/pkg/io-error",
                            e.to_string(),
                            Some(op),
                            Term::Map(
                                [(
                                    TermOrdKey(Term::symbol(":path")),
                                    Term::Str(me.path.clone()),
                                )]
                                .into_iter()
                                .collect(),
                            ),
                        ));
                    }
                };
                let forms = match gc_coreform::parse_module(&src) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/parse-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let forms = match gc_coreform::canonicalize_module(forms) {
                    Ok(f) => f,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/canon-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let module_h = gc_coreform::hash_module(&forms);
                if let Some(want_hex) = &me.hash {
                    if want_hex.len() != 64 || !want_hex.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-hash",
                            format!("manifest module hash is not 64-hex: {}", me.path),
                            Some(op),
                        ));
                    }
                    let got_hex = blake3::Hash::from_bytes(module_h).to_hex().to_string();
                    if &got_hex != want_hex {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/hash-mismatch",
                            format!("module hash mismatch: {}", me.path),
                            Some(op),
                        ));
                    }
                }

                let module_art = Term::Vector(forms);
                let module_bytes = print_term(&module_art);
                let store_hex = match store_put_with_budget(
                    store,
                    module_bytes.as_bytes(),
                    policy,
                    budget,
                    error_tok,
                    op,
                ) {
                    Ok(h) => h,
                    Err(v) => return Ok(v),
                };
                let mut mm = BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::Symbol(":path".to_string())),
                    Term::Str(me.path.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::Symbol(":hash".to_string())),
                    Term::Str(store_hex),
                );
                mm.insert(
                    TermOrdKey(Term::Symbol(":module-h".to_string())),
                    Term::Bytes(module_h.to_vec().into()),
                );
                modules_out.push(Term::Map(mm));
            }

            let snapshot = Term::Map(
                [
                    (
                        TermOrdKey(Term::Symbol(":type".to_string())),
                        Term::Symbol(":vcs/snapshot".to_string()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":v".to_string())),
                        Term::Int(1.into()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":kind".to_string())),
                        Term::Symbol(":package".to_string()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":pkg/name".to_string())),
                        Term::Str(manifest.name.clone()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":pkg/version".to_string())),
                        Term::Str(manifest.version.clone()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":modules".to_string())),
                        Term::Vector(modules_out.clone()),
                    ),
                    (
                        TermOrdKey(Term::Symbol(":obligations".to_string())),
                        Term::Vector(
                            manifest
                                .obligations
                                .iter()
                                .cloned()
                                .map(Term::Symbol)
                                .collect(),
                        ),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let snapshot_bytes = print_term(&snapshot);
            let snap_hex = match store_put_with_budget(
                store,
                snapshot_bytes.as_bytes(),
                policy,
                budget,
                error_tok,
                op,
            ) {
                Ok(h) => h,
                Err(v) => return Ok(v),
            };

            let mut out = BTreeMap::new();
            out.insert(
                TermOrdKey(Term::Symbol(":snapshot".to_string())),
                Term::Str(snap_hex),
            );
            out.insert(
                TermOrdKey(Term::Symbol(":modules".to_string())),
                Term::Vector(modules_out),
            );
            Ok(Value::Data(Term::Map(out)))
        }

        "core/pkg-low::publish" => {
            let store = store.ok_or_else(|| {
                EffectsError::Log("missing artifact store for core/pkg-low::publish".to_string())
            })?;
            let refs = refs.ok_or_else(|| {
                EffectsError::Log("missing refs db for core/pkg-low::publish".to_string())
            })?;

            let remote_s = match payload_pkg_publish_remote(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let refname = match payload_pkg_publish_ref(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let policy_h = match payload_pkg_publish_policy(payload) {
                Ok(s) => s,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let expected_old = match payload_pkg_publish_expected_old(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            let depth = payload_pkg_publish_depth(payload).unwrap_or(0);
            let commit_override = match payload_pkg_publish_commit(payload) {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-payload",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            let commit_hex = if let Some(h) = commit_override {
                h
            } else {
                let h = match refs.get(&refname) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/refs/io-error",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let Some(h) = h else {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/ref-not-found",
                        format!("local ref is unset: {refname}"),
                        Some(op),
                    ));
                };
                h
            };

            if gc_vcs::validate_hex_hash(&commit_hex).is_err() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-payload",
                    "commit must be 64-hex".to_string(),
                    Some(op),
                ));
            }
            if gc_vcs::validate_hex_hash(&policy_h).is_err() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/bad-payload",
                    "policy must be 64-hex".to_string(),
                    Some(op),
                ));
            }

            let pol_term = match store_get_term(store, &policy_h) {
                Ok(t) => t,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {policy_h}"),
                        Some(op),
                    ));
                }
            };
            let pol_art = match gc_vcs::Policy::from_term(&pol_term) {
                Ok(p) => p,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-policy",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };
            if pol_art.is_frozen_ref(&refname) {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/ref-frozen",
                    format!("ref is frozen by policy: {refname}"),
                    Some(op),
                ));
            }
            let class = match pol_art.class_for_ref(&refname) {
                Some(c) => c,
                None => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/no-policy-class",
                        format!("policy has no matching class for ref: {refname}"),
                        Some(op),
                    ));
                }
            };

            let commit_term = match store_get_term(store, &commit_hex) {
                Ok(t) => t,
                Err(_) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/store/not-found",
                        format!("artifact not found: {commit_hex}"),
                        Some(op),
                    ));
                }
            };
            let commit = match gc_vcs::Commit::from_term(&commit_term) {
                Ok(c) => c,
                Err(e) => {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/bad-commit",
                        e.to_string(),
                        Some(op),
                    ));
                }
            };

            for req in &class.required_obligations {
                if !commit.obligations.iter().any(|o| o == req) {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-obligation",
                        format!("commit missing required obligation: {req}"),
                        Some(op),
                    ));
                }
            }
            if !class.required_obligations.is_empty() && commit.evidence.is_empty() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/missing-evidence",
                    "commit has required obligations but no evidence".to_string(),
                    Some(op),
                ));
            }
            let mut evidence_kinds: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for ev_h in &commit.evidence {
                let ev_term = match store_get_term(store, ev_h) {
                    Ok(t) => t,
                    Err(_) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/store/not-found",
                            format!("artifact not found: {ev_h}"),
                            Some(op),
                        ));
                    }
                };
                let ev = match gc_vcs::Evidence::from_term(&ev_term) {
                    Ok(ev) => ev,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-evidence",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                evidence_kinds.insert(gc_vcs::PolicyClass::normalize_evidence_kind(&ev.kind));
            }
            let missing_kinds =
                class.missing_required_evidence_kinds(&commit.obligations, &evidence_kinds);
            if !missing_kinds.is_empty() {
                return Ok(mk_error(
                    error_tok,
                    "core/pkg/missing-evidence-kind",
                    format!(
                        "commit evidence missing required kinds: {}",
                        missing_kinds.join(", ")
                    ),
                    Some(op),
                ));
            }

            if class.require_signatures {
                let signing_h = match gc_vcs::commit_signing_hash(&commit_term) {
                    Ok(h) => h,
                    Err(e) => {
                        return Ok(mk_error(
                            error_tok,
                            "core/pkg/bad-commit",
                            e.to_string(),
                            Some(op),
                        ));
                    }
                };
                let mut valid: u64 = 0;
                let mut seen_pks: std::collections::BTreeSet<Vec<u8>> =
                    std::collections::BTreeSet::new();
                for at_h in &commit.attestations {
                    let at_term = match store_get_term(store, at_h) {
                        Ok(t) => t,
                        Err(_) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/store/not-found",
                                format!("artifact not found: {at_h}"),
                                Some(op),
                            ));
                        }
                    };
                    let at = match gc_vcs::Attestation::from_term(&at_term) {
                        Ok(a) => a,
                        Err(e) => {
                            return Ok(mk_error(
                                error_tok,
                                "core/pkg/bad-attestation",
                                e.to_string(),
                                Some(op),
                            ));
                        }
                    };
                    let pk_vec = at.pk.to_vec();
                    if seen_pks.contains(&pk_vec) {
                        continue;
                    }
                    if gc_vcs::verify_commit_attestation(
                        &at,
                        &signing_h,
                        &class.allowed_public_keys,
                    )
                    .is_ok()
                    {
                        seen_pks.insert(pk_vec);
                        valid = valid.saturating_add(1);
                    }
                }
                if valid < class.min_signatures {
                    return Ok(mk_error(
                        error_tok,
                        "core/pkg/missing-signatures",
                        format!(
                            "need {} valid signatures, got {valid}",
                            class.min_signatures
                        ),
                        Some(op),
                    ));
                }
            }

            let mut set_ref_mm = BTreeMap::new();
            set_ref_mm.insert(
                TermOrdKey(Term::symbol(":name")),
                Term::Str(refname.clone()),
            );
            set_ref_mm.insert(
                TermOrdKey(Term::symbol(":hash")),
                Term::Str(commit_hex.clone()),
            );
            set_ref_mm.insert(
                TermOrdKey(Term::symbol(":policy")),
                Term::Str(policy_h.clone()),
            );
            if let Some(eo) = &expected_old {
                set_ref_mm.insert(
                    TermOrdKey(Term::symbol(":expected-old")),
                    Term::Str(eo.clone()),
                );
            }

            let mut sync_payload_m = BTreeMap::new();
            sync_payload_m.insert(TermOrdKey(Term::symbol(":remote")), Term::Str(remote_s));
            sync_payload_m.insert(
                TermOrdKey(Term::symbol(":roots")),
                Term::Vector(vec![
                    Term::Str(commit_hex.clone()),
                    Term::Str(policy_h.clone()),
                ]),
            );
            if depth > 0 {
                sync_payload_m.insert(
                    TermOrdKey(Term::symbol(":depth")),
                    Term::Int((depth as i64).into()),
                );
            }
            sync_payload_m.insert(
                TermOrdKey(Term::symbol(":set-refs")),
                Term::Vector(vec![Term::Map(set_ref_mm)]),
            );

            let sync_payload = Term::Map(sync_payload_m);
            let sync_pol = pol.or_else(|| policy.op_policy("core/sync::push"));
            let sync_out = call_capability(
                "core/sync::push",
                &sync_payload,
                sync_pol,
                policy,
                Some(store),
                Some(refs),
                budget,
                error_tok,
            )?;

            let out = match sync_out {
                Value::Data(Term::Map(mut m)) => {
                    m.insert(TermOrdKey(Term::symbol(":commit")), Term::Str(commit_hex));
                    m.insert(TermOrdKey(Term::symbol(":ref")), Term::Str(refname));
                    m.insert(
                        TermOrdKey(Term::symbol(":provenance")),
                        commit_provenance_term(&commit),
                    );
                    Value::Data(Term::Map(m))
                }
                other => other,
            };
            Ok(out)
        }
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op",
            format!("unknown capability op: {op}"),
            Some(op),
        )),
    }
}
