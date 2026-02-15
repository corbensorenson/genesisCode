use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module, print_term};
use gc_effects::{CapsPolicy, Decision, RefsDb, run};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;
use tempfile::tempdir;

#[test]
fn selfhost_frontend_can_load_modules_from_commit_via_ref_and_store_get() {
    let td = tempdir().expect("tempdir");
    let store_dir = td.path().join(".genesis/store");
    let refs_path = td.path().join(".genesis/refs.coreform");
    std::fs::create_dir_all(&store_dir).expect("mkdir store");

    // Configure caps to allow store+refs reads and point to our temp locations.
    let caps = {
        let base_dir = td.path().to_string_lossy().to_string();
        CapsPolicy::from_toml_str(&format!(
            r#"
allow = ["core/store::get", "core/refs::get"]

[store]
dir = "{base_dir}/.genesis/store"

[refs]
path = "{base_dir}/.genesis/refs.coreform"
"#
        ))
        .expect("caps parse")
    };

    // Pre-populate the content-addressed store with:
    // - module artifact: vector of canonical forms
    // - package snapshot referencing that module
    // - empty patch artifact
    // - commit artifact referencing snapshot + patch
    let store = gc_effects::ArtifactStore::open(&store_dir).expect("open store");

    let module_src = r#"
      (def x 41)
      (prim int/add x 1)
    "#;
    let forms =
        canonicalize_module(parse_module(module_src).expect("parse module")).expect("canon module");
    let module_art = Term::Vector(forms);
    let module_hex = store
        .put_bytes(print_term(&module_art).as_bytes())
        .expect("store module");

    let snapshot_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::Symbol(":vcs/snapshot".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Symbol(":package".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":modules")),
                Term::Vector(vec![Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str("m.gc".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":hash")),
                            Term::Str(module_hex.clone()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                )]),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let snapshot_hex = store
        .put_bytes(print_term(&snapshot_term).as_bytes())
        .expect("store snapshot");

    let patch_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::Symbol(":vcs/patch".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":ops")), Term::Vector(Vec::new())),
        ]
        .into_iter()
        .collect(),
    );
    let patch_hex = store
        .put_bytes(print_term(&patch_term).as_bytes())
        .expect("store patch");

    let commit_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::Symbol(":vcs/commit".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":parents")),
                Term::Vector(Vec::new()),
            ),
            (TermOrdKey(Term::symbol(":base")), Term::Nil),
            (
                TermOrdKey(Term::symbol(":patch")),
                Term::Str(patch_hex.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":result")),
                Term::Str(snapshot_hex.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":obligations")),
                Term::Vector(Vec::new()),
            ),
            (
                TermOrdKey(Term::symbol(":evidence")),
                Term::Vector(Vec::new()),
            ),
            (
                TermOrdKey(Term::symbol(":attestations")),
                Term::Vector(Vec::new()),
            ),
            (
                TermOrdKey(Term::symbol(":message")),
                Term::Str("test".to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let commit_hex = store
        .put_bytes(print_term(&commit_term).as_bytes())
        .expect("store commit");

    // Set local ref -> commit.
    let rdb = RefsDb::open(&refs_path).expect("open refs db");
    rdb.set("refs/heads/main", Some(&commit_hex), None)
        .expect("set ref");

    // Evaluate loader module and run it as an effect program.
    let loader_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/frontend_v0.gc");
    let loader_src = std::fs::read_to_string(&loader_path).expect("read loader");
    let tool_src = format!(
        r#"
{loader}

(selfhost/frontend::load-root-modules "refs/heads/main")
        "#,
        loader = loader_src
    );
    let tool_forms =
        canonicalize_module(parse_module(&tool_src).expect("parse tool")).expect("canon tool");
    let program_h = gc_coreform::hash_module(&tool_forms);

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &tool_forms).expect("eval tool");
    assert!(matches!(prog, Value::EffectProgram(_)));

    let r = run(
        &mut ctx,
        &caps,
        prog,
        program_h,
        "gc_prelude-test".to_string(),
    )
    .expect("run");
    assert!(
        r.log.entries.iter().all(|e| e.decision == Decision::Allow),
        "expected allow decisions"
    );

    // Expect:
    // { :snapshot <snapshot_hex> :modules [ { :path "m.gc" :forms <module_art> :hash <module_hex> } ] }
    let Value::Map(out) = r.value else {
        panic!("expected map, got {}", r.value.debug_repr());
    };
    match out.get(&TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Value::Data(Term::Str(s))) => assert_eq!(s, &snapshot_hex),
        other => panic!("expected :snapshot string, got {other:?}"),
    }
    let Some(Value::Vector(mods)) = out.get(&TermOrdKey(Term::symbol(":modules"))) else {
        panic!("expected :modules vector");
    };
    assert_eq!(mods.len(), 1);
    let Value::Map(m0) = &mods[0] else {
        panic!("expected module map");
    };
    match m0.get(&TermOrdKey(Term::symbol(":path"))) {
        Some(Value::Data(Term::Str(s))) => assert_eq!(s, "m.gc"),
        other => panic!("expected :path string, got {other:?}"),
    }
    match m0.get(&TermOrdKey(Term::symbol(":hash"))) {
        Some(Value::Data(Term::Str(s))) => assert_eq!(s, &module_hex),
        other => panic!("expected :hash string, got {other:?}"),
    }
    match m0.get(&TermOrdKey(Term::symbol(":forms"))) {
        Some(Value::Data(t)) => assert_eq!(print_term(t), print_term(&module_art)),
        other => panic!("expected :forms datum, got {other:?}"),
    }
}
