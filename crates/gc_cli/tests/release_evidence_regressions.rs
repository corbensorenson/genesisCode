use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize repo root")
}

fn isolated_bash() -> Command {
    let mut command = Command::new("bash");
    for name in [
        "CARGO_TARGET_DIR",
        "GENESIS_CARGO_CACHE_EPHEMERAL",
        "GENESIS_CARGO_CACHE_HIT",
        "GENESIS_CARGO_CACHE_KEY_SHA256",
        "GENESIS_CARGO_CACHE_RESOLVED",
        "GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON",
        "GENESIS_CARGO_CACHE_SCOPE",
        "GENESIS_GENERATED_STATE_LEASE_PID",
        "GENESIS_GENERATED_STATE_LEASE_TOKEN",
        "GENESIS_GENERATED_STATE_ROOT",
    ] {
        command.env_remove(name);
    }
    command
}

const ARTIFACTS: &[(&str, &str)] = &[
    (
        "agent_capability_gauntlet_report.json",
        "genesis/agent-capability-gauntlet-v0.1",
    ),
    ("agent_capability_gauntlet_history.jsonl", "jsonl-history"),
    (
        "runtime_backend_feature_matrix_report.json",
        "genesis/runtime-backend-feature-matrix-v0.1",
    ),
    (
        "runtime_backend_feature_matrix_history.jsonl",
        "jsonl-history",
    ),
    (
        "host_bridge_fault_injection_report.json",
        "genesis/host-bridge-fault-injection-v0.1",
    ),
    ("host_bridge_fault_injection_history.jsonl", "jsonl-history"),
    (
        "webxr_browser_conformance_report.json",
        "genesis/webxr-browser-conformance-v0.1",
    ),
    (
        "gpu_xr_productization_kits_report.json",
        "genesis/gpu-xr-productization-kits-v0.1",
    ),
    (
        "assurance_profile_packs_report.json",
        "genesis/assurance-profile-packs-v0.1",
    ),
    ("assurance_profile_packs_history.jsonl", "jsonl-history"),
    (
        "agent_workflow_runtime_parity_report.json",
        "genesis/agent-workflow-runtime-parity-v0.1",
    ),
    (
        "agent_workflow_runtime_parity_history.jsonl",
        "jsonl-history",
    ),
    (
        "agent_capability_gauntlet_native_report.json",
        "genesis/agent-capability-gauntlet-v0.1",
    ),
    (
        "agent_capability_gauntlet_native_history.jsonl",
        "jsonl-history",
    ),
    (
        "agent_capability_gauntlet_wasi_report.json",
        "genesis/agent-capability-gauntlet-v0.1",
    ),
    (
        "agent_capability_gauntlet_wasi_history.jsonl",
        "jsonl-history",
    ),
    (
        "agent_generative_workloads_report.json",
        "genesis/agent-generative-workloads-v0.1",
    ),
    ("agent_generative_workloads_history.jsonl", "jsonl-history"),
];

fn run_helper(root: &Path, args: &[&str]) -> Output {
    Command::new("python3")
        .arg(root.join("scripts/lib/health_profile_evidence.py"))
        .args(args)
        .current_dir(root)
        .output()
        .expect("run health evidence helper")
}

fn write_fixture_bundle(path: &Path) {
    for (name, kind) in ARTIFACTS {
        let payload = if *kind == "jsonl-history" {
            b"{\"kind\":\"fixture\",\"ok\":true}\n".to_vec()
        } else {
            serde_json::to_vec(&json!({"kind": kind, "ok": true})).expect("serialize fixture")
        };
        fs::write(path.join(name), payload).expect("write fixture artifact");
    }
}

