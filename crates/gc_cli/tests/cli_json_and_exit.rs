use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};

mod support;

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/spec")
        .join(path)
}

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

fn map_get<'a>(m: &'a std::collections::BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(k)))
}

#[test]
fn test_json_output_is_valid_and_exit_code_is_stable() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_basic");
    let dst = td.path().join("pkg_basic");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let caps = dst.join("caps.toml");

    let out = cargo_bin_cmd!("genesis")
        .args(["--json", "test", "--pkg"])
        .arg(&pkg)
        .args(["--caps"])
        .arg(&caps)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/test-v0.2")
    );
    assert_eq!(
        v.get("diagnostics_schema").and_then(|x| x.as_str()),
        Some("genesis/diagnostics-schema-v1")
    );
    assert_eq!(
        v.get("diagnostics")
            .and_then(|x| x.as_array())
            .map(|xs| xs.len()),
        Some(0)
    );
    let acc = v
        .get("data")
        .and_then(|d| d.get("acceptance_artifact"))
        .and_then(|x| x.as_str())
        .expect("acceptance_artifact");
    assert_eq!(acc.len(), 64);
}

#[test]
fn optimize_json_includes_egg_stats() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let artifact = support::copy_repo_toolchain_artifact(dir);

    let prog = dir.join("prog.gc");
    fs::write(
        &prog,
        r#"
          (def id (fn (x) (prim int/add x 0)))
          (id 5)
        "#,
    )
    .unwrap();

    let out = cargo_bin_cmd!("genesis")
        .args(["--selfhost-artifact"])
        .arg(&artifact)
        .args(["--json", "optimize"])
        .arg(&prog)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(true));
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/optimize-v0.2")
    );
    let data = v.get("data").expect("data");
    assert_eq!(
        data.get("coreform_frontend")
            .and_then(|x| x.get("name"))
            .and_then(|x| x.as_str()),
        Some("selfhost")
    );
    assert!(
        data.get("changed")
            .and_then(|x| x.as_bool())
            .unwrap_or(false)
    );
    assert_eq!(
        data.get("original_hash")
            .and_then(|x| x.as_str())
            .unwrap()
            .len(),
        64
    );
    assert_eq!(
        data.get("optimized_hash")
            .and_then(|x| x.as_str())
            .unwrap()
            .len(),
        64
    );
    assert!(data.get("egg_runs").and_then(|x| x.as_u64()).unwrap() > 0);
    assert!(
        data.get("egg_rewrites_applied")
            .and_then(|x| x.as_object())
            .is_some()
    );
    let opt_src = data
        .get("optimized_coreform")
        .and_then(|x| x.as_str())
        .unwrap();
    assert!(opt_src.contains("(def id"));
    assert!(opt_src.contains("(fn (x) x)"));
}

#[test]
fn translation_validation_artifact_includes_optimizer_summary() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_basic");
    let dst = td.path().join("pkg_basic");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let caps = dst.join("caps.toml");

    let out = cargo_bin_cmd!("genesis")
        .args(["--json", "test", "--pkg"])
        .arg(&pkg)
        .args(["--caps"])
        .arg(&caps)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    let obs = v
        .get("data")
        .and_then(|d| d.get("obligations"))
        .and_then(|x| x.as_array())
        .expect("obligations");
    let tv = obs
        .iter()
        .find(|o| {
            o.get("name").and_then(|x| x.as_str())
                == Some("core/obligation::translation-validation")
        })
        .expect("translation-validation obligation result");
    assert_eq!(tv.get("ok").and_then(|x| x.as_bool()), Some(true));
    let art = tv
        .get("artifact")
        .and_then(|x| x.as_str())
        .expect("translation-validation artifact hash");
    assert_eq!(art.len(), 64);

    let store_path = dst.join(".genesis/store").join(art);
    let bytes = fs::read(&store_path).expect("read translation-validation artifact bytes");
    let s = std::str::from_utf8(&bytes).expect("artifact utf-8");
    let term = parse_term(s).expect("parse artifact term");

    let Term::Map(m) = term else {
        panic!("translation-validation artifact must be a map term");
    };
    assert!(
        matches!(
            map_get(&m, ":kind"),
            Some(Term::Str(k)) if k == "genesis/translation-validation-v0.2"
        ),
        "artifact :kind mismatch"
    );
    let Some(Term::Map(opt)) = map_get(&m, ":optimizer") else {
        panic!("artifact missing :optimizer map");
    };
    for k in [
        ":egg-runs",
        ":egg-iterations",
        ":egg-eclasses",
        ":egg-enodes",
        ":egg-rewrites",
    ] {
        assert!(
            opt.contains_key(&TermOrdKey(Term::symbol(k))),
            "missing {k}"
        );
    }
    let Some(Term::Vector(mods)) = map_get(&m, ":modules") else {
        panic!("artifact missing :modules vector");
    };
    assert!(!mods.is_empty(), ":modules should be non-empty");
    let Some(Term::Map(stage2)) = map_get(&m, ":stage2") else {
        panic!("artifact missing :stage2 map");
    };
    assert!(
        stage2.contains_key(&TermOrdKey(Term::symbol(":supported-modules"))),
        "stage2 summary missing :supported-modules"
    );
    assert!(
        stage2.contains_key(&TermOrdKey(Term::symbol(":validated-modules"))),
        "stage2 summary missing :validated-modules"
    );
    assert!(
        stage2.contains_key(&TermOrdKey(Term::symbol(":entries"))),
        "stage2 summary missing :entries"
    );
}

