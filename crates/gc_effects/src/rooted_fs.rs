//! Host filesystem operations anchored to an opened capability directory.
//! Resolved names are internal: consumers receive handles or operation results.
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
pub(crate) use cap_std::fs::FileType;
use cap_std::fs::{Dir, Metadata, OpenOptions, ReadDir};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

#[path = "rooted_fs/scratch_native.rs"]
mod scratch;

pub(crate) struct FsRoot {
    directory: Dir,
    base: PathBuf,
}

/// A document write has no ambient-path conversion. Preparation is read-only;
/// creation and replacement use the same opened capability directory.
pub(crate) struct AtomicWriteTarget {
    root: FsRoot,
    path: PathBuf,
    create: bool,
}

impl AtomicWriteTarget {
    pub(crate) fn write(&self, bytes: &[u8]) -> io::Result<()> {
        let (parent, leaf) = self.root.parent(&self.path, self.create)?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        // Collisions are bounded. Neither an existing entry nor a hostile link
        // may be truncated while acquiring a temporary output.
        for sequence in 0..1024u32 {
            let temporary = OsString::from(format!(
                ".genesis-write.{}.{}.tmp",
                crate::platform_process_id(),
                sequence
            ));
            let mut file = match parent.open_with(&temporary, &options) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            let result = (|| {
                file.write_all(bytes)?;
                file.sync_all()?;
                drop(file);
                parent.rename(&temporary, &parent, &leaf)
            })();
            if result.is_err() {
                parent.remove_file_or_symlink(&temporary)?;
            }
            return result;
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "temporary document slots exhausted",
        ))
    }
}

fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn requires_directory(path: &Path) -> bool {
    let raw = path.as_os_str().as_encoded_bytes();
    raw.ends_with(b"/")
        || raw.ends_with(b"/.")
        || (cfg!(windows) && (raw.ends_with(b"\\") || raw.ends_with(b"\\.")))
}

fn normalize_relative(path: &Path) -> io::Result<PathBuf> {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(name) => result.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return Err(denied("filesystem link escapes capability root"));
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(denied("filesystem path escapes capability root"));
            }
        }
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    Ok(result)
}

impl FsRoot {
    pub(crate) fn open(base: &Path) -> io::Result<Self> {
        let base = std::fs::canonicalize(base)?;
        let directory = Dir::open_ambient_dir(&base, cap_std::ambient_authority())?;
        Ok(Self { directory, base })
    }

    pub(crate) fn prepare_atomic_write(
        self,
        input: &str,
        create: bool,
    ) -> io::Result<AtomicWriteTarget> {
        let input = self.legacy_relative_input(input)?;
        let path = self.resolve_path(PathBuf::from(input), false, true)?;
        if path.file_name().is_none() {
            return Err(denied("operation requires an entry below capability root"));
        }
        if !create {
            self.parent(&path, false)?;
        }
        Ok(AtomicWriteTarget {
            root: self,
            path,
            create,
        })
    }

    // Transitional adapters for host APIs that still consume ambient paths.
    // This preflight prevents authorization-time creation outside the root;
    // the returned pathname is not a race-safe operation capability.
    pub(crate) fn legacy_path(
        &self,
        input: &str,
        follow_final: bool,
        allow_missing: bool,
        create: bool,
    ) -> io::Result<PathBuf> {
        let input = self.legacy_relative_input(input)?;
        let path = self.resolve_path(PathBuf::from(input), follow_final, allow_missing)?;
        if create {
            self.parent(&path, true)?;
        }
        Ok(self.base.join(path))
    }

