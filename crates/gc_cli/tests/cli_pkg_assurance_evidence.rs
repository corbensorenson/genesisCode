use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey};

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(&caps, "allow = []\n").unwrap();
    caps
}

#[test]
fn gcpm_trace_emits_deterministic_requirements_trace_evidence() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
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
    let snapshot_h = "a".repeat(64);
    let policy_h = "b".repeat(64);
    let out_path = dir.join("trace.gc");

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "trace",
            "--pkg",
            "package.toml",
            "--requirements",
            "requirements.gc",
            "--snapshot",
            &snapshot_h,
            "--policy",
            &policy_h,
            "--out",
            "trace.gc",
            "--no-store",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-requirements-trace-v0.1")
    );

    let trace_src_1 = fs::read_to_string(&out_path).unwrap();
    let trace_t = gc_coreform::parse_term(&trace_src_1).unwrap();
    let Term::Map(m) = trace_t else {
        panic!("trace artifact must be map");
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":requirements-trace"))
    );
    let Term::Map(release) = m
        .get(&TermOrdKey(Term::symbol(":release")))
        .expect("trace :release")
    else {
        panic!("trace :release must be map");
    };
    assert_eq!(
        release.get(&TermOrdKey(Term::symbol(":commit"))),
        Some(&Term::Nil)
    );
    assert_eq!(
        release.get(&TermOrdKey(Term::symbol(":snapshot"))),
        Some(&Term::Str(snapshot_h.clone()))
    );

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "trace",
            "--pkg",
            "package.toml",
            "--requirements",
            "requirements.gc",
            "--snapshot",
            &snapshot_h,
            "--policy",
            &policy_h,
            "--out",
            "trace.gc",
            "--no-store",
        ])
        .assert()
        .success();
    let trace_src_2 = fs::read_to_string(&out_path).unwrap();
    assert_eq!(trace_src_1, trace_src_2);
}

#[test]
fn gcpm_qualify_emits_deterministic_tool_qualification_evidence() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    let tool_path = dir.join("genesis_tool.bin");
    let tool_bytes = b"genesis-toolchain-binary";
    fs::write(&tool_path, tool_bytes).unwrap();
    let expected_tool_hash = blake3::hash(tool_bytes).to_hex().to_string();
    let policy_h = "c".repeat(64);
    let test_artifact_h = "d".repeat(64);
    let out_path = dir.join("qualification.gc");

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "qualify",
            "--policy",
            &policy_h,
            "--profile",
            "dal-a",
            "--requirement",
            "TQ-1",
            "--test-artifact",
            &format!("selfhost-boundary={test_artifact_h}"),
            "--tool",
            &format!("genesis={}", tool_path.display()),
            "--out",
            "qualification.gc",
            "--no-store",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-tool-qualification-v0.1")
    );

    let qual_src_1 = fs::read_to_string(&out_path).unwrap();
    let qual_t = gc_coreform::parse_term(&qual_src_1).unwrap();
    let Term::Map(m) = qual_t else {
        panic!("qualification artifact must be map");
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":tool-qualification"))
    );
    let Term::Map(release) = m
        .get(&TermOrdKey(Term::symbol(":release")))
        .expect("qualification :release")
    else {
        panic!("qualification :release must be map");
    };
    assert_eq!(
        release.get(&TermOrdKey(Term::symbol(":commit"))),
        Some(&Term::Nil)
    );
    let Term::Vector(tools) = m
        .get(&TermOrdKey(Term::symbol(":tools")))
        .expect("qualification :tools")
    else {
        panic!("qualification :tools must be vector");
    };
    assert_eq!(tools.len(), 1);
    let Term::Map(tool) = &tools[0] else {
        panic!("qualification :tools[0] must be map");
    };
    assert_eq!(
        tool.get(&TermOrdKey(Term::symbol(":blake3"))),
        Some(&Term::Str(expected_tool_hash))
    );

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(&caps)
        .args([
            "qualify",
            "--policy",
            &policy_h,
            "--profile",
            "dal-a",
            "--requirement",
            "TQ-1",
            "--test-artifact",
            &format!("selfhost-boundary={test_artifact_h}"),
            "--tool",
            &format!("genesis={}", tool_path.display()),
            "--out",
            "qualification.gc",
            "--no-store",
        ])
        .assert()
        .success();
    let qual_src_2 = fs::read_to_string(&out_path).unwrap();
    assert_eq!(qual_src_1, qual_src_2);
}
