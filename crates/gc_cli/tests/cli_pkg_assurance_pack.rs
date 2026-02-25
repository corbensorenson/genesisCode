use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term, print_term};

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(&caps, "allow = []\n").unwrap();
    caps
}

fn put_store_term(dir: &Path, src: &str) -> String {
    let term = parse_term(src).unwrap();
    let canonical = print_term(&term) + "\n";
    let hash = blake3::hash(canonical.as_bytes()).to_hex().to_string();
    let store_dir = dir.join(".genesis").join("store");
    fs::create_dir_all(&store_dir).unwrap();
    fs::write(store_dir.join(&hash), canonical.as_bytes()).unwrap();
    hash
}

fn write_run_manifest(
    dir: &Path,
    test_id: &str,
    profile: &str,
    snapshot_h: &str,
    policy_h: &str,
    artifact_h: &str,
) -> String {
    put_store_term(
        dir,
        &format!(
            r#"
{{
  :kind "genesis/qualification-test-run-manifest-v0.1"
  :test-id "{test_id}"
  :artifact "{artifact_h}"
  :result :pass
  :profile "{profile}"
  :run-id "lineage-pack-01"
  :runner "gcpm-assurance-pack-tests"
  :release {{
    :commit nil
    :snapshot "{snapshot_h}"
    :policy "{policy_h}"
  }}
}}
"#
        ),
    )
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
    snapshot_h: &str,
    policy_h: &str,
    run_manifest_h: &str,
    tool_path: &Path,
    out: &str,
) {
    let test_artifact = format!("selfhost-boundary={run_manifest_h}");
    let tool = format!("genesis={}", tool_path.display());
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--json", "gcpm", "--caps"])
        .arg(caps)
        .args([
            "qualify",
            "--snapshot",
            snapshot_h,
            "--policy",
            policy_h,
            "--profile",
            "dal-a",
        ])
        .args(["--requirement", "TQ-1", "--test-artifact"])
        .arg(&test_artifact)
        .args(["--tool"])
        .arg(&tool)
        .args(["--out", out, "--no-store"])
        .assert()
        .success();
}

fn canonical_file_hash(path: &Path) -> String {
    let src = fs::read_to_string(path).unwrap();
    let term = parse_term(&src).unwrap();
    let canonical = print_term(&term) + "\n";
    blake3::hash(canonical.as_bytes()).to_hex().to_string()
}

fn write_object_equivalence(
    dir: &Path,
    trace_h: &str,
    qualification_h: &str,
    out: &str,
) -> PathBuf {
    let out_path = dir.join(out);
    fs::write(
        &out_path,
        format!(
            r#"
{{
  :kind "genesis/object-equivalence-v0.1"
  :ok true
  :trace-artifact "{trace_h}"
  :qualification-artifact "{qualification_h}"
  :source-artifact "{source_artifact}"
  :object-artifact "{object_artifact}"
  :method :repro-build
}}
"#,
            source_artifact = "c".repeat(64),
            object_artifact = "d".repeat(64),
        ),
    )
    .unwrap();
    out_path
}

