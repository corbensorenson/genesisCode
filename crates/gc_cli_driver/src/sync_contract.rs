use super::SyncCmd;

pub(crate) fn kind(cmd: &SyncCmd) -> &'static str {
    match cmd {
        SyncCmd::Pull { .. } => "genesis/sync-pull-v0.1",
        SyncCmd::Push { .. } => "genesis/sync-push-v0.1",
    }
}

pub(crate) fn log_op(cmd: &SyncCmd) -> &'static str {
    match cmd {
        SyncCmd::Pull { .. } => "sync-pull",
        SyncCmd::Push { .. } => "sync-push",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{kind, log_op};
    use crate::SyncCmd;

    #[test]
    fn sync_command_contracts_are_unique_and_stable() {
        let cmds = vec![
            SyncCmd::Pull {
                remote: "gen://remote".to_string(),
                refs: vec!["refs/heads/main".to_string()],
                roots: vec!["a".repeat(64)],
                depth: 1,
                force: false,
            },
            SyncCmd::Push {
                remote: "gen://remote".to_string(),
                roots: vec!["a".repeat(64)],
                depth: 1,
                set_refs: vec![format!("refs/heads/main={}", "a".repeat(64))],
            },
        ];
        let mut kinds = BTreeSet::new();
        let mut ops = BTreeSet::new();
        for cmd in &cmds {
            let k = kind(cmd);
            let op = log_op(cmd);
            assert!(k.starts_with("genesis/sync-"));
            assert!(k.ends_with("-v0.1"));
            assert!(op.starts_with("sync-"));
            assert!(kinds.insert(k), "duplicate kind: {k}");
            assert!(ops.insert(op), "duplicate log op: {op}");
        }
        assert_eq!(kinds.len(), cmds.len());
        assert_eq!(ops.len(), cmds.len());
    }
}