#[test]
fn release_profile_uses_one_closed_bundle_without_duplicate_derived_workloads() {
    let root = repo_root();
    let health = fs::read_to_string(root.join("scripts/render_upgrade_plan_health_report.sh"))
        .expect("read release health runner");
    let bundle = fs::read_to_string(root.join("scripts/render_health_profile_evidence_bundle.sh"))
        .expect("read release evidence bundle");
    let parity =
        fs::read_to_string(root.join("scripts/render_agent_workflow_runtime_parity_report.sh"))
            .expect("read parity renderer");
    let web = fs::read_to_string(root.join("scripts/wasm_web_smoke.mjs")).expect("read Web smoke");
    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read CI workflow");
    let measurement = fs::read_to_string(root.join("scripts/lib/release_full_measurement.py"))
        .expect("read release measurement runner");
    let helper = fs::read_to_string(root.join("scripts/lib/health_profile_evidence.py"))
        .expect("read evidence helper");
    for marker in [
        "GENESIS_AGENT_PARITY_PREBUILT_REPORT",
        "GENESIS_AGENT_GENERATIVE_PREBUILT_REPORT",
        "GENESIS_GPU_XR_PRODUCTIZATION_PREBUILT_REPORT",
        "GENESIS_CHECK_HOST_BRIDGE_FAULT_REPORT",
    ] {
        assert!(
            health.contains(marker),
            "missing closed evidence reuse: {marker}"
        );
    }
    assert_eq!(
        health
            .matches("bash scripts/check_slo_report_contracts.sh")
            .count(),
        2,
        "prepush and release must each schedule SLO validation exactly once"
    );
    for marker in [
        "genesis/health-profile-evidence-bundle-v0.2",
        "semanticInputsSha256",
        "executionEnvironmentIdentitySha256",
        "contentIdentitySha256",
        "declaredEnvironment",
    ] {
        assert!(
            helper.contains(marker),
            "closed manifest marker missing: {marker}"
        );
    }
    for marker in [
        "GENESIS_AGENT_PARITY_GAUNTLET_PROFILE=\"$PROFILE\"",
        "GENESIS_AGENT_PARITY_REUSE_REPORTS=0",
        "GENESIS_AGENT_PARITY_REUSE_NATIVE_REPORT=1",
        "\"$NATIVE_REPORT\" \\",
        "\"$NATIVE_HISTORY\" \\",
        "\"$GAUNTLET_REPORT\" \\",
        "\"$GAUNTLET_HISTORY\" \\",
        "\"$WASI_REPORT\" \\",
        "\"$WASI_HISTORY\" \\",
    ] {
        assert!(
            bundle.contains(marker),
            "missing exact native reuse: {marker}"
        );
    }
    for marker in [
        "lane_source=\"reused-native-report\"",
        "reusing native gauntlet and running WASI lane",
        "native-report:runtime_profile=",
        "native report reuse failed",
    ] {
        assert!(parity.contains(marker), "missing parity guard: {marker}");
    }
    assert!(
        web.contains("process.env.CARGO_TARGET_DIR ?? path.join(rootDir, \"target\")")
            && web.contains("path.join(cargoTargetDir, \"wasm-bindgen-web\", \"gc_wasm\")"),
        "Web bindings must resolve from the configured Cargo target directory"
    );
    assert!(
        ci.contains("node scripts/wasm_web_smoke.mjs \"$(dirname \"$wasm_js_path\")\""),
        "CI must pass the producer-selected Web binding directory across subprocess scope"
    );
    assert!(
        !measurement.contains("GENESIS_HEALTH_CARGO_GATE_SHARDS"),
        "release-pair measurements must retain the serial Cargo gate default"
    );
    assert!(
        health.contains("GENESIS_AGENT_PARITY_PREBUILT_REPORT='$HEALTH_EVIDENCE_ROOT/agent_workflow_runtime_parity_report.json' GENESIS_AGENT_PARITY_NATIVE_REPORT='$HEALTH_EVIDENCE_ROOT/agent_capability_gauntlet_native_report.json'")
            && health.contains("GENESIS_AGENT_PARITY_GENERATIVE_HISTORY='$HEALTH_EVIDENCE_ROOT/agent_generative_workloads_history.jsonl' bash scripts/check_source_decomposition_tracked_parity.sh"),
        "source-decomposition parity must verify the sealed setup evidence instead of rerunning gauntlets"
    );
    assert!(
        bundle.contains("GENESIS_HOST_BRIDGE_FAULT_RUNS=\"$([[ \"$PROFILE\" == \"release-full\" ]] && echo 3 || echo 1)\"")
            && health.contains("GENESIS_TASK_STRESS_RUNS=3")
            && health.contains("GENESIS_TASK_STRESS_SUITE_BUDGET_MS=240000"),
        "each release pair must retain an odd three-run stress cohort"
    );
    assert!(
        bundle.contains(
            "RUNTIME_BACKEND_BUDGET_MS=360000\nif [[ \"$PROFILE\" == \"release-full\" ]]; then"
        ) && bundle.contains("RUNTIME_BACKEND_BUDGET_MS=600000\nfi")
            && bundle.contains(
                "GENESIS_RUNTIME_BACKEND_MATRIX_BUDGET_MS=\"$RUNTIME_BACKEND_BUDGET_MS\""
            ),
        "the cold release matrix must use its 600s sub-budget without relaxing prepush"
    );
}

