use super::*;
#[path = "dispatch_lock_io/save_lock.rs"]
mod save_lock;

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
        "core/pkg-low::save-lock" => save_lock::dispatch_save_lock(payload, pol, error_tok, op),

        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op-eff",
            format!("core/pkg-low dispatch received unsupported op_eff: {op_eff}"),
            Some(op),
        )),
    }
}
