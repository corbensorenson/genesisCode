use super::*;
use crate::policy::AuthorizedXrBackend;

const BACKEND: &str = "xr_backend";
const RUNTIME_PROFILE: &str = "runtime_profile";
const RUNTIME_PROFILE_ALIAS: &str = "host_runtime_profile";

pub(super) fn input(table: Option<&toml::value::Table>) -> Term {
    let get = |key| table.and_then(|table| table.get(key));
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":backend")),
                network::optional_string_input(get(BACKEND)),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-profile")),
                network::optional_string_input(get(RUNTIME_PROFILE)),
            ),
            (
                TermOrdKey(Term::symbol(":runtime-profile-alias")),
                network::optional_string_input(get(RUNTIME_PROFILE_ALIAS)),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn selected_runtime<'a>(
    table: Option<&'a BTreeMap<String, toml::Value>>,
) -> Option<&'a toml::Value> {
    table.and_then(|table| {
        table
            .get(RUNTIME_PROFILE)
            .or_else(|| table.get(RUNTIME_PROFILE_ALIAS))
    })
}

fn is_production(value: Option<&toml::Value>) -> bool {
    value.and_then(toml::Value::as_str).is_some_and(|raw| {
        matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "production" | "prod" | "release"
        )
    })
}

fn production(bridge_active: bool) -> AuthorizedXrBackend {
    if bridge_active {
        AuthorizedXrBackend::WebxrDevice
    } else {
        AuthorizedXrBackend::ProductionRequiresBridge
    }
}

pub(crate) fn legacy(policy: Option<&OpPolicy>, bridge_active: bool) -> AuthorizedXrBackend {
    let table = policy.map(|policy| &policy.extra);
    let backend = table
        .and_then(|table| table.get(BACKEND))
        .and_then(toml::Value::as_str);
    if backend.is_none() && is_production(selected_runtime(table)) {
        return production(bridge_active);
    }
    let Some(raw) = backend else {
        return AuthorizedXrBackend::FirstParty;
    };
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "first-party" | "first-party-runtime" | "headless-sim" | "xr-headless-sim" => {
            AuthorizedXrBackend::FirstParty
        }
        "production" | "prod" | "release" => production(bridge_active),
        "webxr-device" | "device-runtime" | "browser-device" => AuthorizedXrBackend::WebxrDevice,
        _ => AuthorizedXrBackend::Invalid(normalized),
    }
}

pub(crate) fn decode(term: &Term, allowed: bool) -> Result<AuthorizedXrBackend, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(AuthorizedXrBackend::FirstParty)
        } else {
            Err(authority_error("denied result :xr-policy must be nil"))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error("admitted result :xr-policy must be a map"));
    };
    let expected = [":backend", ":invalid-value"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect::<BTreeSet<_>>();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result :xr-policy field set mismatch"));
    }
    let invalid = map
        .get(&TermOrdKey(Term::symbol(":invalid-value")))
        .ok_or_else(|| authority_error("result :xr-policy is missing :invalid-value"))?;
    match map.get(&TermOrdKey(Term::symbol(":backend"))) {
        Some(Term::Symbol(value)) if value == ":first-party-runtime" && invalid == &Term::Nil => {
            Ok(AuthorizedXrBackend::FirstParty)
        }
        Some(Term::Symbol(value)) if value == ":webxr-device" && invalid == &Term::Nil => {
            Ok(AuthorizedXrBackend::WebxrDevice)
        }
        Some(Term::Symbol(value))
            if value == ":production-requires-bridge" && invalid == &Term::Nil =>
        {
            Ok(AuthorizedXrBackend::ProductionRequiresBridge)
        }
        Some(Term::Symbol(value)) if value == ":invalid" => match invalid {
            Term::Str(value) if !value.is_empty() => {
                Ok(AuthorizedXrBackend::Invalid(value.clone()))
            }
            _ => Err(authority_error(
                "invalid XR backend decision must carry a nonempty string",
            )),
        },
        _ => Err(authority_error("contradictory result :xr-policy state")),
    }
}
