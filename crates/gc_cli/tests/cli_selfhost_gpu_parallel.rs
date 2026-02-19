use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};
use predicates::prelude::*;
use tempfile::tempdir;

mod support;

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
        .expect("stdout must contain artifact hash")
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
fn selfhost_only_gpu_parallel_reference_pkg_emits_obligation_evidence() {
    let td = tempdir().unwrap();
    let src = fixture("pkg_gpu_parallel_obligations");
    let dst = td.path().join("pkg_gpu_parallel_obligations");
    copy_dir_all(&src, &dst).unwrap();
    let artifact = support::copy_repo_toolchain_artifact(td.path());

    let pkg = dst.join("package.toml");
    let out = cargo_bin_cmd!("genesis")
        .current_dir(&dst)
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "test",
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_match("[0-9a-f]{64}\\s*").unwrap())
        .get_output()
        .stdout
        .clone();

    let acceptance_h = parse_hash_line(&out);
    let acc = read_store_term(&dst, &acceptance_h);

    let replay_h = acceptance_obligation_artifact(&acc, "core/obligation::replayable-tests")
        .expect("replayable-tests obligation artifact");
    let concurrency_h = acceptance_obligation_artifact(&acc, "core/obligation::concurrency-replay")
        .expect("concurrency-replay obligation artifact");

    let replay_report = read_store_term(&dst, &replay_h);
    let concurrency_report = read_store_term(&dst, &concurrency_h);

    let Term::Map(replay_map) = replay_report else {
        panic!("replayable-tests report must be map");
    };
    assert_eq!(
        replay_map.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );

    let Term::Map(concurrency_map) = concurrency_report else {
        panic!("concurrency-replay report must be map");
    };
    assert_eq!(
        concurrency_map.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    let concurrent_tests = concurrency_map
        .get(&TermOrdKey(Term::symbol(":concurrent-tests")))
        .and_then(|t| match t {
            Term::Int(i) => i.to_string().parse::<u64>().ok(),
            _ => None,
        })
        .expect("concurrency report must include integer :concurrent-tests");
    assert!(concurrent_tests >= 1);
}
