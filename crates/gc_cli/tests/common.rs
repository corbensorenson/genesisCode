use std::path::{Path, PathBuf};

pub fn workspace_root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    // crates/gc_cli -> crates -> workspace root
    here.ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

pub fn repo_selfhost_toolchain_artifact() -> PathBuf {
    workspace_root().join("selfhost").join("toolchain.gc")
}

pub fn copy_repo_selfhost_toolchain_artifact(dir: &Path) -> PathBuf {
    let src = repo_selfhost_toolchain_artifact();
    assert!(
        src.is_file(),
        "missing toolchain artifact at {}",
        src.display()
    );
    let dst = dir.join("selfhost_toolchain.gc");
    std::fs::copy(&src, &dst).expect("copy toolchain artifact");
    dst
}
