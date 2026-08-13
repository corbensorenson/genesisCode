use super::*;
use crate::policy::{AuthorizedGpuBackend, AuthorizedGpuFallback, AuthorizedGpuPolicy};

const DEFAULT_POLICY_ENV: &str = "GENESIS_GPU_BACKEND_POLICY_DEFAULT";

pub(super) fn observed_default() -> Option<String> {
    std::env::var(DEFAULT_POLICY_ENV).ok()
}

pub(super) fn input(table: Option<&toml::value::Table>, default_policy: Option<&str>) -> Term {
    let get = |key| table.and_then(|table| table.get(key));
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":backend")),
                network::optional_string_input(get("gpu_backend")),
            ),
            (
                TermOrdKey(Term::symbol(":fallback-default")),
                default_policy
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":fallback-override")),
                network::optional_string_input(get("gpu_backend_policy")),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn backend(raw: Option<&toml::Value>) -> AuthorizedGpuBackend {
    match raw
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "device-runtime" => AuthorizedGpuBackend::DeviceRuntimeSubmitIntrospection,
        "device-runtime-full" => AuthorizedGpuBackend::DeviceRuntimeFullLifecycle,
        _ => AuthorizedGpuBackend::FirstParty,
    }
}

fn fallback(raw: Option<&str>) -> AuthorizedGpuFallback {
    match raw.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
        "require-device" => AuthorizedGpuFallback::RequireDevice,
        _ => AuthorizedGpuFallback::AllowFallback,
    }
}

pub(super) fn legacy(
    policy: Option<&OpPolicy>,
    default_policy: Option<&str>,
) -> AuthorizedGpuPolicy {
    let extra = policy.map(|policy| &policy.extra);
    let get = |key| extra.and_then(|extra| extra.get(key));
    AuthorizedGpuPolicy {
        backend: backend(get("gpu_backend")),
        fallback: fallback(
            get("gpu_backend_policy")
                .and_then(toml::Value::as_str)
                .or(default_policy),
        ),
    }
}

pub(super) fn decode(term: &Term, allowed: bool) -> Result<AuthorizedGpuPolicy, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(legacy(None, None))
        } else {
            Err(authority_error("denied result :gpu-policy must be nil"))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error(
            "admitted result :gpu-policy must be a data map",
        ));
    };
    let expected: BTreeSet<_> = [":backend", ":fallback"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result :gpu-policy field set mismatch"));
    }
    let backend = match map.get(&TermOrdKey(Term::symbol(":backend"))) {
        Some(Term::Symbol(value)) if value == ":first-party-runtime" => {
            AuthorizedGpuBackend::FirstParty
        }
        Some(Term::Symbol(value)) if value == ":device-runtime" => {
            AuthorizedGpuBackend::DeviceRuntimeSubmitIntrospection
        }
        Some(Term::Symbol(value)) if value == ":device-runtime-full" => {
            AuthorizedGpuBackend::DeviceRuntimeFullLifecycle
        }
        _ => {
            return Err(authority_error(
                "result :gpu-policy :backend must be a supported symbol",
            ));
        }
    };
    let fallback = match map.get(&TermOrdKey(Term::symbol(":fallback"))) {
        Some(Term::Symbol(value)) if value == ":allow-fallback" => {
            AuthorizedGpuFallback::AllowFallback
        }
        Some(Term::Symbol(value)) if value == ":require-device" => {
            AuthorizedGpuFallback::RequireDevice
        }
        _ => {
            return Err(authority_error(
                "result :gpu-policy :fallback must be a supported symbol",
            ));
        }
    };
    Ok(AuthorizedGpuPolicy { backend, fallback })
}
