use super::*;

impl FsRoot {
    /// The open descriptor owns an unlinked spool; no ambient name is retained.
    pub(crate) fn scratch_file(&self) -> io::Result<std::fs::File> {
        for sequence in 0..1024u32 {
            let name = format!(
                ".genesis-import.{}.{}.tmp",
                crate::platform_process_id(),
                sequence
            );
            let descriptor = match fs::openat(
                &self.directory,
                &name,
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(descriptor) => descriptor,
                Err(error) if error == rustix::io::Errno::EXIST => continue,
                Err(error) => return Err(error.into()),
            };
            if let Err(error) = fs::unlinkat(&self.directory, &name, AtFlags::empty()) {
                drop(descriptor);
                let _ = fs::unlinkat(&self.directory, &name, AtFlags::empty());
                return Err(error.into());
            }
            return Ok(descriptor.into());
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "scratch file slots exhausted",
        ))
    }
}
