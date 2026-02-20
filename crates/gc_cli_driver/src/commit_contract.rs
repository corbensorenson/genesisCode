use super::CommitCmd;

pub(crate) fn kind(cmd: &CommitCmd) -> &'static str {
    match cmd {
        CommitCmd::New { .. } => "genesis/commit-new-v0.1",
        CommitCmd::Show { .. } => "genesis/commit-show-v0.1",
    }
}

pub(crate) fn log_op(cmd: &CommitCmd) -> &'static str {
    match cmd {
        CommitCmd::New { .. } => "commit-new",
        CommitCmd::Show { .. } => "commit-show",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{kind, log_op};
    use crate::{CommitCmd, CommitTargetKind};

    #[test]
    fn commit_command_contracts_are_unique_and_stable() {
        let cmds = vec![
            CommitCmd::New {
                target_kind: CommitTargetKind::Package,
                target_id: "pkg/demo".to_string(),
                base: "a".repeat(64),
                patch: "b".repeat(64),
                message: "m".to_string(),
                why: None,
                obligations: vec!["core/obligation::unit-tests".to_string()],
                evidence: vec!["c".repeat(64)],
                author: Some("dev".to_string()),
                sign: None,
                store: true,
            },
            CommitCmd::Show {
                hash: "d".repeat(64),
            },
        ];
        let mut kinds = BTreeSet::new();
        let mut ops = BTreeSet::new();
        for cmd in &cmds {
            let k = kind(cmd);
            let op = log_op(cmd);
            assert!(k.starts_with("genesis/commit-"));
            assert!(op.starts_with("commit-"));
            assert!(kinds.insert(k), "duplicate kind: {k}");
            assert!(ops.insert(op), "duplicate log op: {op}");
        }
        assert_eq!(kinds.len(), cmds.len());
        assert_eq!(ops.len(), cmds.len());
    }
}
