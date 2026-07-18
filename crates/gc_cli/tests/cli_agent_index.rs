use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn genesisbench_tracks_and_hardware_classes_are_closed() {
    let profile: Value = serde_json::from_slice(
        &fs::read(repo_root().join("docs/spec/GENESISBENCH_PROTOCOL_v0.1.json")).unwrap(),
    )
    .unwrap();
    let policy = &profile["trackPolicy"];
    assert_eq!(policy["crossTrackRankingAllowed"], false);
    assert_eq!(
        policy["rawScaffoldedAdaptedAndLocalAggregatesMayMix"],
        false
    );
    let tracks: Vec<_> = policy["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        tracks,
        [
            "cold-acquisition",
            "embedded-local",
            "genesis-adapted",
            "open-agent"
        ]
    );
    let bounds: Vec<_> = policy["hardwareClasses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["maxCombinedResidentBytes"].as_u64().unwrap())
        .collect();
    assert_eq!(bounds, [4_u64 << 30, 16_u64 << 30, 64_u64 << 30]);
}

#[test]
fn genesisbench_temporal_epoch_and_overlay_pass_public_controls() {
    for (script, expected) in [
        (
            "scripts/lib/gc_held_out_evaluation.py",
            "lineages=90 controls=30",
        ),
        ("scripts/lib/gc_capability_lease.py", "controls=7"),
    ] {
        let output = Command::new("python3")
            .current_dir(repo_root())
            .args([script, "--check", "--self-test"])
            .output()
            .expect("execute temporal benchmark verifier");
        assert!(
            output.status.success(),
            "{script} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(expected),
            "{script} omitted expected evidence"
        );
    }
}

#[test]
fn genesisbench_reference_agent_and_ablations_pass_public_controls() {
    let output = Command::new("python3")
        .current_dir(repo_root())
        .args([
            "scripts/lib/genesisbench_reference_agent.py",
            "--check",
            "--self-test",
        ])
        .output()
        .expect("execute fixed reference agent verifier");
    assert!(
        output.status.success(),
        "reference agent verifier failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ablations=8 lineages=9 conditions=72 controls=25"));
}