fn resign_manifest(root: &Path, manifest: &Path, mutation: &str) {
    let script = format!(
        "import json,pathlib,sys; sys.path.insert(0, str(pathlib.Path({root:?})/'scripts/lib')); \
         import health_profile_evidence as h; p=pathlib.Path({manifest:?}); d=json.loads(p.read_text()); \
         {mutation}; d['contentIdentitySha256']=h.object_identity(d); \
         p.write_text(json.dumps(d,indent=2,sort_keys=True)+'\\n')",
        root = root.as_os_str(),
        manifest = manifest.as_os_str(),
    );
    let output = Command::new("python3")
        .args(["-c", &script])
        .current_dir(root)
        .output()
        .expect("mutate manifest fixture");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn closed_release_evidence_rejects_tamper_staleness_environment_and_unknown_fields() {
    let root = repo_root();
    let temp = tempfile::tempdir().expect("create evidence fixture");
    write_fixture_bundle(temp.path());
    let output_root = temp.path().to_str().expect("UTF-8 fixture path");
    let build = run_helper(
        &root,
        &[
            "build",
            "--root",
            ".",
            "--profile",
            "release-full",
            "--output-root",
            output_root,
        ],
    );
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let manifest = temp.path().join("manifest.json");
    let manifest_arg = manifest.to_str().expect("UTF-8 manifest path");
    let gauntlet = temp.path().join("agent_capability_gauntlet_report.json");
    let parity = temp
        .path()
        .join("agent_workflow_runtime_parity_report.json");
    let verify_args = || {
        vec![
            "verify",
            "--root",
            ".",
            "--manifest",
            manifest_arg,
            "--consumer",
            "slo-report-contracts",
            "--script",
            "scripts/check_slo_report_contracts.sh",
            gauntlet.to_str().unwrap(),
            parity.to_str().unwrap(),
        ]
    };
    assert!(run_helper(&root, &verify_args()).status.success());

    let original_manifest = fs::read(&manifest).expect("read original manifest");
    let original_gauntlet = fs::read(&gauntlet).expect("read original artifact");
    fs::write(&gauntlet, b"tampered\n").expect("tamper artifact");
    let tampered = run_helper(&root, &verify_args());
    assert!(!tampered.status.success());
    assert!(String::from_utf8_lossy(&tampered.stderr).contains("artifact identity mismatch"));
    fs::write(&gauntlet, original_gauntlet).expect("restore artifact");

    resign_manifest(
        &root,
        &manifest,
        "d['generatedAtUtc']='2000-01-01T00:00:00+00:00'; d['expiresAtUtc']='2000-01-01T06:00:00+00:00'",
    );
    let stale = run_helper(&root, &verify_args());
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("manifest is stale"));

    fs::write(&manifest, &original_manifest).expect("restore manifest");
    resign_manifest(
        &root,
        &manifest,
        "d['expiresAtUtc']='2099-01-01T00:00:00+00:00'",
    );
    let extended = run_helper(&root, &verify_args());
    assert!(!extended.status.success());
    assert!(String::from_utf8_lossy(&extended.stderr).contains("freshness window mismatch"));

    fs::write(&manifest, &original_manifest).expect("restore manifest");
    resign_manifest(
        &root,
        &manifest,
        "d['producerEnvironment']['profile']='prepush-standard'",
    );
    let producer_profile = run_helper(&root, &verify_args());
    assert!(!producer_profile.status.success());
    assert!(
        String::from_utf8_lossy(&producer_profile.stderr)
            .contains("producer environment profile does not match")
    );

    fs::write(&manifest, &original_manifest).expect("restore manifest");
    resign_manifest(
        &root,
        &manifest,
        "d['executionEnvironment']['architecture']='forged-arch'",
    );
    let environment = run_helper(&root, &verify_args());
    assert!(!environment.status.success());
    assert!(String::from_utf8_lossy(&environment.stderr).contains("environment identity mismatch"));

    fs::write(&manifest, &original_manifest).expect("restore manifest");
    resign_manifest(&root, &manifest, "d['unknownField']=True");
    let unknown = run_helper(&root, &verify_args());
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("manifest fields mismatch"));
}

