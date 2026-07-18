use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use crate::error::EffectsError;

#[cfg(not(target_os = "wasi"))]
use fs2::FileExt;

#[derive(Debug)]
pub struct ExclusiveLock {
    #[cfg(not(target_os = "wasi"))]
    file: std::fs::File,

    // The sidecar coordinates native and WASI processes. Native processes also
    // retain an advisory lock, which survives crashes without a stale marker.
    marker_path: PathBuf,
    marker_file: Option<std::fs::File>,
}

impl ExclusiveLock {
    pub fn acquire(lock_path: &Path) -> Result<Self, EffectsError> {
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        #[cfg(not(target_os = "wasi"))]
        {
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(lock_path)?;
            f.lock_exclusive()
                .map_err(|e| EffectsError::Log(format!("lock failed: {e}")))?;
            let (marker_path, marker_file) = acquire_portable_marker(lock_path)?;
            Ok(Self {
                file: f,
                marker_path,
                marker_file: Some(marker_file),
            })
        }

        #[cfg(target_os = "wasi")]
        {
            let (marker_path, marker_file) = acquire_portable_marker(lock_path)?;
            Ok(Self {
                marker_path,
                marker_file: Some(marker_file),
            })
        }
    }
}

fn acquire_portable_marker(lock_path: &Path) -> Result<(PathBuf, std::fs::File), EffectsError> {
    use std::io::ErrorKind;
    use std::time::Duration;

    let mut marker_name = lock_path.as_os_str().to_os_string();
    marker_name.push(".exclusive");
    let marker_path = PathBuf::from(marker_name);

    // O_EXCL is available in WASI Preview 1. The bounded wait avoids hanging
    // forever; a process crash may require removal of the stale sidecar.
    const RETRIES: u32 = 1_000; // ~10s at 10ms
    for _ in 0..RETRIES {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker_path)
        {
            Ok(file) => return Ok((marker_path, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(EffectsError::Log(format!(
        "lock busy: {}",
        lock_path.display()
    )))
}

impl Drop for ExclusiveLock {
    fn drop(&mut self) {
        // WASI Preview 1 may reject unlinking an open file. Close the marker
        // handle before removing its path on every target.
        drop(self.marker_file.take());
        let _ = std::fs::remove_file(&self.marker_path);

        #[cfg(not(target_os = "wasi"))]
        {
            let _ = self.file.unlock();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExclusiveLock;

    #[test]
    fn persistent_advisory_path_does_not_block_reacquisition() {
        let root =
            std::env::temp_dir().join(format!("genesis-effects-lock-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create lock test root");
        let path = root.join("refs.lock");

        drop(ExclusiveLock::acquire(&path).expect("first lock acquisition"));
        assert!(path.exists(), "native advisory lock path should persist");
        assert!(
            !root.join("refs.lock.exclusive").exists(),
            "portable marker must be removed on drop"
        );
        drop(ExclusiveLock::acquire(&path).expect("second lock acquisition"));

        std::fs::remove_file(path).expect("remove advisory lock path");
        std::fs::remove_dir(root).expect("remove lock test root");
    }
}
