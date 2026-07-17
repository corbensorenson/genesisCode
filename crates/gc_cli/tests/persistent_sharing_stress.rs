use std::collections::BTreeMap;
use std::fmt::Write as _;

use gc_coreform::{Term, TermOrdKey, parse_term};
use gc_effects::{Decision, EffectLog, EffectLogEntry, GCLOG_CURRENT_VERSION, LoggedResp};
use gc_kernel::{Value, ValueMap, ValueVector};
use gc_pkg::PackageManifest;
use gc_vcs::{Snapshot, SnapshotKind, VCS_SNAPSHOT_VERSION};

const HASH_HEX: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value))
            .collect(),
    )
}

fn exercise_composite_retained_data(cardinality: usize) {
    let base_vector =
        ValueVector::from_iter((0..cardinality).map(|index| Value::int(index as i64)));
    let mut retained_vectors = Vec::with_capacity(cardinality + 1);
    let mut current_vector = gc_kernel::Shared::new(base_vector);
    retained_vectors.push(current_vector.clone());
    for index in 0..cardinality {
        let mut next = current_vector.clone();
        assert!(ValueVector::set_shared(
            &mut next,
            index,
            Value::int(-(index as i64) - 1),
        ));
        retained_vectors.push(next.clone());
        current_vector = next;
    }
    assert_eq!(
        retained_vectors[0].get(0).map(Value::debug_repr),
        Some("0".into())
    );
    assert_eq!(
        current_vector.get(0).map(Value::debug_repr),
        Some("-1".into())
    );

    let mut base_map = ValueMap::new();
    for index in 0..cardinality {
        base_map.insert_mut(
            TermOrdKey(Term::Int(index.into())),
            Value::int(index as i64),
        );
    }
    let mut current_map = gc_kernel::Shared::new(base_map);
    let original_map = current_map.clone();
    let mut retained_maps = Vec::with_capacity(cardinality + 1);
    retained_maps.push(current_map.clone());
    for index in 0..cardinality {
        let mut next = current_map.clone();
        ValueMap::insert_shared(
            &mut next,
            TermOrdKey(Term::Int(index.into())),
            Value::int(-(index as i64) - 1),
        );
        retained_maps.push(next.clone());
        current_map = next;
    }
    assert_eq!(
        original_map
            .get(&TermOrdKey(Term::Int(0.into())))
            .map(Value::debug_repr),
        Some("0".into())
    );
    assert_eq!(
        current_map
            .get(&TermOrdKey(Term::Int(0.into())))
            .map(Value::debug_repr),
        Some("-1".into())
    );

    let retained_strings = (0..cardinality)
        .map(|len| Term::Str("x".repeat(len)))
        .collect::<Vec<_>>();
    let strings_canonical = gc_coreform::print_term(&Term::Vector(retained_strings));
    assert!(strings_canonical.len() < 16 * 1024 * 1024);
    assert!(
        matches!(parse_term(&strings_canonical), Ok(Term::Vector(values)) if values.len() == cardinality)
    );
}

fn exercise_package_graph(cardinality: usize) {
    let directory = tempfile::tempdir().expect("temporary package directory");
    let manifest_path = directory.path().join("package.toml");
    let mut manifest = String::from(
        "schema = 1\nname = \"stress/root\"\nversion = \"0.0.0\"\nmodules = [{ path = \"src/main.gc\" }]\nobligations = []\n",
    );
    for index in 0..cardinality {
        writeln!(
            manifest,
            "\n[[dependencies]]\nname = \"dep/{index:05}\"\npath = \"deps/dep-{index:05}\""
        )
        .expect("write package fixture");
    }
    std::fs::write(&manifest_path, manifest).expect("write package manifest");
    let (parsed, root) = PackageManifest::load(&manifest_path).expect("parse package graph");
    assert_eq!(root, directory.path());
    assert_eq!(parsed.dependencies.len(), cardinality);
    assert_eq!(
        parsed.dependencies[cardinality - 1].name,
        format!("dep/{:05}", cardinality - 1)
    );
}

fn exercise_effect_log(cardinality: usize) {
    let entries = (0..cardinality)
        .map(|index| EffectLogEntry {
            i: index as u64,
            op: "core/io::read".to_string(),
            payload_h: [1; 32],
            cont_h: [2; 32],
            req_h: [3; 32],
            task_id: Some(format!("task-{index:05}")),
            parent_task: None,
            schedule_step: Some(index as u64),
            await_edge: None,
            decision: Decision::Deny,
            cap: Term::Nil,
            resp: LoggedResp::Error(Term::symbol(":denied")),
            resp_h: [4; 32],
        })
        .collect();
    let log = EffectLog {
        version: GCLOG_CURRENT_VERSION,
        program_hash: [5; 32],
        toolchain: "persistent-sharing-stress".to_string(),
        entries,
    };
    let canonical = log.to_string_canonical();
    assert!(canonical.len() < 16 * 1024 * 1024);
    let term = parse_term(&canonical).expect("parse canonical effect log");
    let restored = EffectLog::from_term(&term).expect("restore effect log");
    assert_eq!(restored.entries.len(), cardinality);
    assert_eq!(
        restored.entries[cardinality - 1].i,
        (cardinality - 1) as u64
    );
}

fn exercise_snapshot(cardinality: usize) {
    let modules = (0..cardinality)
        .map(|index| {
            (
                TermOrdKey(Term::symbol(format!("module/{index:05}"))),
                Term::Str(HASH_HEX.to_string()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let term = map([
        (":type", Term::symbol(":vcs/snapshot")),
        (":v", Term::Int(VCS_SNAPSHOT_VERSION.into())),
        (":kind", Term::symbol(":workspace")),
        (":workspace", Term::Str("stress".to_string())),
        (":lock", Term::Nil),
        (":modules", Term::Map(modules)),
    ]);
    let snapshot = Snapshot::from_term(&term).expect("parse workspace snapshot");
    let SnapshotKind::Workspace(workspace) = snapshot.kind else {
        panic!("expected workspace snapshot");
    };
    assert_eq!(workspace.modules.len(), cardinality);
    assert_eq!(workspace.modules["module/00000"], HASH_HEX);
}

fn exercise(cardinality: usize) {
    exercise_composite_retained_data(cardinality);
    exercise_package_graph(cardinality);
    exercise_effect_log(cardinality);
    exercise_snapshot(cardinality);
}

#[test]
fn persistent_sharing_default_control_is_non_vacuous() {
    exercise(8);
}

#[test]
#[ignore = "perf-gate"]
fn persistent_sharing_composite_stress_is_bounded() {
    exercise(4_096);
}