#[test]
fn release_fanout_auth_rejects_cross_run_token_producer_and_open_fields() {
    let root = repo_root();
    let temp = tempfile::tempdir().expect("create fanout fixture");
    write_fixture_bundle(temp.path());
    let output_root = temp.path().to_str().expect("UTF-8 fixture path");
    let build = run_helper(
        &root,
        &[
            "build",
            "--root",
            ".",
            "--profile",
            "release-full",
            "--output-root",
            output_root,
        ],
    );
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let script = r#"
import copy, datetime, json, pathlib, sys
root, bundle = map(pathlib.Path, sys.argv[1:])
sys.path.insert(0, str(root / 'scripts/lib'))
import release_evidence_fanout as f

request = f.urllib.request.Request(
    'https://api.github.com/repos/example/project/actions/artifacts/7/zip',
    headers={'Authorization': 'Bearer secret'},
)
handler = f.CredentialStrippingRedirect()
same_origin = handler.redirect_request(
    request, None, 302, 'Found', {}, 'https://api.github.com/signed-artifact'
)
cross_origin = handler.redirect_request(
    request, None, 302, 'Found', {}, 'https://results-receiver.actions.githubusercontent.com/signed-artifact'
)
assert same_origin.get_header('Authorization') == 'Bearer secret'
assert cross_origin.get_header('Authorization') is None

manifest = json.loads((bundle / 'manifest.json').read_text())
producer_environment = manifest['executionEnvironment']
consumer_environment = copy.deepcopy(producer_environment)
consumer_environment['operatingSystemRelease'] += '-distinct-hosted-runner'
consumer_core = {
    key: value for key, value in consumer_environment.items()
    if key != 'identitySha256'
}
consumer_environment['identitySha256'] = f.sha256_bytes(f.canonical(consumer_core))
assert f.compatible_execution_environment(producer_environment, consumer_environment)
for field, value in (
    ('profile', 'release-standard'),
    ('architecture', 'incompatible-architecture'),
    ('operatingSystem', 'incompatible-operating-system'),
    ('toolchains', []),
):
    incompatible = copy.deepcopy(consumer_environment)
    incompatible[field] = value
    incompatible_core = {
        key: item for key, item in incompatible.items()
        if key != 'identitySha256'
    }
    incompatible['identitySha256'] = f.sha256_bytes(f.canonical(incompatible_core))
    assert not f.compatible_execution_environment(producer_environment, incompatible)
forged = copy.deepcopy(consumer_environment)
forged['identitySha256'] = 'f' * 64
try:
    f.compatible_execution_environment(producer_environment, forged)
except f.FanoutError as exc:
    assert 'identity mismatch' in str(exc)
else:
    raise SystemExit('accepted forged fanout execution environment identity')
context = {
    'repository': 'corbensorenson/genesisCode',
    'runAttempt': '1',
    'runId': '42',
    'sha': manifest['source']['gitCommit'],
}
token = 'd' * 64
auth = {
    'artifact': {
        'createdAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'digestSha256': token,
        'id': 7,
        'name': f.artifact_name(context),
    },
    'contentIdentitySha256': '',
    'github': context,
    'kind': f.KIND,
    'producer': {
        'bundleIdentitySha256': manifest['contentIdentitySha256'],
        'dagIdentitySha256': f.dag_identity(root),
        'evidenceClass': 'cache-sensitive',
        'index': 1,
        'manifestSha256': f.sha256_file(bundle / 'manifest.json'),
    },
    'version': f.VERSION,
}
auth['contentIdentitySha256'] = f.identity(auth)
auth_path = bundle.parent / 'fanout-auth.json'

def check(candidate, supplied_token, expected):
    candidate['contentIdentitySha256'] = f.identity(candidate)
    auth_path.write_text(json.dumps(candidate, sort_keys=True) + '\n')
    try:
        f.validate_auth(root, bundle, auth_path, supplied_token, {
            'GITHUB_REPOSITORY': context['repository'],
            'GITHUB_RUN_ATTEMPT': context['runAttempt'],
            'GITHUB_RUN_ID': context['runId'],
            'GITHUB_SHA': context['sha'],
        })
    except f.FanoutError as exc:
        if expected not in str(exc):
            raise
    else:
        raise SystemExit(f'accepted invalid fanout auth: {expected}')

auth_path.write_text(json.dumps(auth, sort_keys=True) + '\n')
f.validate_auth(root, bundle, auth_path, token, {
    'GITHUB_REPOSITORY': context['repository'],
    'GITHUB_RUN_ATTEMPT': context['runAttempt'],
    'GITHUB_RUN_ID': context['runId'],
    'GITHUB_SHA': context['sha'],
})
for mutate, expected in (
    (lambda d: d['github'].__setitem__('runId', '43'), 'another workflow run'),
    (lambda d: d['producer'].__setitem__('index', 2), 'producer binding mismatch'),
    (lambda d: d['artifact'].__setitem__('digestSha256', 'e' * 64), 'token or identity mismatch'),
    (lambda d: d.__setitem__('verdict', 'pass'), 'fields mismatch'),
):
    candidate = copy.deepcopy(auth)
    mutate(candidate)
    check(candidate, token, expected)
"#;
    let output = Command::new("python3")
        .args(["-c", script])
        .arg(&root)
        .arg(temp.path())
        .current_dir(&root)
        .output()
        .expect("run fanout authentication controls");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn unsupported_product_cannot_execute_or_relabel_a_synthetic_target_adapter() {
    let root = repo_root();
    let temp = tempfile::tempdir().expect("create target fixture");
    let report = temp.path().join("report.json");
    let artifacts = temp.path().join("artifacts");
    let marker = temp.path().join("runtime-command-executed");
    let (
        target,
        runner_label,
        runtime_command_env,
        runtime_class_env,
        runtime_identity_env,
        sdk_identity_env,
        runtime_class,
        command_fingerprint,
    ) = match std::env::consts::OS {
        "macos" => (
            "ios",
            "macos-15",
            "GENESIS_GCPM_IOS_RUNTIME_CMD",
            "GENESIS_GCPM_IOS_RUNTIME_CLASS",
            "GENESIS_GCPM_IOS_RUNTIME_IDENTITY",
            "GENESIS_GCPM_IOS_SDK_IDENTITY",
            "emulator",
            "xcrun simctl",
        ),
        "linux" => (
            "edge",
            "ubuntu-24.04",
            "GENESIS_GCPM_EDGE_RUNTIME_CMD",
            "GENESIS_GCPM_EDGE_RUNTIME_CLASS",
            "GENESIS_GCPM_EDGE_RUNTIME_IDENTITY",
            "GENESIS_GCPM_EDGE_SDK_IDENTITY",
            "host-runtime",
            "wasmtime run",
        ),
        platform => panic!("no named release-reference shard for test host: {platform}"),
    };
    let command = format!("touch {} # {command_fingerprint}", marker.display());
    let git_sha = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&root)
            .output()
            .expect("read source commit")
            .stdout,
    )
    .expect("UTF-8 source commit")
    .trim()
    .to_owned();
    let output = isolated_bash()
        .arg(root.join("scripts/render_gcpm_target_runtime_pipelines_report.sh"))
        .arg(&report)
        .arg(&artifacts)
        .env("GENESIS_GCPM_TARGET_RUNTIME_TARGETS", target)
        .env("GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_NON_SYNTHETIC", "1")
        .env("GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_REFERENCE_SETUP", "1")
        .env(
            "GENESIS_GCPM_TARGET_RUNTIME_EXPECT_OUTCOME",
            "unsupported-product",
        )
        .env(runtime_command_env, command)
        .env(runtime_class_env, runtime_class)
        .env(runtime_identity_env, "fixture-runtime-identity")
        .env(sdk_identity_env, "fixture-sdk-identity")
        .env("GENESIS_CARGO_CACHE_ROOT", temp.path().join("cargo-cache"))
        .env("CI", "true")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_RUN_ID", "123")
        .env("GITHUB_SHA", &git_sha)
        .env("GENESIS_GCPM_TARGET_RUNTIME_RUNNER_LABEL", runner_label)
        .current_dir(&root)
        .output()
        .expect("run strict target classification");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !marker.exists(),
        "unsupported product executed a configured runtime command"
    );
    let doc: Value = serde_json::from_slice(&fs::read(&report).expect("read target report"))
        .expect("parse target report");
    assert_eq!(doc["ok"], true);
    assert_eq!(doc["release_qualified"], false);
    assert_eq!(
        doc["qualification_statuses"],
        json!(["unsupported-product"])
    );
    assert_eq!(
        doc["targets"][0]["qualification"]["status"],
        "unsupported-product"
    );
    assert_eq!(
        doc["targets"][0]["runtime_evidence"]["lifecycle"],
        Value::Null
    );
    assert_eq!(doc["targets"][0]["runtime_evidence"]["ok"], false);
    assert_eq!(doc["runner_evidence"]["label"], runner_label);
    assert_eq!(doc["runner_evidence"]["matches_reference"], true);
    assert_eq!(doc["runner_evidence"]["ci"], true);

    let validate_report = |path: &Path| {
        Command::new("python3")
            .args([
                "-c",
                "import pathlib,sys; sys.path.insert(0,str(pathlib.Path(sys.argv[1])/'scripts/lib')); import release_full_measurement as m; root=pathlib.Path(sys.argv[1]); policy,digest=m.reference_set(root); m.validate_target_report(root,pathlib.Path(sys.argv[2]),policy,digest,sys.argv[3])",
            ])
            .arg(&root)
            .arg(path)
            .arg(&git_sha)
            .current_dir(&root)
            .output()
            .expect("validate named target report")
    };
    let validated = validate_report(&report);
    assert!(
        validated.status.success(),
        "{}",
        String::from_utf8_lossy(&validated.stderr)
    );
    let mut tampered_doc = doc.clone();
    tampered_doc["targets"][0]["qualification"]["reason"] =
        Value::String("forged product narrative".to_owned());
    let tampered_report = temp.path().join("tampered-report.json");
    fs::write(
        &tampered_report,
        serde_json::to_vec(&tampered_doc).expect("serialize tampered target report"),
    )
    .expect("write tampered target report");
    let tampered = validate_report(&tampered_report);
    assert!(!tampered.status.success());
    assert!(
        String::from_utf8_lossy(&tampered.stderr).contains("policy or product binding mismatch")
    );

    let missing_setup_report = temp.path().join("missing-setup.json");
    let missing_setup = isolated_bash()
        .arg(root.join("scripts/render_gcpm_target_runtime_pipelines_report.sh"))
        .arg(&missing_setup_report)
        .arg(temp.path().join("missing-setup-artifacts"))
        .env("GENESIS_GCPM_TARGET_RUNTIME_TARGETS", target)
        .env("GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_NON_SYNTHETIC", "1")
        .env("GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_REFERENCE_SETUP", "1")
        .env(
            "GENESIS_GCPM_TARGET_RUNTIME_EXPECT_OUTCOME",
            "unsupported-product",
        )
        .env("GENESIS_CARGO_CACHE_ROOT", temp.path().join("cargo-cache"))
        .env("CI", "true")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_RUN_ID", "123")
        .env("GITHUB_SHA", &git_sha)
        .env("GENESIS_GCPM_TARGET_RUNTIME_RUNNER_LABEL", runner_label)
        .current_dir(&root)
        .output()
        .expect("run missing reference setup control");
    assert!(!missing_setup.status.success());
    let missing_setup_doc: Value = serde_json::from_slice(
        &fs::read(&missing_setup_report).expect("read missing setup report"),
    )
    .expect("parse missing setup report");
    assert_eq!(
        missing_setup_doc["targets"][0]["qualification"]["status"],
        "setup-required"
    );

    let mismatch = isolated_bash()
        .arg(root.join("scripts/render_gcpm_target_runtime_pipelines_report.sh"))
        .arg(temp.path().join("mismatch.json"))
        .arg(temp.path().join("mismatch-artifacts"))
        .env("GENESIS_GCPM_TARGET_RUNTIME_TARGETS", target)
        .env("GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_NON_SYNTHETIC", "1")
        .env("GENESIS_GCPM_TARGET_RUNTIME_EXPECT_OUTCOME", "qualified")
        .env("GENESIS_CARGO_CACHE_ROOT", temp.path().join("cargo-cache"))
        .current_dir(&root)
        .output()
        .expect("run expected-outcome mismatch");
    assert!(!mismatch.status.success());
    assert!(
        String::from_utf8_lossy(&mismatch.stderr)
            .contains("configured expected outcome disagrees with reference target set")
    );
}

