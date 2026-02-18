use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        if from.file_name().is_some_and(|n| n == ".genesis") {
            continue;
        }
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to)?;
        } else if ty.is_file() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/spec")
        .join(path)
}

fn parse_acceptance_hash(stdout: &[u8]) -> String {
    let s = String::from_utf8_lossy(stdout);
    s.lines()
        .find_map(|l| {
            let t = l.trim();
            if t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()) {
                Some(t.to_string())
            } else {
                None
            }
        })
        .expect("expected acceptance artifact hash on stdout")
}

fn read_acceptance(pkg_dir: &Path, hex: &str) -> gc_coreform::Term {
    let p = pkg_dir.join(".genesis").join("store").join(hex);
    let s = fs::read_to_string(&p).unwrap();
    gc_coreform::parse_term(&s).unwrap()
}

fn acceptance_ok(t: &gc_coreform::Term) -> bool {
    use gc_coreform::{Term, TermOrdKey};
    let Term::Map(m) = t else {
        panic!("acceptance must be map")
    };
    match m.get(&TermOrdKey(Term::symbol(":ok"))) {
        Some(Term::Bool(b)) => *b,
        _ => panic!("acceptance missing :ok"),
    }
}

fn acceptance_has_obligation(t: &gc_coreform::Term, name: &str) -> bool {
    use gc_coreform::{Term, TermOrdKey};
    let Term::Map(m) = t else { return false };
    let Term::Vector(obs) = m
        .get(&TermOrdKey(Term::symbol(":obligations")))
        .expect("acceptance missing :obligations")
    else {
        panic!(":obligations must be vector");
    };
    obs.iter().any(|o| match o {
        Term::Map(om) => matches!(
            om.get(&TermOrdKey(Term::symbol(":name"))),
            Some(Term::Symbol(s)) if s == name
        ),
        _ => false,
    })
}

#[test]
fn unit_tests_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_unit");
    let dst = td.path().join("pkg_fail_unit");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");

    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::unit-tests"
    ));
}

#[test]
fn determinism_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_determinism");
    let dst = td.path().join("pkg_fail_determinism");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");

    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::determinism"
    ));
}

#[test]
fn capabilities_declared_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_caps_declared");
    let dst = td.path().join("pkg_fail_caps_declared");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");

    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::capabilities-declared"
    ));
}

#[test]
fn typecheck_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_typecheck");
    let dst = td.path().join("pkg_fail_typecheck");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");

    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::typecheck"
    ));
}

#[test]
fn hash_mismatch_is_captured_as_preflight_acceptance() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_hash");
    let dst = td.path().join("pkg_fail_hash");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");

    let assert = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30);

    // Ensure we didn't regress to "no artifact on failure" for preflight checks.
    let out = assert.get_output().stdout.clone();
    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::preflight"
    ));

    // Also ensure stderr indicates failure, but avoid relying on exact wording.
    // Stderr is intentionally not part of the stable interface for expected obligation failures.
}

#[test]
fn package_policy_rejects_no_step_limit_by_default() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_limits_policy");
    let dst = td.path().join("pkg_limits_policy");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");

    cargo_bin_cmd!("genesis")
        .arg("--no-step-limit")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(10);
}

#[test]
fn budgets_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_budgets");
    let dst = td.path().join("pkg_fail_budgets");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(&acc, "core/obligation::budgets"));
}

#[test]
fn property_tests_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_property_tests");
    let dst = td.path().join("pkg_fail_property_tests");
    copy_dir_all(&src, &dst).unwrap();

    // Pin module hashes first (fixtures must be packable).
    let pkg = dst.join("package.toml");
    cargo_bin_cmd!("genesis")
        .args(["pack", "--pkg"])
        .arg(&pkg)
        .assert()
        .success();

    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::property-tests"
    ));
}

#[test]
fn coverage_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_coverage");
    let dst = td.path().join("pkg_fail_coverage");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(&acc, "core/obligation::coverage"));
}

#[test]
fn gfx_golden_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_gfx_golden");
    let dst = td.path().join("pkg_fail_gfx_golden");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::gfx-golden-images"
    ));
}

#[test]
fn gfx_frame_budget_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_gfx_frame_budget");
    let dst = td.path().join("pkg_fail_gfx_frame_budget");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::gfx-frame-budgets"
    ));
}

#[test]
fn gfx_api_stability_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_gfx_api");
    let dst = td.path().join("pkg_fail_gfx_api");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::gfx-api-stability"
    ));
}

#[test]
fn gfx_pixel_golden_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_gfx_pixel_golden");
    let dst = td.path().join("pkg_fail_gfx_pixel_golden");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::gfx-golden-images"
    ));
}

#[test]
fn lint_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_lint");
    let dst = td.path().join("pkg_fail_lint");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(&acc, "core/obligation::lint"));
}

#[test]
fn ai_style_failure_is_recorded_in_acceptance_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_ai_style");
    let dst = td.path().join("pkg_fail_ai_style");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let hex = parse_acceptance_hash(&out);
    let acc = read_acceptance(&dst, &hex);
    assert!(!acceptance_ok(&acc));
    assert!(acceptance_has_obligation(
        &acc,
        "core/obligation::ai-style"
    ));
}
