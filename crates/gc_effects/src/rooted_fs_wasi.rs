//! Stable-WASI descriptor-relative filesystem operations. Ancestor links are
//! denied; final entries remain entries for stat, unlink and replacement.
use rustix::fd::OwnedFd;
use rustix::fs::{self, AtFlags, Mode, OFlags};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[path = "rooted_fs/scratch_wasi.rs"]
mod scratch;

pub(crate) struct FsRoot {
    directory: OwnedFd,
    base: PathBuf,
}

pub(crate) struct AtomicWriteTarget {
    root: FsRoot,
    path: PathBuf,
    create: bool,
}

fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn open_directory(fd: &OwnedFd, path: &Path) -> io::Result<OwnedFd> {
    Ok(fs::openat(
        fd,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    )?)
}

fn relative_path(path: &Path) -> io::Result<PathBuf> {
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => relative.push(name),
            Component::CurDir => {}
            _ => return Err(denied("filesystem path escapes capability root")),
        }
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Ok(relative)
}

impl FsRoot {
    pub(crate) fn open(base: &Path) -> io::Result<Self> {
        let directory = fs::open(
            base,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        Ok(Self {
            directory,
            base: base.to_path_buf(),
        })
    }

    fn resolve(&self, input: &str) -> io::Result<PathBuf> {
        crate::runner_io_ops::validate_portable_effect_path(input).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid portable filesystem path",
            )
        })?;
        relative_path(Path::new(input))
    }

    fn parent(&self, path: &Path, create: bool) -> io::Result<(OwnedFd, OsString)> {
        let leaf = path
            .file_name()
            .ok_or_else(|| denied("operation requires an entry below capability root"))?
            .to_owned();
        let mut directory = open_directory(&self.directory, Path::new("."))?;
        let parent = path.parent().unwrap_or(Path::new("."));
        for component in parent.components() {
            let Component::Normal(name) = component else {
                continue;
            };
            match open_directory(&directory, Path::new(name)) {
                Ok(next) => directory = next,
                Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
                    match fs::mkdirat(&directory, name, Mode::RWXU) {
                        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                        Err(error) => return Err(error.into()),
                    }
                    // A coequal writer may replace the new name. Reopen with
                    // NOFOLLOW rather than inheriting authority from mkdir.
                    directory = open_directory(&directory, Path::new(name))?;
                }
                Err(error) => return Err(error),
            }
        }
        Ok((directory, leaf))
    }

    // Examine every existing ancestor before any optional parent creation.
    // Names after the first missing ancestor are already traversal-free.
    fn preflight(&self, path: &Path) -> io::Result<()> {
        let mut directory = open_directory(&self.directory, Path::new("."))?;
        let parent = path.parent().unwrap_or(Path::new("."));
        for component in parent.components() {
            let Component::Normal(name) = component else {
                continue;
            };
            match open_directory(&directory, Path::new(name)) {
                Ok(next) => directory = next,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub(crate) fn prepare_atomic_write(
        self,
        input: &str,
        create: bool,
    ) -> io::Result<AtomicWriteTarget> {
        if input.ends_with('/') || input.ends_with("/.") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "document destination requires a file entry",
            ));
        }
        let input = Path::new(input);
        let input = if input.is_absolute() {
            input
                .strip_prefix(&self.base)
                .map_err(|_| denied("document destination escapes capability root"))?
        } else {
            input
        };
        let path = relative_path(input)?;
        if path.file_name().is_none() {
            return Err(denied("operation requires an entry below capability root"));
        }
        self.preflight(&path)?;
        if !create {
            self.parent(&path, false)?;
        }
        Ok(AtomicWriteTarget {
            root: self,
            path,
            create,
        })
    }

    pub(crate) fn open_read(&self, input: &str) -> io::Result<std::fs::File> {
        let path = self.resolve(input)?;
        let (parent, leaf) = self.parent(&path, false)?;
        Ok(fs::openat(
            &parent,
            leaf,
            OFlags::RDONLY | OFlags::NOFOLLOW,
            Mode::empty(),
        )?
        .into())
    }

    pub(crate) fn open_document_read(&self, input: &str) -> io::Result<std::fs::File> {
        if input.ends_with('/') || input.ends_with("/.") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "document source requires a file entry",
            ));
        }
        let input = Path::new(input);
        let input = if input.is_absolute() {
            input
                .strip_prefix(&self.base)
                .map_err(|_| denied("document source escapes capability root"))?
        } else {
            input
        };
        let path = relative_path(input)?;
        let (parent, leaf) = self.parent(&path, false)?;
        Ok(fs::openat(
            &parent,
            leaf,
            OFlags::RDONLY | OFlags::NOFOLLOW,
            Mode::empty(),
        )?
        .into())
    }

    pub(crate) fn write(&self, input: &str, bytes: &[u8], create: bool) -> io::Result<()> {
        let path = self.resolve(input)?;
        self.preflight(&path)?;
        let (parent, leaf) = self.parent(&path, create)?;
        let mut file: std::fs::File = fs::openat(
            &parent,
            leaf,
            OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )?
        .into();
        file.write_all(bytes)?;
        file.sync_all()
    }

    pub(crate) fn mkdir(&self, input: &str, parents: bool) -> io::Result<()> {
        let path = self.resolve(input)?;
        if path == Path::new(".") {
            return if parents {
                Ok(())
            } else {
                Err(io::ErrorKind::AlreadyExists.into())
            };
        }
        self.preflight(&path)?;
        let (parent, leaf) = self.parent(&path, parents)?;
        match fs::mkdirat(&parent, &leaf, Mode::RWXU) {
            Ok(()) => Ok(()),
            Err(rustix::io::Errno::EXIST) if parents => {
                open_directory(&parent, Path::new(&leaf))?;
                Ok(())
            }
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn stat(&self, input: &str) -> io::Result<Option<Metadata>> {
        let path = self.resolve(input)?;
        if path == Path::new(".") {
            return Ok(Some(Metadata(fs::fstat(&self.directory)?)));
        }
        let (parent, leaf) = match self.parent(&path, false) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        match fs::statat(&parent, leaf, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(metadata) => Ok(Some(Metadata(metadata))),
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn list(&self, input: &str) -> io::Result<(PathBuf, ReadDir)> {
        let path = self.resolve(input)?;
        let directory = if path == Path::new(".") {
            open_directory(&self.directory, Path::new("."))?
        } else {
            let (parent, leaf) = self.parent(&path, false)?;
            open_directory(&parent, Path::new(&leaf))?
        };
        let iterator = fs::Dir::read_from(&directory)?;
        Ok((
            path,
            ReadDir {
                directory: Arc::new(directory),
                iterator,
            },
        ))
    }

    pub(crate) fn remove(&self, input: &str, recursive: bool) -> io::Result<()> {
        let path = self.resolve(input)?;
        let (parent, leaf) = match self.parent(&path, false) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let metadata = match fs::statat(&parent, &leaf, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(metadata) => Metadata(metadata),
            Err(rustix::io::Errno::NOENT) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_dir() {
            if recursive {
                remove_directory_tree(&parent, Path::new(&leaf), 0)?;
            } else {
                fs::unlinkat(&parent, leaf, AtFlags::REMOVEDIR)?;
            }
        } else {
            fs::unlinkat(&parent, leaf, AtFlags::empty())?;
        }
        Ok(())
    }

    pub(crate) fn rename(
        &self,
        from: &str,
        to: &str,
        overwrite: bool,
        create: bool,
    ) -> io::Result<()> {
        if !overwrite {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "atomic no-replace rename is unavailable on the WASI profile",
            ));
        }
        let source = self.resolve(from)?;
        let destination = self.resolve(to)?;
        self.preflight(&source)?;
        self.preflight(&destination)?;
        let (source_parent, source_leaf) = self.parent(&source, false)?;
        // Reject a missing source before materializing destination parents.
        fs::statat(&source_parent, &source_leaf, AtFlags::SYMLINK_NOFOLLOW)?;
        let (destination_parent, destination_leaf) = self.parent(&destination, create)?;
        Ok(fs::renameat(
            &source_parent,
            source_leaf,
            &destination_parent,
            destination_leaf,
        )?)
    }
}

fn remove_directory_tree(parent: &OwnedFd, name: &Path, depth: usize) -> io::Result<()> {
    if depth == 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WASI recursive removal depth exceeds 256 directories",
        ));
    }
    let directory = open_directory(parent, name)?;
    for entry in fs::Dir::read_from(&directory)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        let metadata = Metadata(fs::statat(&directory, name, AtFlags::SYMLINK_NOFOLLOW)?);
        if metadata.file_type().is_dir() {
            let name = name.to_str().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 directory entry")
            })?;
            remove_directory_tree(&directory, Path::new(name), depth + 1)?;
        } else {
            fs::unlinkat(&directory, name, AtFlags::empty())?;
        }
    }
    Ok(fs::unlinkat(parent, name, AtFlags::REMOVEDIR)?)
}