#[test]
fn agent_card_and_symbol_search_are_bounded_canonical_authorities() {
    let card_out = cargo_bin_cmd!("genesis")
        .args(["--json", "agent-index", "--card", "core"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let card: Value = serde_json::from_slice(&card_out).expect("card JSON");
    assert_eq!(card["kind"], "genesis/agent-card-v0.3");
    assert_eq!(card["data"]["card_name"], "core");
    assert_eq!(
        card["data"]["card"]["kind"],
        "genesis/gc-agent-core-card-v0.3"
    );

    let search_out = cargo_bin_cmd!("genesis")
        .args([
            "--json",
            "agent-index",
            "--search-symbol",
            "int/",
            "--max-results",
            "2",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let search: Value = serde_json::from_slice(&search_out).expect("search JSON");
    assert_eq!(search["kind"], "genesis/agent-symbol-search-v0.3");
    assert_eq!(search["data"]["max_results"], 2);
    assert_eq!(search["data"]["results"].as_array().map(Vec::len), Some(2));
    assert_eq!(search["data"]["truncated"], true);
}

#[test]
fn agent_index_emits_expected_schema_and_sources() {
    let out = cargo_bin_cmd!("genesis")
        .args(["--json", "agent-index"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("parse json");

    assert_eq!(
        json.get("kind").and_then(Value::as_str),
        Some("genesis/agent-index-v0.1")
    );
    assert_eq!(
        json.pointer("/data/schema").and_then(Value::as_str),
        Some("genesis/agent-index-v0.1")
    );
    assert_eq!(
        json.pointer("/data/runtime_profile")
            .and_then(Value::as_str),
        Some("production")
    );
    assert_eq!(
        json.pointer("/data/cli_schema/schema")
            .and_then(Value::as_str),
        Some("genesis/cli-schema-v0.1")
    );
    assert_eq!(
        json.pointer("/data/capability_indices/host_abi/path")
            .and_then(Value::as_str),
        Some("docs/spec/HOST_ABI_INDEX_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/capability_indices/host_abi_schema/path")
            .and_then(Value::as_str),
        Some("docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/capability_indices/prelude_capabilities/path")
            .and_then(Value::as_str),
        Some("docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/selfhost_symbol_index/schema")
            .and_then(Value::as_str),
        Some("genesis/selfhost-symbol-ownership-index-v0.1")
    );
    assert_eq!(
        json.pointer("/data/selfhost_symbol_index/path")
            .and_then(Value::as_str),
        Some("selfhost/toolchain_manifest.gc")
    );
    assert_eq!(
        json.pointer("/data/docs/agent_authoring_bundle")
            .and_then(Value::as_str),
        Some("docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md")
    );
    assert_eq!(
        json.pointer("/data/docs/agent_plan")
            .and_then(Value::as_str),
        Some("docs/spec/AGENT_INDEX_v0.1.md#agent-plan-v01")
    );
    for (key, path) in [
        (
            "genesisbench_local_models_schema",
            "docs/spec/GENESISBENCH_LOCAL_MODELS_v0.1.schema.json",
        ),
        (
            "genesisbench_local_model_preselection",
            "benchmarks/genesisbench/v0.1/local-models/preselection.json",
        ),
        (
            "genesisbench_local_model_inventory",
            "benchmarks/genesisbench/v0.1/local-models/inventory.json",
        ),
    ] {
        assert_eq!(
            json.pointer(&format!("/data/docs/{key}"))
                .and_then(Value::as_str),
            Some(path),
        );
    }
    assert_eq!(
        json.pointer("/data/docs/gc_agent_corpus")
            .and_then(Value::as_str),
        Some("docs/spec/GC_AGENT_CORPUS_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/gc_canonical_examples")
            .and_then(Value::as_str),
        Some("examples/canonical_language/v0.1/suite.json")
    );
    assert_eq!(
        json.pointer("/data/docs/gc_agent_task_benchmark")
            .and_then(Value::as_str),
        Some("benchmarks/agent_tasks/v0.1/suite.json")
    );
    assert_eq!(
        json.pointer("/data/docs/gc_agent_benchmark_scoring")
            .and_then(Value::as_str),
        Some("docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/gc_agent_benchmark_score_schema")
            .and_then(Value::as_str),
        Some("docs/spec/GC_AGENT_BENCHMARK_SCORE_v0.1.schema.json")
    );
    assert_eq!(
        json.pointer("/data/docs/gc_agent_benchmark_run_schema")
            .and_then(Value::as_str),
        Some("docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_protocol")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_PROTOCOL_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_construct_validity_policy")
            .and_then(Value::as_str),
        Some("policies/genesisbench_construct_validity_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_construct_validity_schema")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_CONSTRUCT_VALIDITY_v0.1.schema.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_construct_validity_report")
            .and_then(Value::as_str),
        Some("benchmarks/genesisbench/v0.1/construct-validity/report.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_reference_agent")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_REFERENCE_AGENT_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_reference_agent_ablations")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_REFERENCE_AGENT_ABLATIONS_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_reference_agent_trace_schema")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_REFERENCE_AGENT_TRACE_v0.1.schema.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_front_door")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_FRONT_DOOR_v0.1.md")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_adapters")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_ADAPTERS_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_adapters_schema")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_ADAPTERS_v0.1.schema.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_execution_run_schema")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_EXECUTION_RUN_v0.1.schema.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_bundle_manifest_schema")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_BUNDLE_MANIFEST_v0.1.schema.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_analysis_plan")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_ANALYSIS_PLAN_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_observations_schema")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_OBSERVATIONS_v0.1.schema.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_analysis_report_schema")
            .and_then(Value::as_str),
        Some("docs/spec/GENESISBENCH_ANALYSIS_REPORT_v0.1.schema.json")
    );
    assert_eq!(
        json.pointer("/data/docs/gc_agent_model_runner_effect")
            .and_then(Value::as_str),
        Some("docs/spec/GC_AGENT_MODEL_RUNNER_EFFECT_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/gc_agent_held_out_evaluation")
            .and_then(Value::as_str),
        Some("docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/genesisbench_temporal_epoch_audit")
            .and_then(Value::as_str),
        Some("docs/program/GENESISBENCH_TEMPORAL_EPOCH_AUDIT_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/docs/gc_capability_lease_protocol")
            .and_then(Value::as_str),
        Some("docs/spec/GC_CAPABILITY_LEASE_PROTOCOL_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/language_symbol_index/path")
            .and_then(Value::as_str),
        Some("docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json")
    );
    assert_eq!(
        json.pointer("/data/language_symbol_index/symbol_count")
            .and_then(Value::as_u64),
        Some(170)
    );
    assert_eq!(
        json.pointer("/data/language_symbol_index/unsupported_behavior_count")
            .and_then(Value::as_u64),
        Some(12)
    );
    assert_eq!(
        json.pointer("/data/language_symbol_index/unsupported_classes/0")
            .and_then(Value::as_str),
        Some("experimental-syntax")
    );
    assert_eq!(
        json.pointer("/data/language_symbol_index/lookup/command")
            .and_then(Value::as_str),
        Some("genesis --json agent-index --symbol <exact-name>")
    );
    assert_eq!(
        json.pointer("/data/diagnostic_catalog/path")
            .and_then(Value::as_str),
        Some("docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json")
    );
    assert_eq!(
        json.pointer("/data/diagnostic_catalog/schema")
            .and_then(Value::as_str),
        Some("genesis/diagnostic-catalog-v0.1")
    );
    assert!(
        json.pointer("/data/diagnostic_catalog/diagnostic_count")
            .and_then(Value::as_u64)
            .is_some_and(|count| count >= 100)
    );
    assert_eq!(
        json.pointer("/data/docs/write_genesiscode_skill_pack")
            .and_then(Value::as_str),
        Some("docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md")
    );
    assert_eq!(
        json.pointer("/data/selfhost_symbol_index/loaded")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        json.pointer("/data/selfhost_symbol_index/symbol_count")
            .and_then(Value::as_u64)
            .is_some_and(|n| n > 0),
        "selfhost symbol index must expose owned symbols"
    );
    let symbols = json
        .pointer("/data/selfhost_symbol_index/symbols")
        .and_then(Value::as_array)
        .expect("selfhost_symbol_index.symbols");
    assert!(
        symbols.iter().any(|entry| {
            entry.get("symbol").and_then(Value::as_str) == Some("core/cli::fmt-module")
                && entry.get("module_path").and_then(Value::as_str).is_some()
        }),
        "selfhost symbol ownership index must include core/cli::fmt-module"
    );

    let workflows = json
        .pointer("/data/reference_workflows")
        .and_then(Value::as_array)
        .expect("reference_workflows");
    assert!(
        workflows
            .iter()
            .any(|w| w.get("name").and_then(Value::as_str) == Some("agent_compute_workflow")),
        "agent_compute_workflow must be discoverable in agent index"
    );
}

#[test]
fn agent_symbol_exact_lookup_is_bounded_and_self_contained() {
    let out = cargo_bin_cmd!("genesis")
        .args(["--json", "agent-index", "--symbol", "int/add"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("parse exact symbol JSON");
    assert_eq!(
        json.get("kind").and_then(Value::as_str),
        Some("genesis/agent-symbol-v0.3")
    );
    assert_eq!(
        json.pointer("/data/symbol/symbol").and_then(Value::as_str),
        Some("int/add")
    );
    assert_eq!(
        json.pointer("/data/symbol/signature/notation")
            .and_then(Value::as_str),
        Some("(prim int/add arg1:Int arg2:Int) -> Int")
    );
    for field in [
        "effects",
        "capabilities",
        "contracts",
        "examples",
        "diagnostics",
        "deprecation",
        "sources",
    ] {
        assert!(
            json.pointer(&format!("/data/symbol/{field}")).is_some(),
            "exact record omitted {field}"
        );
    }
    assert!(json.pointer("/data/symbols").is_none());
}

#[test]
fn agent_symbol_exact_lookup_rejects_case_drift_and_unknown_names() {
    for symbol in ["Int/Add", "not/a-symbol"] {
        let out = cargo_bin_cmd!("genesis")
            .args(["--json", "agent-index", "--symbol", symbol])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let json: Value = serde_json::from_slice(&out).expect("parse lookup error JSON");
        assert_eq!(
            json.pointer("/error/code").and_then(Value::as_str),
            Some("agent-index/symbol-not-found")
        );
    }
}

#[test]
fn agent_diagnostic_exact_lookup_is_bounded_and_self_contained() {
    let out = cargo_bin_cmd!("genesis")
        .args(["--json", "agent-index", "--diagnostic", "replay/mismatch"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("parse exact diagnostic JSON");
    assert_eq!(
        json.get("kind").and_then(Value::as_str),
        Some("genesis/diagnostic-v0.1")
    );
    assert_eq!(
        json.pointer("/data/diagnostic/id").and_then(Value::as_str),
        Some("genesis/diagnostic/v1/replay/mismatch")
    );
    assert_eq!(
        json.pointer("/data/diagnostic/phase")
            .and_then(Value::as_str),
        Some("replay")
    );
    for field in [
        "primarySpan",
        "relatedSpans",
        "parameters",
        "likelyCauses",
        "safeRepairActions",
        "documentation",
        "sourceAuthorities",
    ] {
        assert!(
            json.pointer(&format!("/data/diagnostic/{field}")).is_some(),
            "exact diagnostic omitted {field}"
        );
    }
    assert!(json.pointer("/data/diagnostics").is_none());
}

#[test]
fn agent_diagnostic_lookup_rejects_case_drift_unknown_and_padding() {
    for code in ["Replay/mismatch", "not/a-diagnostic", " replay/mismatch"] {
        let out = cargo_bin_cmd!("genesis")
            .args(["--json", "agent-index", "--diagnostic", code])
            .assert()
            .failure()
            .get_output()
            .stdout
            .clone();
        let json: Value = serde_json::from_slice(&out).expect("parse diagnostic lookup error");
        assert!(
            matches!(
                json.pointer("/error/code").and_then(Value::as_str),
                Some("agent-index/diagnostic-not-found" | "agent-index/diagnostic-invalid")
            ),
            "unexpected lookup failure: {json}"
        );
    }
}

#[test]
fn agent_index_parity_profile_reports_runtime_profile() {
    let out = cargo_bin_cmd!("genesis_parity")
        .args(["--json", "agent-index"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&out).expect("parse json");

    assert_eq!(
        json.pointer("/data/runtime_profile")
            .and_then(Value::as_str),
        Some("parity-harness")
    );
}
