use gc_coreform::{Term, TermOrdKey, hash_module, parse_module};
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

fn sealed_error_code(v: &Value) -> Option<String> {
    let Value::Sealed { payload, .. } = v else {
        return None;
    };
    let Value::Data(Term::Map(mm)) = payload.as_ref() else {
        return None;
    };
    match mm.get(&TermOrdKey(Term::symbol(":error/code"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn sealed_error_ctx(v: &Value) -> Option<&std::collections::BTreeMap<TermOrdKey, Term>> {
    let Value::Sealed { payload, .. } = v else {
        return None;
    };
    let Value::Data(Term::Map(mm)) = payload.as_ref() else {
        return None;
    };
    let Some(Term::Map(ctx)) = mm.get(&TermOrdKey(Term::symbol(":error/context"))) else {
        return None;
    };
    Some(ctx)
}

#[test]
fn untrusted_profile_denies_capability_abuse_by_default() {
    let td = tempfile::tempdir().unwrap();
    let caps_path = td.path().join("caps.toml");
    std::fs::write(&caps_path, "allow = []\n").unwrap();
    let policy = CapsPolicy::load(&caps_path).unwrap();

    let src = r#"
      (def prog
        (core/effect::perform
          'sys/process::exec
          {:program "sh" :args ["-c" "echo abuse"]}
          (fn (r) (core/effect::pure r))))
      prog
    "#;
    let forms = parse_module(src).unwrap();
    let hash = hash_module(&forms);
    let (mut ctx, prog) = eval_prog(&forms);
    let out = run(&mut ctx, &policy, prog, hash, "gc_effects-test".to_string()).unwrap();

    assert_eq!(
        sealed_error_code(&out.value).as_deref(),
        Some("core/caps/denied")
    );
}

#[test]
fn untrusted_profile_enforces_runtime_quota_on_effect_flood() {
    let td = tempfile::tempdir().unwrap();
    let caps_path = td.path().join("caps.toml");
    std::fs::write(
        &caps_path,
        r#"
allow = ["sys/time::now"]

[runtime]
max_effect_ops = 1
"#,
    )
    .unwrap();
    let policy = CapsPolicy::load(&caps_path).unwrap();

    let src = r#"
      (def prog
        (core/effect::perform
          'sys/time::now
          nil
          (fn (_)
            (core/effect::perform
              'sys/time::now
              nil
              (fn (r) (core/effect::pure r))))))
      prog
    "#;
    let forms = parse_module(src).unwrap();
    let hash = hash_module(&forms);
    let (mut ctx, prog) = eval_prog(&forms);
    let out = run(&mut ctx, &policy, prog, hash, "gc_effects-test".to_string()).unwrap();

    assert_eq!(
        sealed_error_code(&out.value).as_deref(),
        Some("core/caps/resource-limit")
    );
    let ctx_map = sealed_error_ctx(&out.value).expect("resource-limit error ctx");
    assert_eq!(
        ctx_map.get(&TermOrdKey(Term::symbol(":runtime/budget"))),
        Some(&Term::Str("max_effect_ops".to_string()))
    );
}

#[test]
fn untrusted_profile_blocks_host_escape_attempts_with_fs_sandbox() {
    let td = tempfile::tempdir().unwrap();
    let sandbox = td.path().join("sandbox");
    let outside = td.path().join("outside.txt");
    std::fs::create_dir_all(&sandbox).unwrap();
    std::fs::write(&outside, "secret").unwrap();

    let caps_path = td.path().join("caps.toml");
    let base_dir = sandbox.to_string_lossy().replace('\\', "/");
    std::fs::write(
        &caps_path,
        format!(
            r#"
allow = ["io/fs::read"]

[op."io/fs::read"]
base_dir = "{base_dir}"
"#
        ),
    )
    .unwrap();
    let policy = CapsPolicy::load(&caps_path).unwrap();

    let src = r#"
      (def prog
        (core/effect::perform
          'io/fs::read
          {:path "../outside.txt"}
          (fn (r) (core/effect::pure r))))
      prog
    "#;
    let forms = parse_module(src).unwrap();
    let hash = hash_module(&forms);
    let (mut ctx, prog) = eval_prog(&forms);
    let err = match run(&mut ctx, &policy, prog, hash, "gc_effects-test".to_string()) {
        Ok(_) => panic!("escape should fail at sandbox boundary"),
        Err(err) => err,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("escapes base_dir"),
        "unexpected sandbox error: {msg}"
    );
}