impl AtomicWriteTarget {
    pub(crate) fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let (parent, leaf) = self.root.parent(&self.path, self.create)?;
        for sequence in 0..1024u32 {
            let temporary = format!(
                ".genesis-write.{}.{}.tmp",
                crate::platform_process_id(),
                sequence
            );
            let mut file: std::fs::File = match fs::openat(
                &parent,
                &temporary,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(file) => file.into(),
                Err(rustix::io::Errno::EXIST) => continue,
                Err(error) => return Err(error.into()),
            };
            let result = (|| {
                file.write_all(bytes)?;
                file.sync_all()?;
                drop(file);
                Ok(fs::renameat(&parent, &temporary, &parent, &leaf)?)
            })();
            if result.is_err() {
                fs::unlinkat(&parent, &temporary, AtFlags::empty())?;
            }
            return result;
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "temporary document slots exhausted",
        ))
    }
}

#[derive(Debug)]
pub(crate) struct Metadata(fs::Stat);
#[derive(Debug)]
pub(crate) struct FileType(fs::FileType);
pub(crate) struct Permissions(bool);
impl Metadata {
    pub(crate) fn file_type(&self) -> FileType {
        FileType(fs::FileType::from_raw_mode(self.0.st_mode))
    }
    pub(crate) fn len(&self) -> u64 {
        self.0.st_size as u64
    }
    pub(crate) fn permissions(&self) -> Permissions {
        // Preview1 filestat has no Unix permission bits. Its libc stat shim
        // therefore cannot supply readonly. Match stable Rust's WASI metadata
        // contract; this observation never grants write authority.
        #[cfg(target_os = "wasi")]
        return Permissions(false);
        #[cfg(not(target_os = "wasi"))]
        Permissions(self.0.st_mode & (Mode::WUSR | Mode::WGRP | Mode::WOTH).bits() == 0)
    }
}
impl FileType {
    pub(crate) fn is_file(&self) -> bool {
        self.0 == fs::FileType::RegularFile
    }
    pub(crate) fn is_dir(&self) -> bool {
        self.0 == fs::FileType::Directory
    }
    pub(crate) fn is_symlink(&self) -> bool {
        self.0 == fs::FileType::Symlink
    }
}
impl Permissions {
    pub(crate) fn readonly(&self) -> bool {
        self.0
    }
}

