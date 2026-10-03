use super::*;

impl FsRoot {
    /// Acquire one bounded, exclusive spool in this held directory. It has no
    /// reopenable pathname on Unix and is deleted by handle lifetime on Windows.
    pub(crate) fn scratch_file(&self) -> io::Result<std::fs::File> {
        #[cfg(not(any(unix, windows)))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "owned scratch files unavailable",
        ));
        #[cfg(any(unix, windows))]
        {
            let mut options = OpenOptions::new();
            options
                .read(true)
                .write(true)
                .create_new(true)
                .follow(FollowSymlinks::No);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            #[cfg(windows)]
            {
                use cap_std::fs::OpenOptionsExt;
                // FILE_FLAG_DELETE_ON_CLOSE; exclusive sharing keeps the spool private.
                // https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew
                options.custom_flags(0x04000000).share_mode(0);
            }
            for sequence in 0..1024u32 {
                let name = format!(
                    ".genesis-import.{}.{}.tmp",
                    crate::platform_process_id(),
                    sequence
                );
                let file = match self.directory.open_with(&name, &options) {
                    Ok(file) => file,
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                };
                #[cfg(unix)]
                if let Err(error) = self.directory.remove_file_or_symlink(&name) {
                    drop(file);
                    let _ = self.directory.remove_file_or_symlink(&name);
                    return Err(error);
                }
                return Ok(file.into_std());
            }
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "scratch file slots exhausted",
            ))
        }
    }
}
