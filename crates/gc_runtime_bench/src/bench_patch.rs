use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use gc_kernel::{MemLimits, StepLimit};
use gc_patches::apply_patch_with_step_limit;

use crate::config::BenchConfig;
use crate::measure::best_of;

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst).with_context(|| format!("create {}", dst.display()))?;
    }
    for entry in std::fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_tree(&src_path, &dst_path)?;
        } else if ty.is_file() {
            std::fs::copy(&src_path, &dst_path).with_context(|| {
                format!("copy {} -> {}", src_path.display(), dst_path.display())
            })?;
        }
    }
    Ok(())
}

pub fn run_patch_apply(cfg: &BenchConfig) -> Result<u128> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .context("canonicalize repo root")?;
    let fixture_src = repo_root.join("tests/spec/pkg_basic");
    if !fixture_src.exists() {
        bail!("missing patch benchmark fixture: {}", fixture_src.display());
    }

    let temp = tempfile::tempdir().context("create patch benchmark tempdir")?;
    let fixture = temp.path().join("pkg_basic");
    copy_tree(&fixture_src, &fixture)?;
    let patch = fixture.join("pure.gcpatch");
    let pkg = fixture.join("package.toml");
    let caps = fixture.join("caps.toml");

    best_of(cfg.warmups, cfg.repeats, || {
        let _out = apply_patch_with_step_limit(
            &patch,
            &pkg,
            Some(&caps),
            StepLimit::Default,
            MemLimits::default(),
        )
        .context("apply benchmark patch")?;
        Ok(())
    })
}