pub(crate) struct ReadDir {
    directory: Arc<OwnedFd>,
    iterator: fs::Dir,
}
pub(crate) struct DirEntry {
    directory: Arc<OwnedFd>,
    name: OsString,
}
impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.iterator.next()? {
                Ok(entry) => entry,
                Err(error) => return Some(Err(error.into())),
            };
            if entry.file_name().to_bytes() == b"." || entry.file_name().to_bytes() == b".." {
                continue;
            }
            let name = match entry.file_name().to_str() {
                Ok(name) => OsString::from(name),
                Err(_) => {
                    return Some(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "non-UTF-8 directory entry",
                    )));
                }
            };
            return Some(Ok(DirEntry {
                directory: Arc::clone(&self.directory),
                name,
            }));
        }
    }
}
impl DirEntry {
    pub(crate) fn file_name(&self) -> OsString {
        self.name.clone()
    }
    pub(crate) fn metadata(&self) -> io::Result<Metadata> {
        Ok(Metadata(fs::statat(
            &*self.directory,
            &self.name,
            AtFlags::SYMLINK_NOFOLLOW,
        )?))
    }
}

#[cfg(all(test, unix))]
mod controls {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn wasi_rooted_document_read_retains_file_and_denies_links() {
        let fixture = tempfile::tempdir().unwrap();
        let base = fixture.path().join("root");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(base.join("parent")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(base.join("parent/document"), b"inside").unwrap();
        std::fs::write(outside.join("document"), b"outside").unwrap();
        let root = FsRoot::open(&base).unwrap();
        let held = root
            .open_document_read(base.join("parent/document").to_str().unwrap())
            .unwrap();
        std::fs::rename(base.join("parent"), base.join("retained")).unwrap();
        symlink(&outside, base.join("parent")).unwrap();
        assert_eq!(std::io::read_to_string(held).unwrap(), "inside");
        assert!(root.open_document_read("parent/document").is_err());
        symlink("retained/document", base.join("link")).unwrap();
        assert!(root.open_document_read("link").is_err());
        assert!(root.open_document_read("retained/document/").is_err());
        assert!(root.open_document_read("../outside/document").is_err());
        assert!(
            root.open_document_read(outside.join("document").to_str().unwrap())
                .is_err()
        );
    }

