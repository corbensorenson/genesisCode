//! Explicit stable-WASI admission boundary. Native directory capabilities rely
//! on an unavailable stable API on this target. No pathname fallback may claim
//! their guarantees. CLI bootstrap I/O remains separately governed.
pub(crate) use std::fs::FileType;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) struct FsRoot;
pub(crate) struct AtomicWriteTarget;

fn unsupported<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "rooted filesystem capability requires a supported directory-handle adapter; stable WASI is not admitted",
    ))
}

impl FsRoot {
    pub(crate) fn open(_: &Path) -> io::Result<Self> {
        unsupported()
    }
    pub(crate) fn prepare_atomic_write(self, _: &str, _: bool) -> io::Result<AtomicWriteTarget> {
        unsupported()
    }
    pub(crate) fn open_read(&self, _: &str) -> io::Result<std::fs::File> {
        unsupported()
    }
    pub(crate) fn write(&self, _: &str, _: &[u8], _: bool) -> io::Result<()> {
        unsupported()
    }
    pub(crate) fn mkdir(&self, _: &str, _: bool) -> io::Result<()> {
        unsupported()
    }
    pub(crate) fn stat(&self, _: &str) -> io::Result<Option<std::fs::Metadata>> {
        unsupported()
    }
    pub(crate) fn list(&self, _: &str) -> io::Result<(PathBuf, std::fs::ReadDir)> {
        unsupported()
    }
    pub(crate) fn remove(&self, _: &str, _: bool) -> io::Result<()> {
        unsupported()
    }
    pub(crate) fn rename(&self, _: &str, _: &str, _: bool, _: bool) -> io::Result<()> {
        unsupported()
    }
}

impl AtomicWriteTarget {
    pub(crate) fn write(&self, _: &[u8]) -> io::Result<()> {
        unsupported()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_wasi_adapter_denies_every_entrypoint_without_io() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        std::fs::write(&source, b"retained").unwrap();
        let _: Option<FileType> = None;
        let denied = |error: io::Error| assert_eq!(error.kind(), io::ErrorKind::Unsupported);
        denied(FsRoot::open(fixture.path()).err().unwrap());
        denied(
            FsRoot
                .prepare_atomic_write("new/document", true)
                .err()
                .unwrap(),
        );
        denied(FsRoot.open_read("source").err().unwrap());
        denied(FsRoot.write("source", b"forbidden", true).unwrap_err());
        denied(FsRoot.mkdir("new/directory", true).unwrap_err());
        denied(FsRoot.stat("source").unwrap_err());
        denied(FsRoot.list(".").unwrap_err());
        denied(FsRoot.remove("source", true).unwrap_err());
        denied(
            FsRoot
                .rename("source", "new/destination", true, true)
                .unwrap_err(),
        );
        denied(AtomicWriteTarget.write(b"forbidden").unwrap_err());
        assert_eq!(std::fs::read(source).unwrap(), b"retained");
        assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 1);
    }
}
