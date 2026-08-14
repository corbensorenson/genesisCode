use super::*;
use crate::policy::{
    AuthorizedPositiveI64, AuthorizedStringList, AuthorizedXrBackend, AuthorizedXrPolicy,
};

#[path = "policy_authority_xr_decode.rs"]
mod decode;

const BACKEND: &str = "xr_backend";
const RUNTIME_PROFILE: &str = "runtime_profile";
const RUNTIME_PROFILE_ALIAS: &str = "host_runtime_profile";

fn list_input(value: Option<&toml::Value>) -> Term {
    match value {
        None => Term::Nil,
        Some(value) => match value.as_array() {
            Some(values) => Term::Vector(
                values
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(|value| Term::Str(value.to_string()))
                            .unwrap_or_else(|| Term::symbol(":invalid-entry"))
                    })
                    .collect(),
            ),
            None => Term::symbol(":invalid-type"),
        },
    }
}

fn integer_input(value: Option<&toml::Value>) -> Term {
    match value {
        None => Term::Nil,
        Some(value) => value
            .as_integer()
            .map(|value| Term::Int(value.into()))
            .unwrap_or_else(|| Term::symbol(":invalid-type")),
    }
}

pub(super) fn input(table: Option<&toml::value::Table>) -> Term {
    let get = |key| table.and_then(|table| table.get(key));
    Term::Vector(vec![
        list_input(get("allow_anchor_spaces")),
        network::optional_bool_input(get("allow_hand_tracking")),
        list_input(get("allow_haptics_inputs")),
        network::optional_bool_input(get("allow_hit_test")),
        list_input(get("allow_layer_types")),
        network::optional_bool_input(get("allow_spatial_mesh")),
        network::optional_string_input(get(BACKEND)),
        integer_input(get("max_anchors")),
        integer_input(get("max_hand_joints")),
        integer_input(get("max_haptics_amplitude")),
        integer_input(get("max_haptics_duration_ms")),
        integer_input(get("max_hit_results")),
        integer_input(get("max_layer_opacity")),
        integer_input(get("max_layers")),
        integer_input(get("max_mesh_vertices")),
        integer_input(get("max_meshes")),
        network::optional_string_input(get(RUNTIME_PROFILE)),
        network::optional_string_input(get(RUNTIME_PROFILE_ALIAS)),
    ])
}

fn selected_runtime(table: Option<&BTreeMap<String, toml::Value>>) -> Option<&toml::Value> {
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

fn legacy_backend(
    table: Option<&BTreeMap<String, toml::Value>>,
    bridge_active: bool,
) -> AuthorizedXrBackend {
    let production = || {
        if bridge_active {
            AuthorizedXrBackend::WebxrDevice
        } else {
            AuthorizedXrBackend::ProductionRequiresBridge
        }
    };
    let backend = table
        .and_then(|table| table.get(BACKEND))
        .and_then(toml::Value::as_str);
    if backend.is_none() && is_production(selected_runtime(table)) {
        return production();
    }
    let Some(raw) = backend else {
        return AuthorizedXrBackend::FirstParty;
    };
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "first-party" | "first-party-runtime" | "headless-sim" | "xr-headless-sim" => {
            AuthorizedXrBackend::FirstParty
        }
        "production" | "prod" | "release" => production(),
        "webxr-device" | "device-runtime" | "browser-device" => AuthorizedXrBackend::WebxrDevice,
        _ => AuthorizedXrBackend::Invalid(normalized),
    }
}

fn legacy_list(value: Option<&toml::Value>, lowercase: bool) -> AuthorizedStringList {
    let Some(value) = value else {
        return AuthorizedStringList::Absent;
    };
    let Some(values) = value.as_array() else {
        return AuthorizedStringList::InvalidType;
    };
    let mut out = Vec::new();
    for value in values {
        let Some(value) = value.as_str() else {
            return AuthorizedStringList::InvalidEntry;
        };
        let value = value.trim();
        if !value.is_empty() {
            out.push(if lowercase {
                value.to_ascii_lowercase()
            } else {
                value.to_string()
            });
        }
    }
    if out.is_empty() {
        return AuthorizedStringList::Empty;
    }
    AuthorizedStringList::Valid(out)
}

fn legacy_positive(value: Option<&toml::Value>, maximum: Option<i64>) -> AuthorizedPositiveI64 {
    let Some(value) = value else {
        return AuthorizedPositiveI64::Absent;
    };
    let Some(value) = value.as_integer() else {
        return AuthorizedPositiveI64::InvalidType;
    };
    if value <= 0 {
        AuthorizedPositiveI64::NonPositive
    } else if maximum.is_some_and(|maximum| value > maximum) {
        AuthorizedPositiveI64::OutOfRange
    } else {
        AuthorizedPositiveI64::Valid(value)
    }
}

