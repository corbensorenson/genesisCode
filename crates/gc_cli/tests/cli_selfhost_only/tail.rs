use super::*;

#[test]
fn selfhost_only_accepts_pkg_vcs_gc_gpk_wrappers_with_replay_parity() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    std::fs::write(dir.path().join("bad.gpk"), b"not-a-gpk-bundle").unwrap();
    std::fs::write(
        dir.path().join("pins.toml"),
        "version = 1\n\n[pins]\nkeep = []\nkeep_refs = []\nkeep_evidence_for = []\n",
    )
    .unwrap();

    let caps = dir.path().join("caps_wrappers.toml");
    std::fs::write(
        &caps,
        r#"
allow = [
  "core/pkg-low::init",
  "core/vcs-low::log",
  "core/gc-low::plan",
  "core/gpk-low::import"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg-low::init"]
base_dir = "."
create_dirs = true

[op."core/gc-low::plan"]
base_dir = "."
create_dirs = true

[op."core/gpk-low::import"]
base_dir = "."
"#,
    )
    .unwrap();

    let workflows = [
        (
            "pkg_init",
            r#"
              (def prog
                ((((core/pkg::init "genesis.lock") "ws") "policy:default-v0.1") nil))
              prog
            "#,
        ),
        (
            "vcs_log",
            r#"
              (def prog
                ((core/vcs::log nil) 4))
              prog
            "#,
        ),
        (
            "gc_plan",
            r#"
              (def prog
                (((((core/gc::plan "genesis.lock") "pins.toml") 8) true) true))
              prog
            "#,
        ),
        (
            "gpk_import",
            r#"
              (def prog
                (core/gpk::import "bad.gpk"))
              prog
            "#,
        ),
    ];

    for (name, program_src) in workflows {
        let file = dir.path().join(format!("{name}.gc"));
        let log = dir.path().join(format!("{name}.gclog"));
        std::fs::write(&file, program_src).unwrap();

        let run_out = cargo_bin_cmd!("genesis")
            .current_dir(dir.path())
            .args([
                "--selfhost-only",
                "--selfhost-artifact",
                artifact.to_str().unwrap(),
                "--no-step-limit",
                "run",
                file.to_str().unwrap(),
                "--engine",
                "selfhost",
                "--caps",
                caps.to_str().unwrap(),
                "--log",
                log.to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();

        let replay_out = cargo_bin_cmd!("genesis")
            .current_dir(dir.path())
            .args([
                "--selfhost-only",
                "--selfhost-artifact",
                artifact.to_str().unwrap(),
                "--no-step-limit",
                "replay",
                file.to_str().unwrap(),
                "--engine",
                "selfhost",
                "--log",
                log.to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();

        assert_eq!(
            String::from_utf8(run_out).unwrap().trim(),
            String::from_utf8(replay_out).unwrap().trim(),
            "run/replay mismatch for workflow {name}"
        );
    }
}
