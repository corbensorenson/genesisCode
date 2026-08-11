use super::ReaderSupportMode;

#[derive(Clone, Copy)]
pub(super) struct ReaderRecord {
    pub(super) compatibility_id: &'static str,
    pub(super) component: &'static str,
    pub(super) current_writer: &'static str,
    pub(super) reader: &'static str,
    pub(super) mode: ReaderSupportMode,
    pub(super) migration_record: Option<&'static str>,
}

pub(super) const READER_RECORDS: &[ReaderRecord] = &[
    reader(
        "genesis/compat/v1/language-profile",
        "language-semantics-v0.2",
        "v0.2",
        "v0.2",
    ),
    reader(
        "genesis/compat/v1/coreform",
        "canonical-coreform-v0.2",
        "genesis/coreform/v0.2",
        "genesis/coreform/v0.2",
    ),
    reader(
        "genesis/compat/v1/coreform",
        "canonical-term-hash",
        "genesis/hash-profile/gcv0.2-blake3",
        "genesis/hash-profile/gcv0.2-blake3",
    ),
    reader(
        "genesis/compat/v1/value-effect-hash",
        "value-and-effect-request-hash",
        "genesis/value-effect-hash/v0.2",
        "genesis/value-effect-hash/v0.2",
    ),
    legacy(
        "genesis/compat/v1/effect-log",
        "gclog",
        "3",
        "2",
        "M-GCLOG-2-TO-3",
    ),
    reader("genesis/compat/v1/effect-log", "gclog", "3", "3"),
    reader(
        "genesis/compat/v1/evidence",
        "evidence-bundle-v0.1",
        "genesis/evidence-profile/v0.1",
        "genesis/evidence-profile/v0.1",
    ),
    legacy(
        "genesis/compat/v1/package",
        "package-manifest",
        "1",
        "pre-schema",
        "M-PACKAGE-PRESCHEMA-TO-1",
    ),
    reader("genesis/compat/v1/package", "package-manifest", "1", "1"),
    reader("genesis/compat/v1/package", "workspace", "1", "1"),
    legacy(
        "genesis/compat/v1/package",
        "lock",
        "2",
        "1",
        "M-LOCK-1-TO-2",
    ),
    reader("genesis/compat/v1/package", "lock", "2", "2"),
    legacy("genesis/compat/v1/package", "gpk", "2", "1", "M-GPK-1-TO-2"),
    reader("genesis/compat/v1/package", "gpk", "2", "2"),
    reader("genesis/compat/v1/patch", "semantic-gcpatch", "1", "1"),
    reader("genesis/compat/v1/patch", "vcs-patch", "1", "1"),
    reader("genesis/compat/v1/snapshot", "vcs-snapshot", "1", "1"),
    reader(
        "genesis/compat/v1/bootstrap",
        "selfhost-toolchain-artifact",
        "genesis/selfhost-toolchain-artifact-v0.2/:v=1",
        "genesis/selfhost-toolchain-artifact-v0.2/:v=1",
    ),
];

const fn reader(
    compatibility_id: &'static str,
    component: &'static str,
    current_writer: &'static str,
    value: &'static str,
) -> ReaderRecord {
    ReaderRecord {
        compatibility_id,
        component,
        current_writer,
        reader: value,
        mode: ReaderSupportMode::Current,
        migration_record: None,
    }
}

const fn legacy(
    compatibility_id: &'static str,
    component: &'static str,
    current_writer: &'static str,
    value: &'static str,
    migration_record: &'static str,
) -> ReaderRecord {
    ReaderRecord {
        compatibility_id,
        component,
        current_writer,
        reader: value,
        mode: ReaderSupportMode::Legacy,
        migration_record: Some(migration_record),
    }
}
