use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};
use predicates::prelude::*;

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

fn parse_hash_line(stdout: &[u8]) -> String {
    let s = String::from_utf8_lossy(stdout);
    s.lines()
        .map(str::trim)
        .find(|t| t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap()
        .to_string()
}

fn read_store_term(pkg_dir: &Path, hash: &str) -> Term {
    let p = pkg_dir.join(".genesis").join("store").join(hash);
    let s = fs::read_to_string(p).unwrap();
    parse_term(&s).unwrap()
}

fn acceptance_obligation_artifact(acc: &Term, name: &str) -> Option<String> {
    let Term::Map(m) = acc else { return None };
    let Term::Vector(obs) = m.get(&TermOrdKey(Term::symbol(":obligations")))? else {
        return None;
    };
    for o in obs {
        let Term::Map(om) = o else { continue };
        let is_name = matches!(
            om.get(&TermOrdKey(Term::symbol(":name"))),
            Some(Term::Symbol(s)) if s == name
        );
        if !is_name {
            continue;
        }
        if let Some(Term::Str(h)) = om.get(&TermOrdKey(Term::symbol(":artifact"))) {
            return Some(h.clone());
        }
    }
    None
}

#[test]
fn fmt_check_is_idempotent_on_fixture() {
    let td = tempfile::tempdir().unwrap();
    let inp = fixture("pkg_basic/basic.gc");
    let out = td.path().join("basic.gc");
    fs::copy(&inp, &out).unwrap();

    // Fixture sources aren't required to be canonical; ensure `fmt` makes them canonical
    // and `fmt --check` is idempotent after that.
    cargo_bin_cmd!("genesis")
        .args(["fmt"])
        .arg(&out)
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(["fmt", "--check"])
        .arg(&out)
        .assert()
        .success();
}

#[test]
fn test_pkg_basic_obligations_succeed() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_basic");
    let dst = td.path().join("pkg_basic");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let caps = dst.join("caps.toml");

    cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .args(["--caps"])
        .arg(&caps)
        .assert()
        .success()
        .stdout(predicate::str::is_match("[0-9a-f]{64}\\s*").unwrap());
}

#[test]
fn test_pkg_gfx_obligations_succeed() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_gfx_obligations");
    let dst = td.path().join("pkg_gfx_obligations");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");

    cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .success()
        .stdout(predicate::str::is_match("[0-9a-f]{64}\\s*").unwrap());
}

#[test]
fn test_pkg_lint_obligation_succeeds() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_lint");
    let dst = td.path().join("pkg_lint");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");

    cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .success()
        .stdout(predicate::str::is_match("[0-9a-f]{64}\\s*").unwrap());
}

#[test]
fn test_pkg_lint_autofix_emits_patch_artifact() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_lint_autofix");
    let dst = td.path().join("pkg_lint_autofix");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let acceptance_h = parse_hash_line(&out);
    let acc = read_store_term(&dst, &acceptance_h);
    let lint_h = acceptance_obligation_artifact(&acc, "core/obligation::lint")
        .expect("lint obligation must produce artifact");
    let lint_report = read_store_term(&dst, &lint_h);

    let Term::Map(rm) = lint_report else {
        panic!("lint report must be a map")
    };
    let Term::Vector(autofixes) = rm
        .get(&TermOrdKey(Term::symbol(":autofix-patches")))
        .expect("lint report must include :autofix-patches")
    else {
        panic!(":autofix-patches must be vector");
    };
    assert!(!autofixes.is_empty(), "expected at least one autofix patch");
    let Term::Map(first) = &autofixes[0] else {
        panic!("autofix entry must be map")
    };
    let Term::Str(patch_h) = first
        .get(&TermOrdKey(Term::symbol(":patch")))
        .expect("autofix entry must include :patch")
    else {
        panic!(":patch must be string hash");
    };

    // Ensure patch artifact exists and has patch schema shape.
    let patch_term = read_store_term(&dst, patch_h);
    let Term::Map(pm) = patch_term else {
        panic!("patch artifact must be map")
    };
    assert!(pm.contains_key(&TermOrdKey(Term::symbol(":ops"))));
}

#[test]
fn test_pkg_ai_style_obligation_succeeds_and_emits_machine_readable_report() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_ai_style");
    let dst = td.path().join("pkg_ai_style");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let acceptance_h = parse_hash_line(&out);
    let acc = read_store_term(&dst, &acceptance_h);
    let ai_h = acceptance_obligation_artifact(&acc, "core/obligation::ai-style")
        .expect("ai-style obligation must produce artifact");
    let ai_report = read_store_term(&dst, &ai_h);

    let Term::Map(rm) = ai_report else {
        panic!("ai-style report must be a map")
    };
    assert_eq!(
        rm.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::Str("genesis/ai-style-v0.1".to_string()))
    );
    assert_eq!(
        rm.get(&TermOrdKey(Term::symbol(":schema"))),
        Some(&Term::Str("genesis/diagnostics-schema-v1".to_string()))
    );
    let Term::Vector(_diags) = rm
        .get(&TermOrdKey(Term::symbol(":diagnostics")))
        .expect("ai-style report must include :diagnostics")
    else {
        panic!(":diagnostics must be a vector");
    };
    let Term::Vector(errors) = rm
        .get(&TermOrdKey(Term::symbol(":errors")))
        .expect("ai-style report must include :errors")
    else {
        panic!(":errors must be a vector");
    };
    assert_eq!(
        errors.len(),
        0,
        "passing fixture should have no style errors"
    );
}

