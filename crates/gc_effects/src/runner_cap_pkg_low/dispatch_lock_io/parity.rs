use super::*;
use crate::runner_pkg_payload::{
    payload_pkg_policy, payload_pkg_registry_default, payload_pkg_selector, payload_pkg_strategy,
    payload_pkg_tag_policy, payload_pkg_update_policy, payload_pkg_workspace,
};

pub(super) fn dispatch_lock_ops_parity(
    operation: &str,
    payload: &Term,
    policy: Option<&OpPolicy>,
    error_token: SealId,
    public_operation: &str,
) -> Result<Value, EffectsError> {
    match operation {
        "core/pkg-low::init" => init(payload, policy, error_token, public_operation),
        "core/pkg-low::add" => add(payload, policy, error_token, public_operation),
        "core/pkg-low::list" => list(payload, policy, error_token, public_operation),
        _ => Err(EffectsError::Log(format!(
            "unknown parity lock operation: {operation}"
        ))),
    }
}

fn init(
    payload: &Term,
    policy: Option<&OpPolicy>,
    error_token: SealId,
    operation: &str,
) -> Result<Value, EffectsError> {
    let lock_s = match payload_pkg_lock(payload) {
        Ok(value) => value,
        Err(message) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-payload",
                message,
                Some(operation),
            ));
        }
    };
    let workspace = match payload_pkg_workspace(payload) {
        Ok(value) => value,
        Err(message) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-payload",
                message,
                Some(operation),
            ));
        }
    };
    let mut lock = gc_pkg::GenesisLock::empty(workspace);
    lock.policy = payload_pkg_policy(payload).unwrap_or_else(|| "policy:default-v0.1".to_string());
    if let Some(registry) = payload_pkg_registry_default(payload) {
        lock.registries.insert("default".to_string(), registry);
    }
    let bytes = lock.to_toml_canonical().into_bytes();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let base = effective_base_dir(policy)?;
    let create_dirs = policy.map(|value| value.create_dirs).unwrap_or(false);
    let path = sandbox_path_write(&base, &lock_s, create_dirs)
        .map_err(|error| EffectsError::Log(format!("parity path: {error}")))?;
    atomic_write_text(&path, &bytes).map_err(|error| {
        EffectsError::Log(format!("parity {operation} persistence failed: {error}"))
    })?;
    Ok(lock_write_result(lock_s, hash))
}

fn add(
    payload: &Term,
    policy: Option<&OpPolicy>,
    error_token: SealId,
    operation: &str,
) -> Result<Value, EffectsError> {
    let lock_s = match payload_pkg_lock(payload) {
        Ok(value) => value,
        Err(message) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-payload",
                message,
                Some(operation),
            ));
        }
    };
    let base = effective_base_dir(policy)?;
    let path = match sandbox_path_read(&base, &lock_s) {
        Ok(value) => value,
        Err(error) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/missing-lock",
                error.to_string(),
                Some(operation),
            ));
        }
    };
    let mut lock = match gc_pkg::GenesisLock::load(&path) {
        Ok(value) => value,
        Err(error) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-lock",
                error.to_string(),
                Some(operation),
            ));
        }
    };
    let name = match payload_pkg_name(payload) {
        Ok(value) => value,
        Err(message) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-payload",
                message,
                Some(operation),
            ));
        }
    };
    let selector = match payload_pkg_selector(payload) {
        Ok(value) => value,
        Err(message) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-payload",
                message,
                Some(operation),
            ));
        }
    };
    let update_policy = match payload_pkg_update_policy(payload) {
        Ok(Some(value)) => value,
        Ok(None) => gc_pkg::UpdatePolicy::Manual,
        Err(message) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-payload",
                message,
                Some(operation),
            ));
        }
    };
    let strategy = match payload_pkg_strategy(payload) {
        Ok(value) => value,
        Err(message) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-payload",
                message,
                Some(operation),
            ));
        }
    };
    let tag_policy = match payload_pkg_tag_policy(payload) {
        Ok(value) => value,
        Err(message) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-payload",
                message,
                Some(operation),
            ));
        }
    };
    lock.set_requirement_with_metadata(
        &name,
        &selector,
        update_policy,
        payload_pkg_registry(payload),
        strategy,
        tag_policy,
    );
    let bytes = lock.to_toml_canonical().into_bytes();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let write_path = sandbox_path_write(&base, &lock_s, false)
        .map_err(|error| EffectsError::Log(format!("parity path: {error}")))?;
    atomic_write_text(&write_path, &bytes).map_err(|error| {
        EffectsError::Log(format!("parity {operation} persistence failed: {error}"))
    })?;
    Ok(lock_write_result(lock_s, hash))
}

fn list(
    payload: &Term,
    policy: Option<&OpPolicy>,
    error_token: SealId,
    operation: &str,
) -> Result<Value, EffectsError> {
    let lock_s = match payload_pkg_lock(payload) {
        Ok(value) => value,
        Err(message) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-payload",
                message,
                Some(operation),
            ));
        }
    };
    let path = match sandbox_path_read(&effective_base_dir(policy)?, &lock_s) {
        Ok(value) => value,
        Err(error) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/missing-lock",
                error.to_string(),
                Some(operation),
            ));
        }
    };
    let lock = match gc_pkg::GenesisLock::load(&path) {
        Ok(value) => value,
        Err(error) => {
            return Ok(mk_error(
                error_token,
                "core/pkg/bad-lock",
                error.to_string(),
                Some(operation),
            ));
        }
    };
    let requirements = lock
        .requirements
        .into_iter()
        .map(|(name, requirement)| {
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":name")), Term::Str(name)),
                    (
                        TermOrdKey(Term::symbol(":registry")),
                        requirement.registry.map(Term::Str).unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":selector")),
                        Term::Str(requirement.selector),
                    ),
                    (
                        TermOrdKey(Term::symbol(":strategy")),
                        Term::symbol(format!(":{}", requirement.strategy.as_str())),
                    ),
                    (
                        TermOrdKey(Term::symbol(":tag-policy")),
                        requirement.tag_policy.map(Term::Str).unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":update-policy")),
                        Term::symbol(match requirement.update_policy {
                            gc_pkg::UpdatePolicy::Manual => ":manual",
                            gc_pkg::UpdatePolicy::Auto => ":auto",
                        }),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();
    let locked = lock
        .locked
        .into_iter()
        .map(|(name, entry)| {
            Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":commit")),
                        entry.commit.map(Term::Str).unwrap_or(Term::Nil),
                    ),
                    (
                        TermOrdKey(Term::symbol(":environment-fingerprint")),
                        entry
                            .environment_fingerprint
                            .map(Term::Str)
                            .unwrap_or(Term::Nil),
                    ),
                    (TermOrdKey(Term::symbol(":name")), Term::Str(name)),
                    (
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(entry.snapshot),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();
    Ok(Value::data(Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (TermOrdKey(Term::symbol(":lock")), Term::Str(lock_s)),
            (
                TermOrdKey(Term::symbol(":requirements")),
                Term::Vector(requirements),
            ),
            (TermOrdKey(Term::symbol(":locked")), Term::Vector(locked)),
        ]
        .into_iter()
        .collect(),
    )))
}
