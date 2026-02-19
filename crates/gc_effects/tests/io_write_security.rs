use gc_coreform::{Term, hash_module, parse_module};
use gc_effects::{CapsPolicy, run};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn eval_prog(forms: &[Term]) -> (EvalCtx, Value) {
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, forms).expect("eval module");
    (ctx, prog)
}

#[cfg(unix)]
#[test]
fn io_fs_write_refuses_symlink_target_without_following_it() {
    let td = tempfile::tempdir().expect("tempdir");
    let sandbox = td.path().join("sandbox");
    std::fs::create_dir_all(&sandbox).expect("create sandbox");
    let sandbox_canon = std::fs::canonicalize(&sandbox).expect("canonical sandbox");
    let target = sandbox.join("target.txt");
    std::fs::write(&target, "original").expect("write target");
    let link = sandbox.join("link.txt");
    std::os::unix::fs::symlink(&target, &link).expect("create symlink");

    let caps_path = td.path().join("caps.toml");
    std::fs::write(
        &caps_path,
        format!(
            r#"
allow = ["io/fs::write"]

[op."io/fs::write"]
base_dir = "{}"
"#,
            sandbox_canon.display()
        ),
    )
    .expect("write caps");
    let pol = CapsPolicy::load(&caps_path).expect("load caps");

    let src = r#"
      (def prog
        (core/effect::perform
          'io/fs::write
          {:path "link.txt" :data "mutated"}
          (fn (r) (core/effect::pure r))))
      prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let mh = hash_module(&forms);
    let (mut ctx, prog) = eval_prog(&forms);
    let r = run(&mut ctx, &pol, prog, mh, "gc_effects-test".to_string()).expect("run");

    match r.value {
        Value::Sealed { .. } => {}
        other => panic!("expected sealed io error, got {}", other.debug_repr()),
    }
    assert_eq!(
        std::fs::read_to_string(&target).expect("read target"),
        "original",
        "symlink target should not be modified",
    );
}