#[test]
fn pack_is_stable_independent_of_invocation_path() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_basic");
    let dst = td.path().join("pkg_basic");
    copy_dir_all(&src, &dst).unwrap();

    let pkg_abs = dst.join("package.toml");

    let out_abs = cargo_bin_cmd!("genesis")
        .args(["pack", "--pkg"])
        .arg(&pkg_abs)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let h_abs = String::from_utf8(out_abs).unwrap();
    let h_abs = h_abs.trim().to_string();

    let out_rel = cargo_bin_cmd!("genesis")
        .current_dir(&dst)
        .args(["pack", "--pkg"])
        .arg("package.toml")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let h_rel = String::from_utf8(out_rel).unwrap();
    let h_rel = h_rel.trim().to_string();

    assert_eq!(h_abs, h_rel);
}

#[test]
fn run_and_replay_roundtrip_effect_program() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let prog = dir.join("prog.gc");
    fs::write(
        &prog,
        r#"
          (def prog
            (core/effect::perform
              'sys/time::now
              {}
              (fn (t)
                (core/effect::pure t))))
          prog
        "#,
    )
    .unwrap();

    let caps = dir.join("caps.toml");
    fs::write(&caps, r#"allow = ["sys/time::now"]"#).unwrap();

    let log = dir.join("out.gclog");

    let run_out = cargo_bin_cmd!("genesis")
        .args(["run"])
        .arg(&prog)
        .args(["--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let replay_out = cargo_bin_cmd!("genesis")
        .args(["replay"])
        .arg(&prog)
        .args(["--log"])
        .arg(&log)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let run_s = String::from_utf8(run_out).unwrap();
    let replay_s = String::from_utf8(replay_out).unwrap();
    assert_eq!(run_s.trim(), replay_s.trim());
}

#[test]
fn sign_and_verify_with_policy_succeeds() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_basic");
    let dst = td.path().join("pkg_basic");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let caps = dst.join("caps.toml");

    cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .args(["--caps"])
        .arg(&caps)
        .assert()
        .success();

    let key = dst.join("signing_key.toml");
    cargo_bin_cmd!("genesis")
        .args(["keygen", "--out"])
        .arg(&key)
        .assert()
        .success();

    let key_s = fs::read_to_string(&key).unwrap();
    let pk_b64 = key_s
        .lines()
        .find_map(|l| {
            let l = l.trim();
            l.strip_prefix("pk_b64 = \"")
                .and_then(|rest| rest.strip_suffix('\"'))
        })
        .expect("pk_b64 in key file");

    let policy = dst.join("policy.toml");
    fs::write(
        &policy,
        format!("version = 1\nmin_signatures = 1\nallowed_public_keys = [\"{pk_b64}\"]\n"),
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args(["sign", "--pkg"])
        .arg(&pkg)
        .args(["--key"])
        .arg(&key)
        .assert()
        .success()
        .stdout(predicate::str::is_match("[0-9a-f]{64}\\s*").unwrap());

    cargo_bin_cmd!("genesis")
        .args(["verify", "--pkg"])
        .arg(&pkg)
        .args(["--policy"])
        .arg(&policy)
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));

    cargo_bin_cmd!("genesis")
        .args(["transparency-verify", "--pkg"])
        .arg(&pkg)
        .assert()
        .success();
}

#[test]
fn verify_with_policy_fails_when_not_signed() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_basic");
    let dst = td.path().join("pkg_basic");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let caps = dst.join("caps.toml");

    cargo_bin_cmd!("genesis")
        .args(["test", "--pkg"])
        .arg(&pkg)
        .args(["--caps"])
        .arg(&caps)
        .assert()
        .success();

    let key = dst.join("signing_key.toml");
    cargo_bin_cmd!("genesis")
        .args(["keygen", "--out"])
        .arg(&key)
        .assert()
        .success();

    let key_s = fs::read_to_string(&key).unwrap();
    let pk_b64 = key_s
        .lines()
        .find_map(|l| {
            let l = l.trim();
            l.strip_prefix("pk_b64 = \"")
                .and_then(|rest| rest.strip_suffix('\"'))
        })
        .expect("pk_b64 in key file");

    let policy = dst.join("policy.toml");
    fs::write(
        &policy,
        format!("version = 1\nmin_signatures = 1\nallowed_public_keys = [\"{pk_b64}\"]\n"),
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args(["verify", "--pkg"])
        .arg(&pkg)
        .args(["--policy"])
        .arg(&policy)
        .assert()
        .failure()
        .code(50);
}