fn write_independent_verifier_run(
    dir: &Path,
    profile: &str,
    trace_h: &str,
    qualification_h: &str,
    object_equivalence_h: &str,
    out: &str,
) -> PathBuf {
    let out_path = dir.join(out);
    fs::write(
        &out_path,
        format!(
            r#"
{{
  :kind "genesis/independent-verifier-run-v0.1"
  :ok true
  :assurance-profile "{profile}"
  :trace-artifact "{trace_h}"
  :qualification-artifact "{qualification_h}"
  :object-equivalence-artifact "{object_equivalence_h}"
  :run-id "ivv-01"
  :runner "independent-qa-lane"
  :roles [:development :verification]
  :result :pass
}}
"#,
        ),
    )
    .unwrap();
    out_path
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
    let test_artifact_h = put_store_term(
        dir,
        r#"
{
  :kind "genesis/unit-tests-v0.2"
  :ok true
  :package "mini"
  :tests []
}
"#,
    );
    let run_manifest_h = write_run_manifest(
        dir,
        "selfhost-boundary",
        "dal-a",
        &snapshot_h,
        &policy_h,
        &test_artifact_h,
    );

    emit_trace(dir, &caps, &snapshot_h, &policy_h, "trace.gc");
    emit_qualification(
        dir,
        &caps,
        &snapshot_h,
        &policy_h,
        &run_manifest_h,
        &tool_path,
        "qualification.gc",
    );
    let trace_h = canonical_file_hash(&dir.join("trace.gc"));
    let qualification_h = canonical_file_hash(&dir.join("qualification.gc"));
    let object_equivalence =
        write_object_equivalence(dir, &trace_h, &qualification_h, "object_equivalence.gc");
    let object_equivalence_h = canonical_file_hash(&object_equivalence);
    let _independent_run = write_independent_verifier_run(
        dir,
        ":do178c-dal-a",
        &trace_h,
        &qualification_h,
        &object_equivalence_h,
        "independent_run.gc",
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
            "--object-equivalence",
            "object_equivalence.gc",
            "--independence-attestation",
            "development:verification@qa-team",
            "--independent-verifier-run",
            "independent_run.gc",
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
    let bindings = m
        .get(&TermOrdKey(Term::symbol(":external-control-bindings")))
        .expect("assurance pack :external-control-bindings");
    let Term::Map(bindings_m) = bindings else {
        panic!(":external-control-bindings must be map");
    };
    assert_eq!(
        bindings_m.get(&TermOrdKey(Term::symbol(":contract"))),
        Some(&Term::Str(
            "genesis/assurance-external-control-bindings-v0.1".to_string()
        ))
    );
    assert_eq!(
        bindings_m.get(&TermOrdKey(Term::symbol(":assurance-profile"))),
        Some(&Term::symbol(":do178c-dal-a"))
    );
    let Term::Vector(objective_bindings) = bindings_m
        .get(&TermOrdKey(Term::symbol(":objective-bindings")))
        .expect(":objective-bindings")
    else {
        panic!(":objective-bindings must be vector");
    };
    assert!(
        !objective_bindings.is_empty(),
        "expected objective bindings in external control map"
    );
    let Term::Map(release_artifacts) = bindings_m
        .get(&TermOrdKey(Term::symbol(":release-artifacts")))
        .expect(":release-artifacts")
    else {
        panic!(":release-artifacts must be map");
    };
    assert_eq!(
        release_artifacts.get(&TermOrdKey(Term::symbol(":trace-artifact"))),
        Some(&Term::Str(trace_h.clone()))
    );
    assert_eq!(
        release_artifacts.get(&TermOrdKey(Term::symbol(":qualification-artifact"))),
        Some(&Term::Str(qualification_h.clone()))
    );
    let Term::Int(unresolved_open_count) = bindings_m
        .get(&TermOrdKey(Term::symbol(":unresolved-open-count")))
        .expect(":unresolved-open-count")
    else {
        panic!(":unresolved-open-count must be int");
    };
    assert_eq!(
        unresolved_open_count.to_string(),
        "0",
        "do178c-dal-a control closures should currently have no open unresolved controls"
    );

    assert!(bundle_dir.join("assurance_pack.gc").exists());
    assert!(bundle_dir.join("requirements_trace.gc").exists());
    assert!(bundle_dir.join("tool_qualification.gc").exists());
    assert!(bundle_dir.join("object_equivalence.gc").exists());
    assert!(bundle_dir.join("bundle_manifest.gc").exists());
    assert!(bundle_dir.join("coverage").exists());
    assert!(bundle_dir.join("independent_verifier").exists());

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
            "--object-equivalence",
            "object_equivalence.gc",
            "--independence-attestation",
            "development:verification@qa-team",
            "--independent-verifier-run",
            "independent_run.gc",
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
    let test_artifact_h = put_store_term(
        dir,
        r#"
{
  :kind "genesis/unit-tests-v0.2"
  :ok true
  :package "mini"
  :tests []
}
"#,
    );
    let run_manifest_h = write_run_manifest(
        dir,
        "selfhost-boundary",
        "dal-a",
        &snapshot_h,
        &policy_h,
        &test_artifact_h,
    );

    emit_trace(dir, &caps, &snapshot_h, &policy_h, "trace.gc");
    emit_qualification(
        dir,
        &caps,
        &snapshot_h,
        &policy_h,
        &run_manifest_h,
        &tool_path,
        "qualification.gc",
    );
    let trace_h = canonical_file_hash(&dir.join("trace.gc"));
    let qualification_h = canonical_file_hash(&dir.join("qualification.gc"));
    let object_equivalence =
        write_object_equivalence(dir, &trace_h, &qualification_h, "object_equivalence.gc");
    let object_equivalence_h = canonical_file_hash(&object_equivalence);
    let _independent_run = write_independent_verifier_run(
        dir,
        ":do178c-dal-a",
        &trace_h,
        &qualification_h,
        &object_equivalence_h,
        "independent_run.gc",
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
            "--object-equivalence",
            "object_equivalence.gc",
            "--independence-attestation",
            "development:verification@qa-team",
            "--independent-verifier-run",
            "independent_run.gc",
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

#[test]
fn gcpm_assurance_pack_rejects_missing_object_equivalence_for_regulated_profile() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    write_minimal_package(dir);

    let tool_path = dir.join("genesis_tool.bin");
    fs::write(&tool_path, b"genesis-toolchain-binary").unwrap();

    let snapshot_h = "a".repeat(64);
    let policy_h = "b".repeat(64);
    let test_artifact_h = put_store_term(
        dir,
        r#"
{
  :kind "genesis/unit-tests-v0.2"
  :ok true
  :package "mini"
  :tests []
}
"#,
    );
    let run_manifest_h = write_run_manifest(
        dir,
        "selfhost-boundary",
        "dal-a",
        &snapshot_h,
        &policy_h,
        &test_artifact_h,
    );

    emit_trace(dir, &caps, &snapshot_h, &policy_h, "trace.gc");
    emit_qualification(
        dir,
        &caps,
        &snapshot_h,
        &policy_h,
        &run_manifest_h,
        &tool_path,
        "qualification.gc",
    );
    fs::write(
        dir.join("coverage_mcdc.gc"),
        "{ :kind \"genesis/coverage-v0.2\" :profile :mcdc :ok true }\n",
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
            "coverage_mcdc.gc",
            "--independence-attestation",
            "development:verification@qa-team",
            "--out",
            "assurance_pack.gc",
            "--no-store",
        ])
        .assert()
        .code(10)
        .stdout(predicates::str::contains(
            "requires --object-equivalence evidence",
        ));
}
