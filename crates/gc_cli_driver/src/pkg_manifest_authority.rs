use std::path::{Path, PathBuf};

use gc_pkg::PackageManifest;

pub(crate) fn load(cli: &crate::Cli, path: &Path) -> Result<(PackageManifest, PathBuf), String> {
    let frontend = crate::resolved_coreform_frontend(cli).map_err(|error| error.json.message)?;
    load_with_frontend(path, &frontend)
}

pub(crate) fn load_with_frontend(
    path: &Path,
    frontend: &gc_obligations::CoreformFrontend,
) -> Result<(PackageManifest, PathBuf), String> {
    gc_obligations::load_package_manifest_with_frontend(path, frontend)
        .map_err(|error| error.to_string())
}
