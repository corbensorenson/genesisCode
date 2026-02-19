use super::PkgCmd;

pub(crate) fn kind(cmd: &PkgCmd) -> &'static str {
    match cmd {
        PkgCmd::New { .. } => "genesis/pkg-new-v0.1",
        PkgCmd::Init { .. } => "genesis/pkg-init-v0.1",
        PkgCmd::Add { .. } => "genesis/pkg-add-v0.1",
        PkgCmd::Remove { .. } => "genesis/pkg-remove-v0.1",
        PkgCmd::Lock { .. } => "genesis/pkg-lock-v0.1",
        PkgCmd::Update { .. } => "genesis/pkg-update-v0.1",
        PkgCmd::Run { .. } => "genesis/pkg-run-v0.1",
        PkgCmd::Test { .. } => "genesis/pkg-test-v0.1",
        PkgCmd::Install { .. } => "genesis/pkg-install-v0.1",
        PkgCmd::Verify { .. } => "genesis/pkg-verify-v0.1",
        PkgCmd::Doctor { .. } => "genesis/pkg-doctor-v0.1",
        PkgCmd::List { .. } => "genesis/pkg-list-v0.1",
        PkgCmd::Info { .. } => "genesis/pkg-info-v0.1",
        PkgCmd::Abi { .. } => "genesis/pkg-abi-v0.1",
        PkgCmd::Snapshot { .. } => "genesis/pkg-snapshot-v0.1",
        PkgCmd::Export { .. } => "genesis/pkg-export-v0.1",
        PkgCmd::Import { .. } => "genesis/pkg-import-v0.1",
        PkgCmd::Publish { .. } => "genesis/pkg-publish-v0.1",
        PkgCmd::Migrate { .. } => "genesis/pkg-migrate-v0.1",
        PkgCmd::Env { .. } => "genesis/pkg-env-v0.1",
    }
}

pub(crate) fn log_op(cmd: &PkgCmd) -> &'static str {
    match cmd {
        PkgCmd::New { .. } => "pkg-new",
        PkgCmd::Init { .. } => "pkg-init",
        PkgCmd::Add { .. } => "pkg-add",
        PkgCmd::Remove { .. } => "pkg-remove",
        PkgCmd::Lock { .. } => "pkg-lock",
        PkgCmd::Update { .. } => "pkg-update",
        PkgCmd::Run { .. } => "pkg-run",
        PkgCmd::Test { .. } => "pkg-test",
        PkgCmd::Install { .. } => "pkg-install",
        PkgCmd::Verify { .. } => "pkg-verify",
        PkgCmd::Doctor { .. } => "pkg-doctor",
        PkgCmd::List { .. } => "pkg-list",
        PkgCmd::Info { .. } => "pkg-info",
        PkgCmd::Abi { .. } => "pkg-abi",
        PkgCmd::Snapshot { .. } => "pkg-snapshot",
        PkgCmd::Export { .. } => "pkg-export",
        PkgCmd::Import { .. } => "pkg-import",
        PkgCmd::Publish { .. } => "pkg-publish",
        PkgCmd::Migrate { .. } => "pkg-migrate",
        PkgCmd::Env { .. } => "pkg-env",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::{kind, log_op};
    use crate::PkgCmd;

    #[test]
    fn pkg_command_contracts_are_unique_and_stable() {
        let cmds = vec![
            PkgCmd::New {
                workspace: "ws".to_string(),
                lock: PathBuf::from("genesis.lock"),
                workspace_file: PathBuf::from("genesis.workspace.toml"),
                policy: "policy:default-v0.1".to_string(),
                registry_default: None,
                members: vec![],
            },
            PkgCmd::Init {
                workspace: "ws".to_string(),
                lock: PathBuf::from("genesis.lock"),
                policy: "policy:default-v0.1".to_string(),
                registry_default: None,
            },
            PkgCmd::Add {
                spec: "dep@snapshot:abc".to_string(),
                lock: PathBuf::from("genesis.lock"),
                update_policy: "manual".to_string(),
                registry: None,
                strategy: None,
                tag_policy: None,
            },
            PkgCmd::Remove {
                name: "dep".to_string(),
                lock: PathBuf::from("genesis.lock"),
            },
            PkgCmd::Lock {
                lock: PathBuf::from("genesis.lock"),
                strict: false,
            },
            PkgCmd::Update {
                lock: PathBuf::from("genesis.lock"),
            },
            PkgCmd::Run {
                task: "test".to_string(),
                workspace_file: PathBuf::from("genesis.workspace.toml"),
            },
            PkgCmd::Test {
                pkg: PathBuf::from("package.toml"),
                caps: None,
            },
            PkgCmd::Install {
                lock: PathBuf::from("genesis.lock"),
                frozen: false,
                strict: false,
            },
            PkgCmd::Verify {
                lock: PathBuf::from("genesis.lock"),
            },
            PkgCmd::Doctor {
                lock: PathBuf::from("genesis.lock"),
            },
            PkgCmd::List {
                lock: PathBuf::from("genesis.lock"),
            },
            PkgCmd::Info {
                name: "dep".to_string(),
                lock: PathBuf::from("genesis.lock"),
            },
            PkgCmd::Abi {
                pkg: PathBuf::from("package.toml"),
            },
            PkgCmd::Snapshot {
                pkg: PathBuf::from("package.toml"),
            },
            PkgCmd::Export {
                root: "root".to_string(),
                out: PathBuf::from("out.gpk"),
                full: false,
                depth: 0,
                include_evidence: "required".to_string(),
                include_deps: "locked".to_string(),
                include_refs: vec![],
            },
            PkgCmd::Import {
                input: PathBuf::from("in.gpk"),
                set_refs: vec![],
                policy: None,
            },
            PkgCmd::Publish {
                remote: "gen://local".to_string(),
                refname: "refs/heads/main".to_string(),
                policy: "a".repeat(64),
                expected_old: None,
                depth: 0,
                commit: None,
            },
            PkgCmd::Migrate {
                pkg: PathBuf::from("package.toml"),
                lock: PathBuf::from("genesis.lock"),
                workspace_file: PathBuf::from("genesis.workspace.toml"),
                workspace: None,
                registry_default: None,
            },
            PkgCmd::Env {
                profile: "dev".to_string(),
                lock: PathBuf::from("genesis.lock"),
                workspace_file: PathBuf::from("genesis.workspace.toml"),
                out_dir: PathBuf::from(".genesis/env"),
            },
        ];

        let mut kinds = BTreeSet::new();
        let mut ops = BTreeSet::new();
        for c in &cmds {
            let k = kind(c);
            let op = log_op(c);
            assert!(k.starts_with("genesis/pkg-"));
            assert!(k.ends_with("-v0.1"));
            assert!(op.starts_with("pkg-"));
            assert!(kinds.insert(k), "duplicate kind: {k}");
            assert!(ops.insert(op), "duplicate log op: {op}");
        }
        assert_eq!(kinds.len(), cmds.len());
        assert_eq!(ops.len(), cmds.len());
    }
}