#[test]
fn health_check_retains_only_explicitly_contained_private_measurement_output() {
    let root = repo_root();
    let containment = tempfile::tempdir().expect("create health output containment");
    let output = containment.path().join("run-output");
    fs::create_dir(&output).expect("create empty run output");
    let run = isolated_bash()
        .arg(root.join("scripts/check_upgrade_plan_health.sh"))
        .args(["--profile", "dev-fast"])
        .env("GENESIS_HEALTH_PROFILE", "dev-fast")
        .env("GENESIS_GATE_TELEMETRY_DISABLE", "1")
        .env("GENESIS_HEALTH_ENFORCE_GATES", "1")
        .env("GENESIS_HEALTH_REQUIRE_GPU_DEVICE_CONFORMANCE", "0")
        .env("GENESIS_HEALTH_TEST_GATE_OVERRIDE", "true")
        .env("GENESIS_CHECK_HEALTH_OUTPUT_ROOT", &output)
        .env(
            "GENESIS_CARGO_CACHE_ROOT",
            containment.path().join("cargo-cache"),
        )
        .env(
            "GENESIS_CHECK_HEALTH_OUTPUT_CONTAINMENT_ROOT",
            containment.path(),
        )
        .current_dir(&root)
        .output()
        .expect("run retained private health check");
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(output.join("profile-report.json").is_file());

    let nested_parent = containment.path().join("nested");
    let nested = nested_parent.join("escaped");
    fs::create_dir_all(&nested).expect("create nested output");
    let rejected = isolated_bash()
        .arg(root.join("scripts/check_upgrade_plan_health.sh"))
        .args(["--profile", "release-full"])
        .env("GENESIS_GATE_TELEMETRY_DISABLE", "1")
        .env("GENESIS_CHECK_HEALTH_OUTPUT_ROOT", &nested)
        .env(
            "GENESIS_CARGO_CACHE_ROOT",
            containment.path().join("cargo-cache"),
        )
        .env(
            "GENESIS_CHECK_HEALTH_OUTPUT_CONTAINMENT_ROOT",
            containment.path(),
        )
        .current_dir(&root)
        .output()
        .expect("run invalid retained health check");
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr)
            .contains("private output must be a direct containment child")
    );
}

