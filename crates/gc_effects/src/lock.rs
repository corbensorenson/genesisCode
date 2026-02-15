use std::fs::OpenOptions;
use std::path::Path;

use crate::error::EffectsError;

#[cfg(target_os = "wasi")]
use std::path::PathBuf;

#[cfg(not(target_os = "wasi"))]
use fs2::FileExt;

#[derive(Debug)]
pub struct ExclusiveLock {
    #[cfg(not(target_os = "wasi"))]
    file: std::fs::File,

    // On WASI we use a best-effort lockfile created via O_EXCL semantics.
    // This is not as strong as OS advisory locks, but it is portable and sufficient
    // for single-host toolchain coordination under WASI.
    #[cfg(target_os = "wasi")]
    lock_path: PathBuf,
    #[cfg(target_os = "wasi")]
    _file: std::fs::File,
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
            Ok(Self { file: f })
        }

        #[cfg(target_os = "wasi")]
        {
            // Create a lockfile atomically. If it exists, wait a bounded time.
            // No ambient timestamps are used: if a process crashes, the user may need
            // to remove the lockfile manually.
            use std::io::ErrorKind;
            use std::time::Duration;

            const RETRIES: u32 = 1_000; // ~10s at 10ms
            for _ in 0..RETRIES {
                match OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(lock_path)
                {
                    Ok(f) => {
                        return Ok(Self {
                            lock_path: lock_path.to_path_buf(),
                            _file: f,
                        });
                    }
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                        std::thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            Err(EffectsError::Log(format!(
                "lock busy: {}",
                lock_path.display()
            )))
        }
    }
}

impl Drop for ExclusiveLock {
    fn drop(&mut self) {
        #[cfg(not(target_os = "wasi"))]
        {
            let _ = self.file.unlock();
        }

        #[cfg(target_os = "wasi")]
        {
            let _ = std::fs::remove_file(&self.lock_path);
        }
    }
}
