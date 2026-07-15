use assert_cmd::cargo::cargo_bin_cmd;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

mod support;

const BUNDLE: &str = "examples/agent_benchmark_reproducibility";
const VERIFIER: &str = "scripts/lib/gc_agent_benchmark_run.py";
const SCORER: &str = "scripts/lib/gc_agent_scoring.py";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn genesis_path() -> PathBuf {
    cargo_bin_cmd!("genesis").get_program().into()
}

fn copy_tree(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_tree(&source_path, &target_path);
        } else {
            fs::copy(&source_path, &target_path).unwrap();
            let permissions = fs::metadata(&source_path).unwrap().permissions();
            fs::set_permissions(&target_path, permissions).unwrap();
        }
    }
}

fn run_verifier(run: &Path) -> std::process::Output {
    Command::new("python3")
        .current_dir(repo_root())
        .args([VERIFIER, "--check", "--run"])
        .arg(run)
        .output()
        .unwrap()
}

fn run_model(bundle: &Path, artifact: &Path, log: &str) -> std::process::Output {
    Command::new(genesis_path())
        .current_dir(bundle)
        .env("GENESIS_SELFHOST_COMPILED_CACHE_DISABLE", "1")
        .args(["--no-step-limit", "--selfhost-artifact"])
        .arg(artifact)
        .args([
            "run",
            "model_effect.gc",
            "--engine",
            "selfhost",
            "--caps",
            "caps.toml",
            "--log",
            log,
        ])
        .output()
        .unwrap()
}

#[test]
fn canonical_run_is_complete_content_addressed_and_scores_with_shipped_binary() {
    let root = repo_root();
    let checked = run_verifier(&root.join(BUNDLE).join("run.json"));
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );

    let temp = tempdir().unwrap();
    let bundle = temp.path().join("bundle");
    copy_tree(&root.join(BUNDLE), &bundle);
    let artifact = support::copy_repo_toolchain_artifact(temp.path());
    let executed = run_model(&bundle, &artifact, "observed.gclog");
    assert!(
        executed.status.success(),
        "{}",
        String::from_utf8_lossy(&executed.stderr)
    );
    let serialized = String::from_utf8(executed.stdout.clone()).unwrap();
    assert!(serialized.contains("genesis-agent-fixture"));
    assert!(serialized.contains("(prim int/add 40 2)"));
    assert!(!serialized.contains(root.to_str().unwrap()));
    assert_eq!(
        fs::read(bundle.join("observed.gclog")).unwrap(),
        fs::read(root.join(BUNDLE).join("model-effect.gclog")).unwrap()
    );

    let score = Command::new("python3")
        .current_dir(&root)
        .args([
            SCORER,
            "--score",
            "--case",
            "generation-small",
            "--candidate",
        ])
        .arg(bundle.join("candidate"))
        .arg("--genesis-bin")
        .arg(genesis_path())
        .arg("--selfhost-artifact")
        .arg(&artifact)
        .output()
        .unwrap();
    assert!(
        score.status.success(),
        "{}",
        String::from_utf8_lossy(&score.stderr)
    );
    assert_eq!(
        score.stdout,
        fs::read(root.join(BUNDLE).join("score.json")).unwrap()
    );
    let report: Value = serde_json::from_slice(&score.stdout).unwrap();
    assert_eq!(report["qualityScoreBasisPoints"], 10_000);
    assert_eq!(report["modelSpecificMetrics"]["present"], false);

    let run: Value = serde_json::from_slice(
        &fs::read(root.join(BUNDLE).join("run.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(run["track"]["trackId"], "open-agent");
    assert_eq!(
        run["track"]["training"]["genesisSpecificTraining"],
        "unknown"
    );
    assert!(run["track"]["hardware"]["classId"].is_null());
    assert_eq!(
        run["track"]["hardware"]["measurementMethod"],
        "not-claimed"
    );
}

#[test]
fn effect_log_replays_without_the_local_model_or_its_weights() {
    let root = repo_root();
    let temp = tempdir().unwrap();
    let bundle = temp.path().join("bundle");
    copy_tree(&root.join(BUNDLE), &bundle);
    let artifact = support::copy_repo_toolchain_artifact(temp.path());
    let executed = run_model(&bundle, &artifact, "observed.gclog");
    assert!(executed.status.success());

    fs::remove_file(bundle.join("tools/local_model_bridge.py")).unwrap();
    fs::remove_file(bundle.join("models/weights.fixture")).unwrap();
    let replayed = Command::new(genesis_path())
        .current_dir(&bundle)
        .env("GENESIS_SELFHOST_COMPILED_CACHE_DISABLE", "1")
        .args(["--no-step-limit", "--selfhost-artifact"])
        .arg(&artifact)
        .args([
            "replay",
            "model_effect.gc",
            "--engine",
            "selfhost",
            "--log",
            "observed.gclog",
        ])
        .output()
        .unwrap();
    assert!(
        replayed.status.success(),
        "{}",
        String::from_utf8_lossy(&replayed.stderr)
    );
    assert_eq!(executed.stdout, replayed.stdout);
}

#[test]
fn verifier_and_effect_boundary_fail_closed_under_tampering() {
    let root = repo_root();
    let temp = tempdir().unwrap();
    let bundle = temp.path().join("bundle");
    copy_tree(&root.join(BUNDLE), &bundle);

    fs::write(bundle.join("candidate/main.gc"), "(prim int/add 41 2)\n").unwrap();
    let tampered = run_verifier(&bundle.join("run.json"));
    assert!(!tampered.status.success());
    assert!(String::from_utf8_lossy(&tampered.stderr).contains("stale artifact facts"));

    fs::remove_dir_all(&bundle).unwrap();
    copy_tree(&root.join(BUNDLE), &bundle);
    let caps = bundle.join("caps.toml");
    let source = fs::read_to_string(&caps).unwrap();
    fs::write(
        &caps,
        source.replace(
            "[op.\"host/plugin::command\"]",
            "[op.\"host/plugin::command\"]\nallow = false",
        ),
    )
    .unwrap();
    let artifact = support::copy_repo_toolchain_artifact(temp.path());
    let denied = run_model(&bundle, &artifact, "denied.gclog");
    assert!(!denied.status.success());
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&denied.stdout),
        String::from_utf8_lossy(&denied.stderr)
    );
    assert!(diagnostic.contains("core/caps/denied"), "{diagnostic}");
    let denied_log = fs::read_to_string(bundle.join("denied.gclog")).unwrap();
    assert!(denied_log.contains(":decision :deny"));
    assert!(denied_log.contains(":op host/plugin::command"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::remove_file(bundle.join("candidate/main.gc")).unwrap();
        symlink("requirements.md", bundle.join("candidate/main.gc")).unwrap();
        let symlinked = run_verifier(&bundle.join("run.json"));
        assert!(!symlinked.status.success());
        assert!(String::from_utf8_lossy(&symlinked.stderr).contains("symlink"));
    }
}
