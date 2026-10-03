use gc_coreform::{Term, TermOrdKey, hash_module, parse_module, print_term};
use gc_effects::{ArtifactStore, CapsPolicy, run};
use gc_kernel::{EvalCtx, Value, eval_module, eval_module_compiled};
use gc_prelude::SelfhostBootstrapMode;
use std::collections::BTreeMap;
use std::path::Path;

fn identity(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn snapshot(child: &str) -> String {
    format!(
        "{{:type :vcs/snapshot :v 1 :kind :package :pkg/name \"import-control\" :pkg/version \"0.1.0\" :modules [{{:path \"module.gc\" :hash {child:?} :module-h b\"{}\"}}] :obligations []}}",
        "\\x00".repeat(32)
    )
}

fn perform(policy: &CapsPolicy, remote: &str, roots: &[String], compiled: bool) -> Value {
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":remote")),
                Term::Str(remote.into()),
            ),
            (
                TermOrdKey(Term::symbol(":roots")),
                Term::Vector(roots.iter().cloned().map(Term::Str).collect()),
            ),
            (TermOrdKey(Term::symbol(":depth")), Term::Int(0.into())),
        ]
        .into_iter()
        .collect(),
    );
    let forms = parse_module(&format!(
        "(core/effect::perform 'core/sync::pull {} (fn (response) (core/effect::pure response)))",
        print_term(&payload)
    ))
    .unwrap();
    let mut context = EvalCtx::new();
    let mut env = gc_prelude::build_prelude(&mut context).env;
    let program = if compiled {
        eval_module_compiled(&mut context, &mut env, &forms)
    } else {
        eval_module(&mut context, &mut env, &forms)
    }
    .unwrap();
    run(
        &mut context,
        policy,
        program,
        hash_module(&forms),
        "sync-import-file-control".into(),
    )
    .unwrap()
    .value
}

fn inventory(root: &Path) -> BTreeMap<std::ffi::OsString, Vec<u8>> {
    std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (entry.file_name(), std::fs::read(entry.path()).unwrap())
        })
        .collect()
}

pub(crate) fn controls(root: &Path, artifact: &Path, compiled: bool) {
    for (name, nested, limit) in [
        ("nested", true, None),
        ("roots", false, None),
        ("budget", true, Some(1)),
    ] {
        let workspace = root.join(name);
        std::fs::create_dir_all(&workspace).unwrap();
        let remote_dir = workspace.join("remote/v1/store");
        std::fs::create_dir_all(&remote_dir).unwrap();
        let remote = format!("file://{}/", workspace.join("remote").display());
        let prefix_bytes = b"honest prefix";
        let prefix = identity(prefix_bytes);
        std::fs::write(remote_dir.join(&prefix), prefix_bytes).unwrap();
        let expected_bytes = b"honest leaf";
        let expected = identity(expected_bytes);
        let child_bytes = snapshot(&expected);
        let child = identity(child_bytes.as_bytes());
        let root_bytes = snapshot(&child);
        let root_hash = identity(root_bytes.as_bytes());
        for (hash, bytes) in [
            (&child, child_bytes.as_bytes()),
            (&root_hash, root_bytes.as_bytes()),
        ] {
            std::fs::write(remote_dir.join(hash), bytes).unwrap();
        }
        std::fs::write(
            remote_dir.join(&expected),
            if limit.is_some() {
                expected_bytes.as_slice()
            } else {
                b"corrupt"
            },
        )
        .unwrap();
        let roots = if nested {
            vec![root_hash.clone()]
        } else {
            vec![prefix, expected.clone()]
        };
        // The budget admits exactly the first artifact and rejects the next
        // closure batch; it is a logical-write bound, not a timing oracle.
        let budget = limit
            .map(|_| format!("max_run_bytes = {}\n", root_bytes.len()))
            .unwrap_or_default();
        let local = workspace.join("local");
        let store = ArtifactStore::open(&local).unwrap();
        let retained = store.put_bytes(b"retained").unwrap();
        let policy = CapsPolicy::from_toml_str_with_selfhost_authority(&format!(
            "allow = [\"core/sync::pull\"]\n[store]\ndir = {:?}\n{}[refs]\npath = {:?}\n[op.\"core/sync::pull\"]\nremote_allow = [{remote:?}]\nwasi_network_profile = \"local\"\ntransfer_workers = 1\nmax_artifact_bytes = 1024\nmax_batch_bytes = 2048\n",
            local.to_str().unwrap(), budget, workspace.join("refs.gc").to_str().unwrap()
        ), SelfhostBootstrapMode::ArtifactOnly, Some(artifact)).unwrap();
        let before = inventory(&local);
        let value = perform(&policy, &remote, &roots, compiled);
        let code = if limit.is_some() {
            "core/caps/resource-limit"
        } else {
            "core/sync/hash-mismatch"
        };
        assert!(
            matches!(value, Value::Sealed { .. }) && value.debug_repr().contains(code),
            "{}",
            value.debug_repr()
        );
        assert_eq!(
            inventory(&local),
            before,
            "file-backed late rejection changed the entire destination inventory"
        );
        assert_eq!(store.get_bytes(&retained).unwrap(), b"retained");
        if limit.is_none() {
            std::fs::write(remote_dir.join(&expected), expected_bytes).unwrap();
            let value = perform(&policy, &remote, &roots, compiled);
            let Term::Map(fields) = value.as_data().unwrap() else {
                panic!("expected sync response")
            };
            let count = if nested { 3 } else { 2 };
            assert_eq!(
                fields.get(&TermOrdKey(Term::symbol(":pulled"))),
                Some(&Term::Int(count.into()))
            );
            assert_eq!(
                fields.get(&TermOrdKey(Term::symbol(":present"))),
                Some(&Term::Int(0.into()))
            );
            assert_eq!(inventory(&local).len(), count as usize + 1);
            assert_eq!(store.get_bytes(&expected).unwrap(), expected_bytes);
            let value = perform(&policy, &remote, &roots, compiled);
            let Term::Map(fields) = value.as_data().unwrap() else {
                panic!("expected repeat response")
            };
            assert_eq!(
                fields.get(&TermOrdKey(Term::symbol(":pulled"))),
                Some(&Term::Int(0.into()))
            );
            assert_eq!(
                fields.get(&TermOrdKey(Term::symbol(":present"))),
                Some(&Term::Int(count.into()))
            );
        }
    }
}