    #[test]
    fn wasi_rooted_descriptor_operations_and_metadata() {
        let fixture = tempfile::tempdir().unwrap();
        let root = FsRoot::open(fixture.path()).unwrap();
        root.mkdir("inside/new", true).unwrap();
        root.write("inside/new/source", b"value", false).unwrap();
        assert_eq!(
            std::io::read_to_string(root.open_read("inside/new/source").unwrap()).unwrap(),
            "value"
        );
        let metadata = root.stat("inside/new/source").unwrap().unwrap();
        assert!(metadata.file_type().is_file());
        assert!(!metadata.file_type().is_symlink());
        assert_eq!(metadata.len(), 5);
        assert!(!metadata.permissions().readonly());
        assert!(root.stat("missing/entry").unwrap().is_none());
        assert!(root.stat(".").unwrap().unwrap().file_type().is_dir());
        let (_, entries) = root.list("inside/new").unwrap();
        let entries: Vec<_> = entries.map(Result::unwrap).collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "source");
        assert!(entries[0].metadata().unwrap().file_type().is_file());
        root.rename("inside/new/source", "destination", true, false)
            .unwrap();
        root.remove("inside", true).unwrap();
        root.remove("destination", false).unwrap();
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
    }

    #[test]
    fn wasi_rooted_ancestors_and_stale_preparation_deny_outside_io() {
        let fixture = tempfile::tempdir().unwrap();
        let inside = fixture.path().join("inside");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&inside).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, inside.join("escape")).unwrap();
        let root = FsRoot::open(&inside).unwrap();
        assert!(
            root.write("escape/new/document", b"forbidden", true)
                .is_err()
        );
        assert!(root.mkdir("escape/new", true).is_err());
        assert!(root.open_read("escape/entry").is_err());
        assert!(root.stat("escape/entry").is_err());
        assert!(root.list("escape").is_err());
        assert!(root.remove("escape/entry", true).is_err());
        assert!(
            root.rename("missing", "escape/new/document", true, true)
                .is_err()
        );
        std::fs::create_dir(inside.join("parent")).unwrap();
        let target = root.prepare_atomic_write("parent/document", true).unwrap();
        assert!(!inside.join("parent/document").exists());
        std::fs::rename(inside.join("parent"), inside.join("retained")).unwrap();
        symlink(&outside, inside.join("parent")).unwrap();
        assert!(target.write(b"forbidden").is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[test]
    fn wasi_rooted_entry_replacement_and_failure_cleanup() {
        let fixture = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("retained"), b"retained").unwrap();
        symlink(
            outside.path().join("retained"),
            fixture.path().join("document"),
        )
        .unwrap();
        let slot = format!(".genesis-write.{}.0.tmp", crate::platform_process_id());
        symlink(outside.path().join("retained"), fixture.path().join(&slot)).unwrap();
        let target = FsRoot::open(fixture.path())
            .unwrap()
            .prepare_atomic_write("document", false)
            .unwrap();
        target.write(b"document").unwrap();
        assert_eq!(
            std::fs::read(fixture.path().join("document")).unwrap(),
            b"document"
        );
        assert_eq!(
            std::fs::read(outside.path().join("retained")).unwrap(),
            b"retained"
        );
        std::fs::create_dir(fixture.path().join("directory")).unwrap();
        std::fs::write(fixture.path().join("directory/retained"), b"retained").unwrap();
        let target = FsRoot::open(fixture.path())
            .unwrap()
            .prepare_atomic_write("directory", false)
            .unwrap();
        assert!(target.write(b"cannot replace directory").is_err());
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 3);
        assert!(
            std::fs::symlink_metadata(fixture.path().join(slot))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let root = FsRoot::open(fixture.path()).unwrap();
        let error = root
            .rename("document", "missing/new", false, true)
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        assert!(!fixture.path().join("missing").exists());
        root.rename("document", "document", true, false).unwrap();
        assert_eq!(
            std::fs::read(fixture.path().join("document")).unwrap(),
            b"document"
        );
    }
}
