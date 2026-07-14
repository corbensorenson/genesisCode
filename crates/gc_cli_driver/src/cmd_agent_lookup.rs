use super::*;

const AGENT_CORE_CARD_JSON: &str = include_str!("../../../docs/spec/GC_AGENT_CORE_CARD_v0.3.json");
const AGENT_PROFILE_JSON: &str = include_str!("../../../docs/spec/GC_AGENT_PROFILE_v0.3.json");
const AGENT_TASK_CARDS_JSON: &str =
    include_str!("../../../docs/spec/GC_AGENT_TASK_CARDS_v0.3.json");

fn parse_embedded_card(source: &str, expected_kind: &str) -> Result<serde_json::Value, CliError> {
    let card: serde_json::Value = serde_json::from_str(source).map_err(|error| {
        cli_err(
            EX_INTERNAL,
            "agent-index/card-invalid",
            format!("embedded agent card is invalid JSON: {error}"),
        )
    })?;
    if card.get("kind").and_then(serde_json::Value::as_str) != Some(expected_kind) {
        return Err(cli_err(
            EX_INTERNAL,
            "agent-index/card-invalid",
            "embedded agent card kind does not match its frozen authority",
        ));
    }
    Ok(card)
}

pub(super) fn cmd_agent_card(cli: &Cli, card: AgentCardArg) -> Result<CmdOut, CliError> {
    let (source, expected_kind) = match card {
        AgentCardArg::Core => (AGENT_CORE_CARD_JSON, "genesis/gc-agent-core-card-v0.3"),
        AgentCardArg::Profile => (AGENT_PROFILE_JSON, "genesis/gc-agent-profile-v0.3"),
        AgentCardArg::Tasks => (AGENT_TASK_CARDS_JSON, "genesis/gc-agent-task-cards-v0.3"),
    };
    let card_value = parse_embedded_card(source, expected_kind)?;
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/agent-card-v0.3",
        data: Some(serde_json::json!({
            "schema": "genesis/agent-card-v0.3",
            "card_name": card.as_str(),
            "card_kind": expected_kind,
            "card": card_value,
        })),
        error: None,
    };
    let json = json_envelope_value(env)?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}

pub(super) fn cmd_agent_symbol_search(
    cli: &Cli,
    query: &str,
    max_results: u64,
) -> Result<CmdOut, CliError> {
    if query.is_empty()
        || query.trim() != query
        || query.len() > 128
        || !query.is_ascii()
        || !(1..=64).contains(&max_results)
    {
        return Err(cli_err(
            EX_PARSE,
            "agent-index/search-invalid",
            "symbol query must be 1..128 unpadded ASCII bytes and max_results must be 1..=64",
        ));
    }
    let index = cmd_agent_index::embedded_agent_symbol_index()?;
    let symbols = index
        .get("symbols")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "agent-index/symbol-index-invalid",
                "embedded symbol index has no symbols array",
            )
        })?;
    let mut matches = symbols
        .iter()
        .filter_map(|entry| {
            let symbol = entry.get("symbol")?.as_str()?;
            let rank = if symbol == query {
                0
            } else if symbol.starts_with(query) {
                1
            } else if symbol.contains(query) {
                2
            } else {
                return None;
            };
            Some((rank, symbol, entry))
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| (left.0, left.1).cmp(&(right.0, right.1)));
    let total_matches = matches.len();
    let limit = usize::try_from(max_results).unwrap_or(64);
    let results = matches
        .into_iter()
        .take(limit)
        .map(|(_, _, entry)| entry.clone())
        .collect::<Vec<_>>();
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/agent-symbol-search-v0.3",
        data: Some(serde_json::json!({
            "schema": "genesis/agent-symbol-search-v0.3",
            "profile_id": index["profileId"],
            "profile_identity_sha256": index["profileIdentitySha256"],
            "index_identity_sha256": index["indexIdentitySha256"],
            "query": query,
            "case_sensitive": true,
            "ranking": ["exact", "prefix", "substring", "symbol-ascending"],
            "max_results": max_results,
            "total_matches": total_matches,
            "truncated": total_matches > results.len(),
            "results": results,
        })),
        error: None,
    };
    let json = json_envelope_value(env)?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}
