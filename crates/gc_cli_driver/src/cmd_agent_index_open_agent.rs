pub(super) fn insert_docs(docs: &mut serde_json::Value) {
    for (key, path) in [
        (
            "genesisbench_open_agent",
            "docs/spec/GENESISBENCH_OPEN_AGENT_v0.3.json",
        ),
        (
            "genesisbench_open_agent_v0_1",
            "docs/spec/GENESISBENCH_OPEN_AGENT_v0.1.json",
        ),
        (
            "genesisbench_open_agent_v0_2",
            "docs/spec/GENESISBENCH_OPEN_AGENT_v0.2.json",
        ),
        (
            "genesisbench_open_agent_campaign_schema",
            "docs/spec/GENESISBENCH_OPEN_AGENT_CAMPAIGN_v0.1.schema.json",
        ),
        (
            "genesisbench_open_agent_campaign_report_schema",
            "docs/spec/GENESISBENCH_OPEN_AGENT_CAMPAIGN_REPORT_v0.1.schema.json",
        ),
        (
            "genesisbench_open_agent_predeclaration_schema",
            "docs/spec/GENESISBENCH_OPEN_AGENT_PREDECLARATION_v0.1.schema.json",
        ),
        (
            "genesisbench_open_agent_run_schema",
            "docs/spec/GENESISBENCH_OPEN_AGENT_RUN_v0.1.schema.json",
        ),
        (
            "genesisbench_open_agent_tool_archive_schema",
            "docs/spec/GENESISBENCH_OPEN_AGENT_TOOL_ARCHIVE_v0.1.schema.json",
        ),
    ] {
        docs[key] = serde_json::Value::String(path.to_string());
    }
}
