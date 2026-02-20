use super::GcCmd;

pub(crate) fn kind(cmd: &GcCmd) -> &'static str {
    match cmd {
        GcCmd::Plan { .. } => "genesis/gc-plan-v0.1",
        GcCmd::Run { .. } => "genesis/gc-run-v0.1",
        GcCmd::Pin { .. } => "genesis/gc-pin-v0.1",
        GcCmd::Unpin { .. } => "genesis/gc-unpin-v0.1",
        GcCmd::Purge { .. } => "genesis/gc-purge-v0.1",
    }
}

pub(crate) fn log_op(cmd: &GcCmd) -> &'static str {
    match cmd {
        GcCmd::Plan { .. } => "gc-plan",
        GcCmd::Run { .. } => "gc-run",
        GcCmd::Pin { .. } => "gc-pin",
        GcCmd::Unpin { .. } => "gc-unpin",
        GcCmd::Purge { .. } => "gc-purge",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    use super::{kind, log_op};
    use crate::GcCmd;

    #[test]
    fn gc_command_contracts_are_unique_and_stable() {
        let cmds = vec![
            GcCmd::Plan {
                lock: PathBuf::from("genesis.lock"),
                pins: PathBuf::from(".genesis/pins.toml"),
                depth: 200,
                no_lock: false,
                no_refs: false,
            },
            GcCmd::Run {
                lock: PathBuf::from("genesis.lock"),
                pins: PathBuf::from(".genesis/pins.toml"),
                depth: 200,
                no_lock: false,
                no_refs: false,
                quarantine: true,
                quarantine_dir: Some(PathBuf::from(".genesis/quarantine")),
            },
            GcCmd::Pin {
                target: "refs/heads/main".to_string(),
                pins: PathBuf::from(".genesis/pins.toml"),
            },
            GcCmd::Unpin {
                target: "refs/heads/main".to_string(),
                pins: PathBuf::from(".genesis/pins.toml"),
            },
            GcCmd::Purge {
                ttl_days: 7,
                quarantine_dir: Some(PathBuf::from(".genesis/quarantine")),
            },
        ];
        let mut kinds = BTreeSet::new();
        let mut ops = BTreeSet::new();
        for cmd in &cmds {
            let k = kind(cmd);
            let op = log_op(cmd);
            assert!(k.starts_with("genesis/gc-"));
            assert!(k.ends_with("-v0.1"));
            assert!(op.starts_with("gc-"));
            assert!(kinds.insert(k), "duplicate kind: {k}");
            assert!(ops.insert(op), "duplicate log op: {op}");
        }
        assert_eq!(kinds.len(), cmds.len());
        assert_eq!(ops.len(), cmds.len());
    }
}
