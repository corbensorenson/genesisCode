use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey};

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(&caps, "allow = []\n").unwrap();
    caps
}

fn write_minimal_package(dir: &Path) {
    fs::write(dir.join("lib.gc"), "(def mini::x 1)\nmini::x\n").unwrap();
    fs::write(
        dir.join("package.toml"),
        r#"
name = "mini"
version = "0.1.0"
obligations = ["core/obligation::unit-tests"]
dependencies = []

[[modules]]
path = "lib.gc"
"#,
    )
    .unwrap();
    fs::write(
        dir.join("requirements.gc"),
        r#"
{
  :type :req/graph
  :requirements [{
    :id "SYS-1"
    :level :system
    :parents []
    :hazards []
    :links {
      :modules [{:path "lib.gc" :exports [mini::x]}]
      :obligations [core/obligation::unit-tests]
      :evidence-kinds [:requirements-trace]
    }
  }]
}
"#,
    )
    .unwrap();
}

fn emit_trace(dir: &Path, caps: &Path, snapshot_h: &str, policy_h: &str, out: &str) {
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(caps)
        .args([
            "trace",
            "--pkg",
            "package.toml",
            "--requirements",
            "requirements.gc",
            "--snapshot",
            snapshot_h,
            "--policy",
            policy_h,
            "--out",
            out,
            "--no-store",
        ])
        .assert()
        .success();
}

fn emit_qualification(
    dir: &Path,
    caps: &Path,
    policy_h: &str,
    test_artifact_h: &str,
    tool_path: &Path,
    out: &str,
) {
    let test_artifact = format!("selfhost-boundary={test_artifact_h}");
    let tool = format!("genesis={}", tool_path.display());
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(caps)
        .args(["qualify", "--policy", policy_h, "--profile", "dal-a"])
        .args(["--requirement", "TQ-1", "--test-artifact"])
        .arg(&test_artifact)
        .args(["--tool"])
        .arg(&tool)
        .args(["--out", out, "--no-store"])
        .assert()
        .success();
}

#[test]
fn gcpm_assurance_pack_emits_deterministic_profile_gated_bundle() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    write_minimal_package(dir);

    let tool_path = dir.join("genesis_tool.bin");
    fs::write(&tool_path, b"genesis-toolchain-binary").unwrap();

    let snapshot_h = "a".repeat(64);
    let policy_h = "b".repeat(64);
    let test_artifact_h = "c".repeat(64);

    emit_trace(dir, &caps, &snapshot_h, &policy_h, "trace.gc");
    emit_qualification(
        dir,
        &caps,
        &policy_h,
        &test_artifact_h,
        &tool_path,
        "qualification.gc",
    );

    fs::write(
        dir.join("coverage_mcdc.gc"),
        "{ :kind \"genesis/coverage-v0.2\" :profile :mcdc :ok true }\n",
    )
    .unwrap();

    let out_path = dir.join("assurance_pack.gc");
    let bundle_dir = dir.join("assurance_bundle");

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "assurance-pack",
            "--pkg",
            "package.toml",
            "--assurance-profile",
            "do178c-dal-a",
            "--snapshot",
            &snapshot_h,
            "--policy",
            &policy_h,
            "--trace",
            "trace.gc",
            "--qualification",
            "qualification.gc",
            "--coverage",
            "coverage_mcdc.gc",
            "--independence-attestation",
            "development:verification@qa-team",
            "--bundle-dir",
        ])
        .arg(&bundle_dir)
        .args(["--out", "assurance_pack.gc", "--no-store"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-assurance-pack-v0.1")
    );

    let pack_src_1 = fs::read_to_string(&out_path).unwrap();
    let pack_t = gc_coreform::parse_term(&pack_src_1).unwrap();
    let Term::Map(m) = pack_t else {
        panic!("assurance pack artifact must be map");
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":assurance-pack"))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":target-profile"))),
        Some(&Term::symbol(":do178c-dal-a"))
    );
    let Term::Vector(coverage) = m
        .get(&TermOrdKey(Term::symbol(":coverage-exports")))
        .expect("assurance pack :coverage-exports")
    else {
        panic!("assurance pack :coverage-exports must be vector");
    };
    assert_eq!(coverage.len(), 1);
    let Term::Vector(attestations) = m
        .get(&TermOrdKey(Term::symbol(":independence-attestations")))
        .expect("assurance pack :independence-attestations")
    else {
        panic!("assurance pack :independence-attestations must be vector");
    };
    assert_eq!(attestations.len(), 1);

    assert!(bundle_dir.join("assurance_pack.gc").exists());
    assert!(bundle_dir.join("requirements_trace.gc").exists());
    assert!(bundle_dir.join("tool_qualification.gc").exists());
    assert!(bundle_dir.join("bundle_manifest.gc").exists());
    assert!(bundle_dir.join("coverage").exists());

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "assurance-pack",
            "--pkg",
            "package.toml",
            "--assurance-profile",
            "do178c-dal-a",
            "--snapshot",
            &snapshot_h,
            "--policy",
            &policy_h,
            "--trace",
            "trace.gc",
            "--qualification",
            "qualification.gc",
            "--coverage",
            "coverage_mcdc.gc",
            "--independence-attestation",
            "development:verification@qa-team",
            "--bundle-dir",
        ])
        .arg(&bundle_dir)
        .args(["--out", "assurance_pack.gc", "--no-store"])
        .assert()
        .success();
    let pack_src_2 = fs::read_to_string(&out_path).unwrap();
    assert_eq!(pack_src_1, pack_src_2);
}

#[test]
fn gcpm_assurance_pack_rejects_insufficient_profile_coverage() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    write_minimal_package(dir);

    let tool_path = dir.join("genesis_tool.bin");
    fs::write(&tool_path, b"genesis-toolchain-binary").unwrap();

    let snapshot_h = "a".repeat(64);
    let policy_h = "b".repeat(64);
    let test_artifact_h = "c".repeat(64);

    emit_trace(dir, &caps, &snapshot_h, &policy_h, "trace.gc");
    emit_qualification(
        dir,
        &caps,
        &policy_h,
        &test_artifact_h,
        &tool_path,
        "qualification.gc",
    );

    fs::write(
        dir.join("coverage_decision.gc"),
        "{ :kind \"genesis/coverage-v0.2\" :profile :decision :ok true }\n",
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "assurance-pack",
            "--pkg",
            "package.toml",
            "--assurance-profile",
            "do178c-dal-a",
            "--snapshot",
            &snapshot_h,
            "--policy",
            &policy_h,
            "--trace",
            "trace.gc",
            "--qualification",
            "qualification.gc",
            "--coverage",
            "coverage_decision.gc",
            "--independence-attestation",
            "development:verification@qa-team",
            "--out",
            "assurance_pack.gc",
            "--no-store",
        ])
        .assert()
        .code(10)
        .stdout(predicates::str::contains(
            "requires minimum coverage rank 3",
        ));
}
