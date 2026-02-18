use std::path::{Path, PathBuf};

pub fn repo_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` here is `.../crates/gc_cli`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

pub fn repo_toolchain_artifact() -> PathBuf {
    repo_root().join("selfhost").join("toolchain.gc")
}

pub fn copy_repo_toolchain_artifact(dst_dir: &Path) -> PathBuf {
    let src = repo_toolchain_artifact();
    assert!(
        src.is_file(),
        "repo toolchain artifact missing at {}",
        src.display()
    );
    let dst = dst_dir.join("selfhost_toolchain.gc");
    std::fs::copy(&src, &dst).expect("copy repo toolchain artifact");
    dst
}