#[test]
fn run_denied_capability_has_exit_code_41() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let artifact = support::copy_repo_toolchain_artifact(dir);

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
    fs::write(&caps, r#"allow = []"#).unwrap();

    let log = dir.join("out.gclog");

    cargo_bin_cmd!("genesis")
        .args(["--selfhost-artifact"])
        .arg(&artifact)
        .args(["run"])
        .arg(&prog)
        .args(["--caps"])
        .arg(&caps)
        .args(["--log"])
        .arg(&log)
        .assert()
        .failure()
        .code(41);

    assert!(log.exists(), "run should still emit a deterministic log");
}

#[test]
fn fmt_check_failure_has_exit_code_11() {
    let td = tempfile::tempdir().unwrap();
    let artifact = support::copy_repo_toolchain_artifact(td.path());
    let p = td.path().join("x.gc");
    fs::write(
        &p,
        // not canonical: multi-arg app should be reprinted nested
        r#"
          ((fn (x y) (prim int/add x y)) 1 2)
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args(["--selfhost-artifact"])
        .arg(&artifact)
        .args(["fmt", "--check"])
        .arg(&p)
        .assert()
        .failure()
        .code(11);
}

#[test]
fn manifest_memory_limit_causes_preflight_obligation_exit_code_30() {
    let td = tempfile::tempdir().unwrap();
    let src = fixture("pkg_fail_mem_limits");
    let dst = td.path().join("pkg_fail_mem_limits");
    copy_dir_all(&src, &dst).unwrap();

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .args(["--json", "test", "--pkg"])
        .arg(&pkg)
        .assert()
        .failure()
        .code(30)
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
    assert_eq!(v.get("ok").and_then(|x| x.as_bool()), Some(false));
    assert_eq!(
        v.get("kind").and_then(|x| x.as_str()),
        Some("genesis/test-v0.2")
    );
    assert_eq!(
        v.get("diagnostics_schema").and_then(|x| x.as_str()),
        Some("genesis/diagnostics-schema-v1")
    );
    assert_eq!(
        v.get("diagnostics")
            .and_then(|x| x.as_array())
            .and_then(|xs| xs.first())
            .and_then(|d| d.get("exit_code"))
            .and_then(|x| x.as_u64()),
        Some(30)
    );
    assert_eq!(
        v.get("error")
            .and_then(|error| error.get("code"))
            .and_then(|code| code.as_str()),
        Some("test/error")
    );
    assert_eq!(
        v.pointer("/error/context/facts/acceptance_artifact"),
        v.pointer("/data/acceptance_artifact")
    );
    assert_eq!(
        v.pointer("/error/context/facts/failed_obligations/0/name")
            .and_then(|name| name.as_str()),
        Some("core/obligation::preflight")
    );
    assert_eq!(
        v.pointer("/diagnostics/0/code")
            .and_then(|code| code.as_str()),
        Some("test/error")
    );
    assert_eq!(
        v.get("data")
            .and_then(|d| d.get("obligations"))
            .and_then(|x| x.as_array())
            .and_then(|arr| arr.iter().find(|o| {
                o.get("name").and_then(|x| x.as_str()) == Some("core/obligation::preflight")
            }))
            .and_then(|o| o.get("ok"))
            .and_then(|x| x.as_bool()),
        Some(false)
    );
}
