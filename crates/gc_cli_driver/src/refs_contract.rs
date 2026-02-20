use super::RefsCmd;

pub(crate) fn kind(cmd: &RefsCmd) -> &'static str {
    match cmd {
        RefsCmd::Get { .. } => "genesis/refs-get-v0.1",
        RefsCmd::List { .. } => "genesis/refs-list-v0.1",
        RefsCmd::Set { .. } => "genesis/refs-set-v0.1",
        RefsCmd::Delete { .. } => "genesis/refs-delete-v0.1",
    }
}

pub(crate) fn log_op(cmd: &RefsCmd) -> &'static str {
    match cmd {
        RefsCmd::Get { .. } => "refs-get",
        RefsCmd::List { .. } => "refs-list",
        RefsCmd::Set { .. } => "refs-set",
        RefsCmd::Delete { .. } => "refs-delete",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{kind, log_op};
    use crate::RefsCmd;

    #[test]
    fn refs_command_contracts_are_unique_and_stable() {
        let cmds = vec![
            RefsCmd::Get {
                name: "refs/heads/main".to_string(),
            },
            RefsCmd::List {
                prefix: Some("refs/heads/".to_string()),
            },
            RefsCmd::Set {
                name: "refs/heads/main".to_string(),
                hash: "a".repeat(64),
                policy: "b".repeat(64),
                expected_old: Some("nil".to_string()),
            },
            RefsCmd::Delete {
                name: "refs/heads/main".to_string(),
                policy: "b".repeat(64),
                expected_old: None,
            },
        ];
        let mut kinds = BTreeSet::new();
        let mut ops = BTreeSet::new();
        for cmd in &cmds {
            let k = kind(cmd);
            let op = log_op(cmd);
            assert!(k.starts_with("genesis/refs-"));
            assert!(k.ends_with("-v0.1"));
            assert!(op.starts_with("refs-"));
            assert!(kinds.insert(k), "duplicate kind: {k}");
            assert!(ops.insert(op), "duplicate log op: {op}");
        }
        assert_eq!(kinds.len(), cmds.len());
        assert_eq!(ops.len(), cmds.len());
    }
}
