use assert_cmd::cargo::cargo_bin_cmd;

fn stdout_str(args: &[&str]) -> String {
    let out = cargo_bin_cmd!("genesis")
        .args(args)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap()
}

#[test]
fn cli_help_surface_contains_expected_command_groups() {
    let s = stdout_str(&["--help"]);
    for needle in [
        "fmt",
        "eval",
        "explain",
        "run",
        "replay",
        "test",
        "pack",
        "selfhost-artifact",
        "keygen",
        "sign",
        "verify",
        "transparency-verify",
        "typecheck",
        "optimize",
        "apply-patch",
        "store",
        "refs",
        "pkg",
        "sync",
        "gc",
        "vcs",
        "--json",
        "--step-limit",
        "--no-step-limit",
        "--selfhost-artifact",
        "--selfhost-bootstrap",
        "--selfhost-only",
    ] {
        assert!(
            s.contains(needle),
            "top-level --help output missing {needle}"
        );
    }
}

#[test]
fn cli_help_surface_contains_recent_spec_alignment_flags() {
    let s = stdout_str(&["fmt", "--help"]);
    assert!(s.contains("--engine"), "fmt --help output missing --engine");

    let s = stdout_str(&["eval", "--help"]);
    assert!(
        s.contains("--engine"),
        "eval --help output missing --engine"
    );
    assert!(
        s.contains("--stage1-pipeline"),
        "eval --help output missing --stage1-pipeline"
    );
    assert!(
        s.contains("--stage1-gate"),
        "eval --help output missing --stage1-gate"
    );
    assert!(
        s.contains("--stage2-gate"),
        "eval --help output missing --stage2-gate"
    );

    let s = stdout_str(&["explain", "--help"]);
    assert!(
        s.contains("--engine"),
        "explain --help output missing --engine"
    );

    let s = stdout_str(&["run", "--help"]);
    assert!(s.contains("--engine"), "run --help output missing --engine");

    let s = stdout_str(&["replay", "--help"]);
    assert!(
        s.contains("--engine"),
        "replay --help output missing --engine"
    );

    let s = stdout_str(&["optimize", "--help"]);
    assert!(
        s.contains("--engine"),
        "optimize --help output missing --engine"
    );
    assert!(
        s.contains("--stage1-gate"),
        "optimize --help output missing --stage1-gate"
    );
    assert!(
        s.contains("--stage2-gate"),
        "optimize --help output missing --stage2-gate"
    );
    assert!(
        s.contains("--emit-wasm"),
        "optimize --help output missing --emit-wasm"
    );

    let s = stdout_str(&["selfhost-artifact", "--help"]);
    assert!(
        s.contains("--out"),
        "selfhost-artifact --help output missing --out"
    );
    assert!(
        s.contains("--min-stage2-supported-modules"),
        "selfhost-artifact --help output missing --min-stage2-supported-modules"
    );
    assert!(
        s.contains("--min-stage2-validated-modules"),
        "selfhost-artifact --help output missing --min-stage2-validated-modules"
    );

    let s = stdout_str(&["pkg", "--help"]);
    assert!(
        s.contains("import"),
        "pkg --help output missing import subcommand"
    );

    let s = stdout_str(&["pkg", "import", "--help"]);
    assert!(
        s.contains("--set-ref"),
        "pkg import --help output missing --set-ref"
    );

    let s = stdout_str(&["vcs", "--help"]);
    assert!(
        s.contains("merge3"),
        "vcs --help output missing merge3 subcommand"
    );

    let s = stdout_str(&["vcs", "merge3", "--help"]);
    assert!(
        s.contains("--out"),
        "vcs merge3 --help output missing --out"
    );
}
