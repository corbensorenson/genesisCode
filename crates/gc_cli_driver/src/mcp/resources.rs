use serde_json::{Value, json};

use super::super::*;

const CORE_CARD: &str = include_str!("../../../../docs/spec/GC_AGENT_CORE_CARD_v0.3.json");
const AGENT_PROFILE: &str = include_str!("../../../../docs/spec/GC_AGENT_PROFILE_v0.3.json");
const TASK_CARDS: &str = include_str!("../../../../docs/spec/GC_AGENT_TASK_CARDS_v0.3.json");
const SYMBOL_INDEX: &str = include_str!("../../../../docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json");
const DIAGNOSTIC_CATALOG: &str =
    include_str!("../../../../docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json");

const RESOURCES: &[(&str, &str, &str)] = &[
    (
        "genesis://cli/schema",
        "GenesisCode CLI schema",
        "Schema-derived canonical CLI command tree.",
    ),
    (
        "genesis://mcp/profile",
        "GenesisCode MCP profile",
        "Pinned generated MCP interface and identity.",
    ),
    (
        "genesis://agent/core-card",
        "GenesisCode core card",
        "Bounded agent language card.",
    ),
    (
        "genesis://agent/profile",
        "GenesisCode agent profile",
        "Complete machine-readable agent profile.",
    ),
    (
        "genesis://agent/task-cards",
        "GenesisCode task cards",
        "Intent-selectable agent task cards.",
    ),
    (
        "genesis://agent/symbol-index",
        "GenesisCode symbol index",
        "Exact deterministic language symbol index.",
    ),
    (
        "genesis://diagnostics/catalog",
        "GenesisCode diagnostic catalog",
        "Versioned diagnostics and safe repair actions.",
    ),
];

pub(super) fn resource_definitions() -> Vec<Value> {
    RESOURCES
        .iter()
        .map(|(uri, name, description)| {
            json!({
                "uri": uri,
                "name": name,
                "title": name,
                "description": description,
                "mimeType": "application/json"
            })
        })
        .collect()
}

pub(super) fn read_resource(uri: &str, profile: RuntimeProfile) -> Result<Value, String> {
    let value = match uri {
        "genesis://cli/schema" => json!({
            "schema": "genesis/cli-schema-v0.1",
            "runtime_profile": cli_schema::runtime_profile_token(profile),
            "command": cli_schema::build_cli_schema(profile),
            "mcp_interface": super::catalog::interface_manifest(profile)?,
        }),
        "genesis://mcp/profile" => super::catalog::interface_manifest(profile)?,
        "genesis://agent/core-card" => parse_embedded(CORE_CARD, uri)?,
        "genesis://agent/profile" => parse_embedded(AGENT_PROFILE, uri)?,
        "genesis://agent/task-cards" => parse_embedded(TASK_CARDS, uri)?,
        "genesis://agent/symbol-index" => parse_embedded(SYMBOL_INDEX, uri)?,
        "genesis://diagnostics/catalog" => parse_embedded(DIAGNOSTIC_CATALOG, uri)?,
        _ => return Err("resource URI is not exposed by this server".to_string()),
    };
    Ok(json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": json_canonical_string(&value)
        }]
    }))
}

fn parse_embedded(source: &str, uri: &str) -> Result<Value, String> {
    serde_json::from_str(source).map_err(|_| format!("embedded resource `{uri}` is invalid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_resource_is_readable() {
        for resource in resource_definitions() {
            let uri = resource["uri"].as_str().expect("uri");
            let read = read_resource(uri, RuntimeProfile::Production).expect("read");
            assert_eq!(read["contents"][0]["uri"], uri);
        }
    }
}
