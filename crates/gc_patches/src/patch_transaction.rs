use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::*;

pub(super) struct PatchWorkspaceRollback {
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
}

impl PatchWorkspaceRollback {
    pub(super) fn capture(
        pkg_toml: &Path,
        pkg_dir: &Path,
        patch: &Patch,
    ) -> Result<Self, PatchError> {
        let mut paths = BTreeSet::new();
        for op in &patch.ops {
            match op {
                PatchOp::ReplaceNode { module_path, .. }
                | PatchOp::ReplaceNodeId { module_path, .. }
                | PatchOp::AddModule { module_path, .. }
                | PatchOp::RemoveModule { module_path }
                | PatchOp::RenameSymbol { module_path, .. }
                | PatchOp::RewriteMetaList { module_path, .. }
                | PatchOp::MigrateContractSignature { module_path, .. } => {
                    paths.insert(pkg_dir.join(module_path));
                }
                PatchOp::MoveModule {
                    from_module_path,
                    to_module_path,
                }
                | PatchOp::SplitModule {
                    from_module_path,
                    to_module_path,
                    ..
                } => {
                    paths.insert(pkg_dir.join(from_module_path));
                    paths.insert(pkg_dir.join(to_module_path));
                }
                PatchOp::UpdateManifest { .. } => {}
            }
        }
        paths.insert(pkg_toml.to_path_buf());

        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let original = match std::fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == ErrorKind::NotFound => None,
                Err(error) => return Err(PatchError::Io(error)),
            };
            files.push((path, original));
        }
        Ok(Self { files })
    }

    pub(super) fn restore(&self) -> Result<(), std::io::Error> {
        for (path, original) in &self.files {
            match original {
                Some(bytes) => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(path, bytes)?;
                }
                None => match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                },
            }
        }
        Ok(())
    }
}

pub(super) fn rollback_error(original: &PatchError, rollback: std::io::Error) -> PatchError {
    PatchError::Validate(format!(
        "patch transaction failed ({original}); rollback failed: {rollback}"
    ))
}