pub(crate) fn legacy(policy: Option<&OpPolicy>, bridge_active: bool) -> AuthorizedXrPolicy {
    let table = policy.map(|policy| &policy.extra);
    let get = |key| table.and_then(|table| table.get(key));
    AuthorizedXrPolicy {
        backend: legacy_backend(table, bridge_active),
        allow_haptics_inputs: legacy_list(get("allow_haptics_inputs"), false),
        max_haptics_amplitude: legacy_positive(get("max_haptics_amplitude"), Some(1000)),
        max_haptics_duration_ms: legacy_positive(get("max_haptics_duration_ms"), None),
        allow_hand_tracking: network::legacy_optional_bool(get("allow_hand_tracking")),
        max_hand_joints: legacy_positive(get("max_hand_joints"), None),
        allow_hit_test: network::legacy_optional_bool(get("allow_hit_test")),
        max_hit_results: legacy_positive(get("max_hit_results"), None),
        allow_spatial_mesh: network::legacy_optional_bool(get("allow_spatial_mesh")),
        max_meshes: legacy_positive(get("max_meshes"), None),
        max_mesh_vertices: legacy_positive(get("max_mesh_vertices"), None),
        allow_anchor_spaces: legacy_list(get("allow_anchor_spaces"), true),
        max_anchors: legacy_positive(get("max_anchors"), None),
        allow_layer_types: legacy_list(get("allow_layer_types"), true),
        max_layers: legacy_positive(get("max_layers"), None),
        max_layer_opacity: legacy_positive(get("max_layer_opacity"), None),
    }
}

fn field<'a>(map: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> Result<&'a Term, EffectsError> {
    map.get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| authority_error(format!("result :xr-policy is missing {name}")))
}

pub(crate) fn decode(term: &Term, allowed: bool) -> Result<AuthorizedXrPolicy, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(legacy(None, false))
        } else {
            Err(authority_error("denied result :xr-policy must be nil"))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error("admitted result :xr-policy must be a map"));
    };
    let expected = [
        ":allow-anchor-spaces",
        ":allow-hand-tracking",
        ":allow-haptics-inputs",
        ":allow-hit-test",
        ":allow-layer-types",
        ":allow-spatial-mesh",
        ":backend",
        ":invalid-value",
        ":max-anchors",
        ":max-hand-joints",
        ":max-haptics-amplitude",
        ":max-haptics-duration-ms",
        ":max-hit-results",
        ":max-layer-opacity",
        ":max-layers",
        ":max-mesh-vertices",
        ":max-meshes",
    ]
    .into_iter()
    .map(|key| TermOrdKey(Term::symbol(key)))
    .collect::<BTreeSet<_>>();
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result :xr-policy field set mismatch"));
    }
    Ok(AuthorizedXrPolicy {
        backend: decode::backend(
            map.get(&TermOrdKey(Term::symbol(":backend"))),
            map.get(&TermOrdKey(Term::symbol(":invalid-value"))),
        )?,
        allow_haptics_inputs: decode::string_list(
            field(map, ":allow-haptics-inputs")?,
            ":allow-haptics-inputs",
            false,
        )?,
        max_haptics_amplitude: decode::positive_i64(
            field(map, ":max-haptics-amplitude")?,
            ":max-haptics-amplitude",
            true,
        )?,
        max_haptics_duration_ms: decode::positive_i64(
            field(map, ":max-haptics-duration-ms")?,
            ":max-haptics-duration-ms",
            false,
        )?,
        allow_hand_tracking: decode::bool_field(
            field(map, ":allow-hand-tracking")?,
            ":allow-hand-tracking",
        )?,
        max_hand_joints: decode::positive_i64(
            field(map, ":max-hand-joints")?,
            ":max-hand-joints",
            false,
        )?,
        allow_hit_test: decode::bool_field(field(map, ":allow-hit-test")?, ":allow-hit-test")?,
        max_hit_results: decode::positive_i64(
            field(map, ":max-hit-results")?,
            ":max-hit-results",
            false,
        )?,
        allow_spatial_mesh: decode::bool_field(
            field(map, ":allow-spatial-mesh")?,
            ":allow-spatial-mesh",
        )?,
        max_meshes: decode::positive_i64(field(map, ":max-meshes")?, ":max-meshes", false)?,
        max_mesh_vertices: decode::positive_i64(
            field(map, ":max-mesh-vertices")?,
            ":max-mesh-vertices",
            false,
        )?,
        allow_anchor_spaces: decode::string_list(
            field(map, ":allow-anchor-spaces")?,
            ":allow-anchor-spaces",
            true,
        )?,
        max_anchors: decode::positive_i64(field(map, ":max-anchors")?, ":max-anchors", false)?,
        allow_layer_types: decode::string_list(
            field(map, ":allow-layer-types")?,
            ":allow-layer-types",
            true,
        )?,
        max_layers: decode::positive_i64(field(map, ":max-layers")?, ":max-layers", false)?,
        max_layer_opacity: decode::positive_i64(
            field(map, ":max-layer-opacity")?,
            ":max-layer-opacity",
            false,
        )?,
    })
}
