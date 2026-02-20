use super::VcsCmd;

pub(crate) fn kind(cmd: &VcsCmd) -> &'static str {
    match cmd {
        VcsCmd::Hash { .. } => "genesis/vcs-hash-v0.2",
        VcsCmd::Diff { .. } => "genesis/vcs-diff-v0.1",
        VcsCmd::Apply { .. } => "genesis/vcs-apply-v0.1",
        VcsCmd::Log { .. } => "genesis/vcs-log-v0.1",
        VcsCmd::Blame { .. } => "genesis/vcs-blame-v0.1",
        VcsCmd::Why { .. } => "genesis/vcs-why-v0.1",
        VcsCmd::Merge3 { .. } => "genesis/vcs-merge3-v0.1",
        VcsCmd::ResolveConflict { .. } => "genesis/vcs-resolve-conflict-v0.1",
    }
}

pub(crate) fn log_op(cmd: &VcsCmd) -> &'static str {
    match cmd {
        VcsCmd::Hash { .. } => "vcs-hash",
        VcsCmd::Diff { .. } => "vcs-diff",
        VcsCmd::Apply { .. } => "vcs-apply",
        VcsCmd::Log { .. } => "vcs-log",
        VcsCmd::Blame { .. } => "vcs-blame",
        VcsCmd::Why { .. } => "vcs-why",
        VcsCmd::Merge3 { .. } => "vcs-merge3",
        VcsCmd::ResolveConflict { .. } => "vcs-resolve-conflict",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::{kind, log_op};
    use crate::VcsCmd;

    #[test]
    fn vcs_command_contracts_are_unique_and_stable() {
        let cmds = vec![
            VcsCmd::Hash {
                input: PathBuf::from("module.gc"),
                engine: None,
            },
            VcsCmd::Diff {
                base: "a".repeat(64),
                to: "b".repeat(64),
                out: Some(PathBuf::from("patch.gc")),
                no_store: false,
            },
            VcsCmd::Apply {
                base: "a".repeat(64),
                patch: "b".repeat(64),
                out: Some(PathBuf::from("snapshot.gc")),
                no_store: false,
            },
            VcsCmd::Log {
                root: "refs/heads/main".to_string(),
                max: 100,
            },
            VcsCmd::Blame {
                snapshot: "a".repeat(64),
                sym: "pkg/mod::x".to_string(),
                path: None,
            },
            VcsCmd::Why {
                snapshot: "a".repeat(64),
                sym: "pkg/mod::x".to_string(),
                op: None,
            },
            VcsCmd::Merge3 {
                base: "a".repeat(64),
                left: "b".repeat(64),
                right: "c".repeat(64),
                out: None,
            },
            VcsCmd::ResolveConflict {
                conflict: "a".repeat(64),
                strategy: Some("left".to_string()),
                picks: vec![],
                sets: vec![],
                out: None,
            },
        ];
        let mut kinds = BTreeSet::new();
        let mut ops = BTreeSet::new();
        for cmd in &cmds {
            let k = kind(cmd);
            let op = log_op(cmd);
            assert!(k.starts_with("genesis/vcs-"));
            assert!(op.starts_with("vcs-"));
            assert!(kinds.insert(k), "duplicate kind: {k}");
            assert!(ops.insert(op), "duplicate log op: {op}");
        }
        assert_eq!(kinds.len(), cmds.len());
        assert_eq!(ops.len(), cmds.len());
    }
}
