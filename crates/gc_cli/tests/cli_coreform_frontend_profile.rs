use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::path::Path;
use tempfile::tempdir;

fn write_caps(dir: &Path) -> std::path::PathBuf {
    let caps = dir.join("caps.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/refs::list"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"
"#,
    )
    .unwrap();
    caps
}

#[test]
fn default_profile_rejects_rust_coreform_frontend_for_semantic_groups() {
    let td = tempdir().unwrap();
    let caps = write_caps(td.path());
    let zero = "0".repeat(64);

    let commands: Vec<Vec<String>> = vec![
        vec![
            "pkg".into(),
            "--caps".into(),
            caps.display().to_string(),
            "list".into(),
            "--lock".into(),
            "genesis.lock".into(),
        ],
        vec![
            "refs".into(),
            "--caps".into(),
            caps.display().to_string(),
            "list".into(),
        ],
        vec![
            "sync".into(),
            "--caps".into(),
            caps.display().to_string(),
            "pull".into(),
            "--remote".into(),
            "file://dummy".into(),
        ],
        vec![
            "gc".into(),
            "--caps".into(),
            caps.display().to_string(),
            "plan".into(),
            "--lock".into(),
            "genesis.lock".into(),
            "--pins".into(),
            "pins.toml".into(),
        ],
        vec![
            "vcs".into(),
            "--caps".into(),
            caps.display().to_string(),
            "log".into(),
            zero,
            "--max".into(),
            "1".into(),
        ],
    ];

    for args in commands {
        cargo_bin_cmd!("genesis")
            .arg("--coreform-frontend")
            .arg("rust")
            .args(args)
            .assert()
            .failure()
            .code(50)
            .stderr(predicate::str::contains(
                "dedicated parity harness binaries",
            ));
    }
}

#[test]
fn compat_opt_in_allows_rust_coreform_frontend_for_refs_group() {
    let td = tempdir().unwrap();
    let caps = write_caps(td.path());

    cargo_bin_cmd!("genesis_parity")
        .args([
            "--coreform-frontend",
            "rust",
            "refs",
            "--caps",
            caps.to_str().unwrap(),
            "list",
        ])
        .assert()
        .success();
}