    fn legacy_relative_input(&self, input: &str) -> io::Result<String> {
        let path = Path::new(input);
        if !path.is_absolute() {
            return Ok(input.to_owned());
        }
        // Legacy document/host adapters accept absolute inside-root names.
        // Resolve their parent only: a final link is still an entry, not the
        // target, when the consuming operation requests no final following.
        let mut relative = match path.strip_prefix(&self.base) {
            Ok(relative) => relative.to_path_buf(),
            Err(_) if std::fs::canonicalize(path).is_ok_and(|canonical| canonical == self.base) => {
                PathBuf::from(".")
            }
            Err(_) => match (path.parent(), path.file_name()) {
                (Some(parent), Some(leaf)) => self.absolute_link_relative(parent)?.join(leaf),
                _ => self.absolute_link_relative(path)?,
            },
        };
        if requires_directory(path) {
            relative.push(".");
        }
        relative
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| denied("non-UTF-8 filesystem path"))
    }

    fn absolute_link_relative(&self, target: &Path) -> io::Result<PathBuf> {
        if let Ok(relative) = target.strip_prefix(&self.base) {
            return Ok(relative.to_path_buf());
        }
        // Ambient canonicalization recognizes alternate names of the configured
        // root. It supplies no operation authority: all subsequent opens and
        // mutations still use the held root directory and a relative name.
        let mut ancestor = target;
        let mut missing = Vec::new();
        loop {
            match std::fs::canonicalize(ancestor) {
                Ok(canonical) => {
                    let mut relative = canonical
                        .strip_prefix(&self.base)
                        .map_err(|_| denied("filesystem link escapes capability root"))?
                        .to_path_buf();
                    relative.extend(missing.iter().rev());
                    return Ok(relative);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    let name = ancestor
                        .file_name()
                        .ok_or_else(|| denied("filesystem link escapes capability root"))?;
                    missing.push(name.to_owned());
                    ancestor = ancestor
                        .parent()
                        .ok_or_else(|| denied("filesystem link escapes capability root"))?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn resolve(&self, input: &str, follow_final: bool, allow_missing: bool) -> io::Result<PathBuf> {
        crate::runner_io_ops::validate_portable_effect_path(input).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid portable filesystem path",
            )
        })?;
        self.resolve_path(PathBuf::from(input), follow_final, allow_missing)
    }

    fn resolve_path(
        &self,
        mut path: PathBuf,
        follow_final: bool,
        allow_missing: bool,
    ) -> io::Result<PathBuf> {
        if path
            .components()
            .any(|part| matches!(part, Component::RootDir | Component::Prefix(_)))
        {
            return Err(denied("filesystem path escapes capability root"));
        }
        let mut followed = 0;
        'resolve: loop {
            let components: Vec<OsString> = path
                .components()
                .map(|part| part.as_os_str().to_owned())
                .collect();
            let mut prefix = PathBuf::new();
            let requires_directory = requires_directory(&path);
            for (index, part) in components.iter().enumerate() {
                if part == "." {
                    continue;
                }
                if part == ".." {
                    if !prefix.pop() {
                        return Err(denied("filesystem link escapes capability root"));
                    }
                    continue;
                }
                prefix.push(part);
                let metadata = match self.directory.symlink_metadata(&prefix) {
                    Ok(metadata) => metadata,
                    Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => {
                        let mut missing = prefix.clone();
                        missing.extend(components.iter().skip(index + 1));
                        // Existing links have already been physically resolved.
                        // Nothing beneath this missing entry can already be a
                        // link. Validate the remaining plan before any creation,
                        // but retain its names so missing/.. is not mistaken for
                        // an existing path during stat or an ordinary open.
                        normalize_relative(&missing)?;
                        return Ok(missing);
                    }
                    Err(error) => return Err(error),
                };
                if !metadata.file_type().is_symlink()
                    || (!follow_final && index + 1 == components.len())
                {
                    if !metadata.is_dir() && (index + 1 < components.len() || requires_directory) {
                        return Err(io::Error::new(
                            io::ErrorKind::NotADirectory,
                            "filesystem ancestor is not a directory",
                        ));
                    }
                    continue;
                }
                if cfg!(target_os = "wasi") {
                    return Err(denied(
                        "symlink traversal is unavailable on the WASI filesystem profile",
                    ));
                }
                followed += 1;
                if followed > 40 {
                    return Err(denied("filesystem symlink traversal limit exceeded"));
                }
                let target = self.directory.read_link_contents(&prefix)?;
                let mut replacement = if target.is_absolute() {
                    self.absolute_link_relative(&target)?
                } else {
                    prefix.parent().unwrap_or(Path::new(".")).join(target)
                };
                replacement.extend(components.iter().skip(index + 1));
                if requires_directory {
                    replacement.push(".");
                }
                path = replacement;
                continue 'resolve;
            }
            if prefix.as_os_str().is_empty() {
                prefix.push(".");
            }
            return Ok(prefix);
        }
    }

    fn parent(&self, path: &Path, create: bool) -> io::Result<(Dir, OsString)> {
        let leaf = path
            .file_name()
            .ok_or_else(|| denied("operation requires an entry below capability root"))?
            .to_owned();
        let parent = path
            .parent()
            .filter(|part| !part.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if create {
            self.directory.create_dir_all(parent)?;
        }
        Ok((self.directory.open_dir(parent)?, leaf))
    }

    pub(crate) fn open_read(&self, input: &str) -> io::Result<std::fs::File> {
        let path = self.resolve(input, true, false)?;
        Ok(self.directory.open(path)?.into_std())
    }

    pub(crate) fn open_document_read(&self, input: &str) -> io::Result<std::fs::File> {
        let input = self.legacy_relative_input(input)?;
        let path = self.resolve_path(PathBuf::from(input), true, false)?;
        Ok(self.directory.open(path)?.into_std())
    }

    pub(crate) fn write(&self, input: &str, bytes: &[u8], create: bool) -> io::Result<()> {
        let path = self.resolve(input, false, true)?;
        let (parent, leaf) = self.parent(&path, create)?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create(true)
            .truncate(true)
            .follow(FollowSymlinks::No);
        let mut file = parent.open_with(leaf, &options)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    pub(crate) fn mkdir(&self, input: &str, parents: bool) -> io::Result<()> {
        let path = self.resolve(input, true, true)?;
        if parents {
            self.directory.create_dir_all(path)
        } else {
            self.directory.create_dir(path)
        }
    }

    pub(crate) fn stat(&self, input: &str) -> io::Result<Option<Metadata>> {
        let path = self.resolve(input, false, true)?;
        match self.directory.symlink_metadata(path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn list(&self, input: &str) -> io::Result<(PathBuf, ReadDir)> {
        let path = self.resolve(input, true, false)?;
        let entries = self.directory.read_dir(&path)?;
        Ok((path, entries))
    }

    pub(crate) fn remove(&self, input: &str, recursive: bool) -> io::Result<()> {
        let path = self.resolve(input, false, true)?;
        let (parent, leaf) = match self.parent(&path, false) {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let metadata = match parent.symlink_metadata(&leaf) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            if recursive {
                parent.remove_dir_all(leaf)
            } else {
                parent.remove_dir(leaf)
            }
        } else {
            parent.remove_file_or_symlink(leaf)
        }
    }

    pub(crate) fn rename(
        &self,
        from: &str,
        to: &str,
        overwrite: bool,
        create: bool,
    ) -> io::Result<()> {
        #[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
        if !overwrite {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "atomic no-replace rename is unavailable on this host",
            ));
        }
        // Resolve both entries before creating any destination ancestors.
        let source = self.resolve(from, false, false)?;
        let destination = self.resolve(to, false, true)?;
        let (source_parent, source_leaf) = self.parent(&source, false)?;
        let (destination_parent, destination_leaf) = self.parent(&destination, create)?;
        if overwrite {
            source_parent.rename(source_leaf, &destination_parent, destination_leaf)
        } else {
            #[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
            {
                rustix::fs::renameat_with(
                    &source_parent,
                    source_leaf,
                    &destination_parent,
                    destination_leaf,
                    rustix::fs::RenameFlags::NOREPLACE,
                )
                .map_err(Into::into)
            }
            #[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
            {
                let _ = (
                    source_parent,
                    source_leaf,
                    destination_parent,
                    destination_leaf,
                );
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "atomic no-replace rename is unavailable on this host",
                ))
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn rooted_document_preparation_is_read_only_and_denies_escaping_ancestors() {
        let fixture = tempfile::tempdir().unwrap();
        let inside = fixture.path().join("inside");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&inside).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, inside.join("escape")).unwrap();
        let result = FsRoot::open(&inside)
            .unwrap()
            .prepare_atomic_write("escape/new/document", true);
        assert!(result.is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        let target = FsRoot::open(&inside)
            .unwrap()
            .prepare_atomic_write("new/document", true)
            .unwrap();
        assert!(!inside.join("new").exists());
        target.write(b"document").unwrap();
        assert_eq!(
            std::fs::read(inside.join("new/document")).unwrap(),
            b"document"
        );
    }

    #[test]
    fn rooted_document_rejects_ancestor_replacement_after_preparation() {
        let fixture = tempfile::tempdir().unwrap();
        let inside = fixture.path().join("inside");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(inside.join("parent")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(inside.join("parent/document"), b"retained").unwrap();
        let target = FsRoot::open(&inside)
            .unwrap()
            .prepare_atomic_write("parent/document", true)
            .unwrap();
        std::fs::rename(inside.join("parent"), inside.join("moved")).unwrap();
        symlink(&outside, inside.join("parent")).unwrap();
        assert!(target.write(b"forbidden").is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(
            std::fs::read(inside.join("moved/document")).unwrap(),
            b"retained"
        );
    }

    #[test]
    fn rooted_document_failed_replacement_removes_temporary_output() {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::create_dir(fixture.path().join("destination")).unwrap();
        std::fs::write(fixture.path().join("destination/retained"), b"retained").unwrap();
        let target = FsRoot::open(fixture.path())
            .unwrap()
            .prepare_atomic_write("destination", false)
            .unwrap();
        assert!(target.write(b"cannot replace directory").is_err());
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 1);
        assert_eq!(
            std::fs::read(fixture.path().join("destination/retained")).unwrap(),
            b"retained"
        );
    }

    #[test]
    fn rooted_document_never_truncates_an_occupied_temporary_slot() {
        let fixture = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("retained"), b"retained").unwrap();
        let slot = fixture.path().join(format!(
            ".genesis-write.{}.0.tmp",
            crate::platform_process_id()
        ));
        symlink(outside.path().join("retained"), &slot).unwrap();
        let target = FsRoot::open(fixture.path())
            .unwrap()
            .prepare_atomic_write("document", false)
            .unwrap();
        target.write(b"document").unwrap();
        assert!(
            std::fs::symlink_metadata(slot)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read(outside.path().join("retained")).unwrap(),
            b"retained"
        );
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 2);
    }

    #[test]
    fn rooted_document_replaces_final_link_without_touching_target() {
        let fixture = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("retained"), b"retained").unwrap();
        symlink(
            outside.path().join("retained"),
            fixture.path().join("document"),
        )
        .unwrap();
        let target = FsRoot::open(fixture.path())
            .unwrap()
            .prepare_atomic_write("document", false)
            .unwrap();
        target.write(b"document").unwrap();
        assert_eq!(
            std::fs::read(outside.path().join("retained")).unwrap(),
            b"retained"
        );
        assert!(
            std::fs::symlink_metadata(fixture.path().join("document"))
                .unwrap()
                .is_file()
        );
    }

    #[test]
    fn legacy_mutator_preflight_denies_before_directory_creation() {
        let fixture = tempfile::tempdir().unwrap();
        let inside = fixture.path().join("inside");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&inside).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, inside.join("escape")).unwrap();
        assert!(
            crate::runner_io_ops::sandbox_path_write(&inside, "escape/new/document", true).is_err()
        );
        assert!(
            crate::runner_io_ops::sandbox_path_allow_missing(&inside, "escape/new/document", true)
                .is_err()
        );
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(&inside).unwrap().count(), 1);
    }

    #[test]
    fn held_parent_rename_cannot_be_redirected_by_replacing_its_name() {
        let fixture = tempfile::tempdir().unwrap();
        let inside = fixture.path().join("inside");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(inside.join("parent")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(inside.join("parent/source"), b"source").unwrap();
        let root = FsRoot::open(&inside).unwrap();
        let (parent, source) = root.parent(Path::new("parent/source"), false).unwrap();
        std::fs::rename(inside.join("parent"), inside.join("moved")).unwrap();
        symlink(&outside, inside.join("parent")).unwrap();
        parent.rename(source, &parent, "destination").unwrap();
        assert_eq!(
            std::fs::read(inside.join("moved/destination")).unwrap(),
            b"source"
        );
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    }
}

#[cfg(all(test, unix))]
mod absolute_document_controls {
    use super::*;

    #[test]
    fn absolute_legacy_document_names_retain_rooted_entry_semantics() {
        let fixture = tempfile::tempdir().unwrap();
        let inside = fixture.path().join("inside");
        let outside = fixture.path().join("outside");
        std::fs::create_dir(&inside).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("retained"), b"retained").unwrap();
        std::os::unix::fs::symlink(outside.join("retained"), inside.join("document")).unwrap();
        for path in [inside.join("document"), inside.join("missing/document")] {
            let target = FsRoot::open(&inside)
                .unwrap()
                .prepare_atomic_write(path.to_str().unwrap(), true)
                .unwrap();
            target.write(b"document").unwrap();
            assert_eq!(std::fs::read(path).unwrap(), b"document");
        }
        assert_eq!(
            std::fs::read(outside.join("retained")).unwrap(),
            b"retained"
        );
        assert!(
            FsRoot::open(&inside)
                .unwrap()
                .prepare_atomic_write(outside.join("new/document").to_str().unwrap(), true)
                .is_err()
        );
        assert!(!outside.join("new").exists());
    }
}