#[test]
fn release_measurement_contract_rejects_cache_relabel_cleanup_and_false_qualification() {
    let root = repo_root();
    let script = r#"
import copy, datetime, pathlib, sys
sys.path.insert(0, str(pathlib.Path(sys.argv[1]) / 'scripts/lib'))
import release_full_measurement as m

def run(index, cls):
    prefix = f'runs/pair-{index:02d}-{cls}'
    return {
        'agentGpuProfile': 'agent-gpu-fallback',
        'artifactAttributionBytes': {
            'isolated-run': 1000,
            'node-modules': 100,
            'workspace-build': 0,
            'workspace-target': 0,
        },
        'artifactPeakBytes': 1100,
        'cacheRootStartedEmpty': cls == 'cold',
        'class': cls,
        'exitCode': 0,
        'index': index,
        'logArtifacts': [f'{prefix}/stdout.log', f'{prefix}/stderr.log'],
        'peakRssBytes': 4096,
        'profileElapsedMs': 100,
        'profileReportArtifact': f'{prefix}/profile-report.json',
        'profileReportSha256': 'a' * 64,
        'telemetryElapsedMs': 120,
    }

runs = [run(i, cls) for i in (1, 2) for cls in ('cold', 'warm')]
history = {
    'coldP95ArtifactBytes': 1100,
    'coldP95PeakRssBytes': 4096,
    'coldP95WallMs': 120,
    'samplesPerClass': 2,
    'warmP95ArtifactBytes': 1100,
    'warmP95PeakRssBytes': 4096,
    'warmP95WallMs': 120,
}

report = {
    'artifacts': [],
    'budgets': {'maxArtifactBytes': m.ARTIFACT_BUDGET_BYTES, 'maxWallMs': m.WALL_BUDGET_MS, 'minimumPairs': 2},
    'cleanupRecovery': [
        {'method': 'owned-ephemeral-root-removal', 'ok': True, 'pair': i, 'recoveredBytes': 1000, 'remainingBytes': 0}
        for i in (1, 2)
    ],
    'contentIdentitySha256': '',
    'executionEnvironment': {},
    'generatedAtUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'history': history,
    'kind': m.KIND,
    'ok': True,
    'pairWorkers': [
        {
            'cacheIsolationIdentitySha256': f'{i}' * 64,
            'executionEnvironmentSha256': m.sha256_bytes(m.canonical({})),
            'githubJob': 'release_full_measurement_pair',
            'githubRunAttempt': '1',
            'githubRunId': '123',
            'githubSha': 'c' * 40,
            'pair': i,
            'workerContentIdentitySha256': 'd' * 64,
            'workerManifestArtifact': f'workers/pair-{i:02d}.json',
            'workerManifestSha256': 'e' * 64,
        }
        for i in (1, 2)
    ],
    'pairs': 2,
    'productReleaseQualified': False,
    'profileOperational': True,
    'readinessStatus': 'unsupported-product',
    'runs': runs,
    'source': {'gitCommit': 'c' * 40},
    'targetReadiness': [
        {'expectedOutcome': 'unsupported-product', 'githubRunAttempt': '1', 'githubRunId': '123', 'githubSha': 'c' * 40, 'releaseQualified': False, 'reportArtifact': f'target-readiness/{target}.json', 'reportSha256': 'b' * 64, 'runner': 'ubuntu-24.04' if target != 'ios' else 'macos-15', 'target': target}
        for target in m.TARGETS
    ],
    'version': m.VERSION,
}
report['contentIdentitySha256'] = m.identity(report)
m.validate_report(report)

for mutation, expected in (
    (lambda d: d['runs'][1].update(cacheRootStartedEmpty=True), 'warm run cache identity is false'),
    (lambda d: d['cleanupRecovery'][0].update(ok=False), 'cleanup recovery is incomplete'),
    (lambda d: d['pairWorkers'][1].update(cacheIsolationIdentitySha256=d['pairWorkers'][0]['cacheIsolationIdentitySha256']), 'reused a cache-isolation identity'),
    (lambda d: d['targetReadiness'][0].update(releaseQualified=True), 'unsupported-product was relabeled'),
    (lambda d: d.update(productReleaseQualified=True), 'confused profile operation with product qualification'),
):
    candidate = copy.deepcopy(report)
    mutation(candidate)
    candidate['contentIdentitySha256'] = m.identity(candidate)
    try:
        m.validate_report(candidate)
    except m.MeasurementError as exc:
        if expected not in str(exc):
            raise
    else:
        raise SystemExit(f'accepted adversarial measurement: {expected}')
"#;
    let output = Command::new("python3")
        .args(["-c", script])
        .arg(&root)
        .current_dir(&root)
        .output()
        .expect("run release measurement contract controls");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn release_evidence_v02_partition_is_closed_and_adversarially_checked() {
    let root = repo_root();
    let policy = fs::read_to_string(root.join("policies/release_evidence_dag_v0.2.json"))
        .expect("read release evidence DAG policy");
    let schema = fs::read_to_string(root.join("docs/spec/RELEASE_EVIDENCE_DAG_v0.2.schema.json"))
        .expect("read release evidence DAG schema");
    let health = fs::read_to_string(root.join("scripts/render_upgrade_plan_health_report.sh"))
        .expect("read release health runner");
    let execution = fs::read_to_string(root.join("scripts/lib/release_evidence_execution.py"))
        .expect("read release evidence execution runner");
    let fanout = fs::read_to_string(root.join("scripts/lib/release_evidence_fanout.py"))
        .expect("read release evidence fanout runner");
    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("read release evidence workflow");
    for marker in [
        "genesis/release-evidence-dag-v0.2",
        "independent-matched-cohorts",
        "independent-odd-cohort",
        "producer-authored-verdict",
        "crossRunReuseAllowed",
    ] {
        assert!(
            policy.contains(marker),
            "missing DAG policy marker: {marker}"
        );
    }
    for marker in [
        "release-evidence-dag-v0.2.schema.json",
        "measuredWorkerWallMs",
        "oddSamplesRequired",
        "agent-gpu-strict",
    ] {
        assert!(
            schema.contains(marker),
            "missing DAG schema marker: {marker}"
        );
    }
    for marker in [
        "GENESIS_RELEASE_EVIDENCE_NODE_CLASS",
        "GENESIS_RELEASE_EVIDENCE_INPUT_ROOT",
        "GENESIS_RELEASE_EVIDENCE_EXPORT_ROOT",
        "GENESIS_RELEASE_EVIDENCE_FANOUT_TOKEN",
        "release_evidence_partial",
        "release_evidence_command_ids_sha256",
        "GENESIS_RELEASE_EVIDENCE_PHASE",
        "release_evidence_fanout.py",
    ] {
        assert!(
            health.contains(marker),
            "missing DAG runner marker: {marker}"
        );
    }
    for marker in [
        "genesis/release-evidence-worker-observation-v0.2",
        "genesis/release-evidence-aggregate-v0.2",
        "commandCoverageExact",
        "dependency_mirror.prove_network_denial(prefix, require_loopback=True)",
        "genesis/release-evidence-worker-start-v0.2",
        "require_initialized_output",
        "fanout consumer does not bind the cold-1 producer",
        "release aggregate has a missing or duplicate execution node",
        "precondition.update(",
        "artifactInventoryAtMeasuredStartSha256\": measured_inventory",
    ] {
        assert!(
            execution.contains(marker),
            "missing v0.2 execution control: {marker}"
        );
    }
    for marker in [
        "genesis/release-evidence-fanout-auth-v0.2",
        "same-run cold-1 fanout artifact",
        "fanout archive path is unsafe or duplicated",
        "another workflow run, attempt, or revision",
    ] {
        assert!(
            fanout.contains(marker),
            "missing v0.2 fanout control: {marker}"
        );
    }
    for marker in [
        "release_evidence_cold_worker:",
        "release_evidence_warm_worker:",
        "release_evidence_invariant_worker:",
        "release_evidence_stress_worker:",
        "index: [1, 2, 3]",
        "Publish Same-Run Cold-1 Fanout",
        "Aggregate Release Evidence DAG",
        "initialize-worker",
        "orchestration/fanout.stderr.log",
        "Install Aggregate Rust",
        "Install Aggregate Node",
    ] {
        assert!(
            workflow.contains(marker),
            "missing v0.2 workflow topology: {marker}"
        );
    }
    let measured_boundary = execution
        .find("artifactInventoryAtMeasuredStartSha256\": measured_inventory")
        .expect("warm measured-boundary inventory capture");
    let setup_start = execution[measured_boundary..]
        .find("setup = run_phase(")
        .expect("measured setup after warm-boundary capture");
    assert!(
        setup_start > 0,
        "warm inventory must be resampled before measured setup"
    );

    for action in ["check", "self-test"] {
        let output = Command::new("python3")
            .arg(root.join("scripts/lib/release_evidence_dag.py"))
            .arg("--root")
            .arg(&root)
            .arg(action)
            .current_dir(&root)
            .output()
            .expect("run release evidence DAG validation");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let execution_self_test = Command::new("python3")
        .arg(root.join("scripts/lib/release_evidence_execution.py"))
        .arg("--root")
        .arg(&root)
        .arg("self-test")
        .current_dir(&root)
        .output()
        .expect("run release evidence execution self-test");
    assert!(
        execution_self_test.status.success(),
        "{}",
        String::from_utf8_lossy(&execution_self_test.stderr)
    );
}
