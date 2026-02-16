use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

fn hash_bytes_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn write_caps(dir: &Path, remote: &str, remote_allow: &str) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        format!(
            r#"
allow = [
  "core/store::has",
  "core/store::get"
]

[store]
dir = "./.genesis/store"
remote = "{remote}"
remote_allow = ["{remote_allow}"]
"#
        ),
    )
    .unwrap();
    caps
}

fn put_remote_artifact(remote_dir: &Path, hex: &str, bytes: &[u8]) {
    let v1 = remote_dir.join("v1");
    let store = v1.join("store");
    fs::create_dir_all(&store).unwrap();
    let p = store.join(hex);
    fs::write(&p, bytes).unwrap();
}

#[test]
fn store_get_and_has_can_read_through_to_remote_registry() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let remote_dir = dir.join("remote-registry");
    fs::create_dir_all(&remote_dir).unwrap();
    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");

    let art = gc_coreform::parse_term("{:x 1 :y \"hi\"}").unwrap();
    let bytes = gc_coreform::print_term(&art).into_bytes();
    let hex = hash_bytes_hex(&bytes);
    put_remote_artifact(&remote_dir, &hex, &bytes);

    let caps = write_caps(dir, &remote, &remote_allow);

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["has"])
        .arg(&hex)
        .assert()
        .success()
        .stdout("true\n");

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["get"])
        .arg(&hex)
        .assert()
        .success()
        .stdout("{:x 1 :y \"hi\"}\n");

    let local = dir.join(".genesis").join("store").join(&hex);
    assert!(local.exists());
    let local_bytes = fs::read(local).unwrap();
    assert_eq!(hash_bytes_hex(&local_bytes), hex);
}

