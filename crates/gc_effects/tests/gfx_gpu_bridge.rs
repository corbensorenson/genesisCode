use gc_coreform::{Term, TermOrdKey, hash_module, parse_module};
use gc_effects::{CapsPolicy, replay, run};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;

fn parse_and_eval(ctx: &mut EvalCtx, src: &str) -> (Value, [u8; 32]) {
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let prelude = build_prelude(ctx);
    let mut env = prelude.env;
    let prog = eval_module(ctx, &mut env, &forms).expect("eval");
    (prog, h)
}

fn toml_escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(unix)]
fn write_bridge_script(dir: &tempfile::TempDir) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bridge = dir.path().join("host_bridge.sh");
    std::fs::write(
        &bridge,
        r#"#!/bin/sh
resp='{:ok true :surface "surface-bridge-0" :id "gpu-bridge-0" :width 800 :height 600 :title "bridge" :events [] :data b"" :features [] :queued 0 :pending-redraws 0}'
printf '%s\n%s' "${#resp}" "$resp"
"#,
    )
    .expect("write bridge");
    let mut perms = std::fs::metadata(&bridge).expect("metadata").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bridge, perms).expect("chmod");
    bridge
}

#[cfg(unix)]
fn write_sleep_bridge_script(dir: &tempfile::TempDir) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bridge = dir.path().join("sleep_bridge.sh");
    std::fs::write(
        &bridge,
        r#"#!/bin/sh
sleep 1
resp='{:ok true}'
printf '%s\n%s' "${#resp}" "$resp"
"#,
    )
    .expect("write sleep bridge");
    let mut perms = std::fs::metadata(&bridge).expect("metadata").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bridge, perms).expect("chmod");
    bridge
}

#[cfg(windows)]
fn write_bridge_script(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let bridge = dir.path().join("host_bridge.cmd");
    std::fs::write(
        &bridge,
        "@echo {:ok true :surface \"surface-bridge-0\" :id \"gpu-bridge-0\" :width 800 :height 600 :title \"bridge\" :events [] :data b\"\" :features [] :queued 0 :pending-redraws 0}\r\n",
    )
    .expect("write bridge");
    bridge
}

#[cfg(windows)]
fn write_sleep_bridge_script(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let bridge = dir.path().join("sleep_bridge.cmd");
    std::fs::write(&bridge, "@echo {:ok true}\r\n").expect("write sleep bridge");
    bridge
}

#[test]
fn gfx_and_gpu_ops_require_bridge_policy() {
    let policy = CapsPolicy::from_toml_str(
        r#"allow = ["gfx/window::create-surface", "gfx/gpu::limits", "gpu/compute::limits"]"#,
    )
    .expect("caps");
    let src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'gfx/window::create-surface
                                {:opts {:height 600 :title "main" :width 800}}
                                (fn (x) (core/effect::pure x))))
            (fn (_)
              ((core/effect::bind (core/effect::perform
                                    'gfx/gpu::limits
                                    {}
                                    (fn (x) (core/effect::pure x))))
                (fn (_)
                  (core/effect::perform
                    'gpu/compute::limits
                    {}
                    (fn (x) (core/effect::pure x))))))))
        prog
    "#;
    let mut ctx = EvalCtx::new();
    let (prog, h) = parse_and_eval(&mut ctx, src);
    let error_tok = ctx.protocol.expect("protocol").error;
    let out = run(&mut ctx, &policy, prog, h, "gc_effects-test".to_string()).expect("run");
    let Value::Sealed { token, payload } = out.value else {
        panic!("missing bridge policy should return sealed error");
    };
    assert_eq!(token, error_tok);
    let Value::Data(Term::Map(mm)) = payload.as_ref() else {
        panic!("sealed error map expected");
    };
    let Some(Term::Str(code)) = mm.get(&TermOrdKey(Term::symbol(":error/code"))) else {
        panic!("error code expected");
    };
    assert!(
        code.starts_with("gfx/bridge-") || code.starts_with("gpu/bridge-"),
        "unexpected bridge error code: {code}"
    );
}

#[test]
fn gfx_and_gpu_ops_bridge_roundtrip_is_replay_deterministic() {
    let td = tempfile::tempdir().expect("tempdir");
    let bridge = write_bridge_script(&td);
    let base = toml_escape(td.path().to_string_lossy().as_ref());
    let bridge_name = toml_escape(
        bridge
            .file_name()
            .and_then(|x| x.to_str())
            .expect("bridge filename"),
    );
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gfx/window::create-surface", "gfx/input::poll-events", "gfx/audio::enqueue", "gfx/gpu::create-buffer", "gpu/compute::limits"]

[op."gfx/window::create-surface"]
base_dir = "{base}"
bridge_cmd = "{bridge_name}"

[op."gfx/input::poll-events"]
base_dir = "{base}"
bridge_cmd = "{bridge_name}"

[op."gfx/audio::enqueue"]
base_dir = "{base}"
bridge_cmd = "{bridge_name}"

[op."gfx/gpu::create-buffer"]
base_dir = "{base}"
bridge_cmd = "{bridge_name}"

[op."gpu/compute::limits"]
base_dir = "{base}"
bridge_cmd = "{bridge_name}"
"#
    ))
    .expect("caps");
    let src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'gfx/window::create-surface
                                {:opts {:height 600 :title "main" :width 800}}
                                (fn (x) (core/effect::pure x))))
            (fn (surface-resp)
              (let ((sid ((core/map::get surface-resp) ':surface)))
                ((core/effect::bind (core/effect::perform
                                      'gfx/input::poll-events
                                      {:surface sid}
                                      (fn (x) (core/effect::pure x))))
                  (fn (_)
                    ((core/effect::bind (core/effect::perform
                                          'gfx/audio::enqueue
                                          {:event {:kind "beep"}}
                                          (fn (x) (core/effect::pure x))))
                      (fn (_)
                        ((core/effect::bind (core/effect::perform
                                              'gfx/gpu::create-buffer
                                              {:desc {:size 8}}
                                              (fn (x) (core/effect::pure x))))
                          (fn (_)
                            (core/effect::perform
                              'gpu/compute::limits
                              {}
                              (fn (x) (core/effect::pure x)))))))))))))
        prog
    "#;
    let mut ctx1 = EvalCtx::new();
    let (prog1, h) = parse_and_eval(&mut ctx1, src);
    let out = run(&mut ctx1, &policy, prog1, h, "gc_effects-test".to_string()).expect("run");
    assert!(!matches!(out.value, Value::Sealed { .. }));
    let mut ctx2 = EvalCtx::new();
    let (prog2, _) = parse_and_eval(&mut ctx2, src);
    let replay_v = replay(&mut ctx2, prog2, &out.log).expect("replay");
    assert_eq!(value_hash(&out.value), value_hash(&replay_v));
}

#[cfg(unix)]
#[test]
fn gfx_bridge_timeout_is_reported_deterministically() {
    let td = tempfile::tempdir().expect("tempdir");
    let bridge = write_sleep_bridge_script(&td);
    let base = toml_escape(td.path().to_string_lossy().as_ref());
    let bridge_name = toml_escape(
        bridge
            .file_name()
            .and_then(|x| x.to_str())
            .expect("bridge filename"),
    );
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gfx/window::create-surface"]

[op."gfx/window::create-surface"]
base_dir = "{base}"
bridge_cmd = "{bridge_name}"
timeout_ms = 10
"#
    ))
    .expect("caps");
    let src = r#"
        (def prog
          (core/effect::perform
            'gfx/window::create-surface
            {:opts {:height 600 :title "main" :width 800}}
            (fn (x) (core/effect::pure x))))
        prog
    "#;
    let mut ctx = EvalCtx::new();
    let (prog, h) = parse_and_eval(&mut ctx, src);
    let error_tok = ctx.protocol.expect("protocol").error;
    let out = run(&mut ctx, &policy, prog, h, "gc_effects-test".to_string()).expect("run");
    let Value::Sealed { token, payload } = out.value else {
        panic!("timeout must return sealed error");
    };
    assert_eq!(token, error_tok);
    let Value::Data(Term::Map(mm)) = payload.as_ref() else {
        panic!("sealed error map expected");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("gfx/bridge-timeout".to_string()))
    );
}

#[test]
fn gfx_bridge_max_bytes_is_enforced_for_request_payload() {
    let td = tempfile::tempdir().expect("tempdir");
    let bridge = write_bridge_script(&td);
    let base = toml_escape(td.path().to_string_lossy().as_ref());
    let bridge_name = toml_escape(
        bridge
            .file_name()
            .and_then(|x| x.to_str())
            .expect("bridge filename"),
    );
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["gfx/window::create-surface"]

[op."gfx/window::create-surface"]
base_dir = "{base}"
bridge_cmd = "{bridge_name}"
max_bytes = 8
"#
    ))
    .expect("caps");
    let src = r#"
        (def prog
          (core/effect::perform
            'gfx/window::create-surface
            {:opts {:height 600 :title "this-title-is-way-too-long-for-max-bytes" :width 800}}
            (fn (x) (core/effect::pure x))))
        prog
    "#;
    let mut ctx = EvalCtx::new();
    let (prog, h) = parse_and_eval(&mut ctx, src);
    let error_tok = ctx.protocol.expect("protocol").error;
    let out = run(&mut ctx, &policy, prog, h, "gc_effects-test".to_string()).expect("run");
    let Value::Sealed { token, payload } = out.value else {
        panic!("max-bytes must return sealed error");
    };
    assert_eq!(token, error_tok);
    let Value::Data(Term::Map(mm)) = payload.as_ref() else {
        panic!("sealed error map expected");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("gfx/bridge-payload-too-large".to_string()))
    );
}

#[test]
fn wasi_bridge_profile_gpu_compute_roundtrip_is_replay_deterministic() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["gpu/compute::limits"]

[op."gpu/compute::limits"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :source :wasi-profile}"
"#,
    )
    .expect("caps");
    let src = r#"
        (def prog
          (core/effect::perform
            'gpu/compute::limits
            {}
            (fn (x) (core/effect::pure x))))
        prog
    "#;
    let mut ctx1 = EvalCtx::new();
    let (prog1, h) = parse_and_eval(&mut ctx1, src);
    let out = run(&mut ctx1, &policy, prog1, h, "gc_effects-test".to_string()).expect("run");
    let Value::Data(Term::Map(mm)) = &out.value else {
        panic!("wasi bridge profile response should be data map");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":source"))),
        Some(&Term::symbol(":wasi-profile"))
    );

    let mut ctx2 = EvalCtx::new();
    let (prog2, _) = parse_and_eval(&mut ctx2, src);
    let replay_v = replay(&mut ctx2, prog2, &out.log).expect("replay");
    assert_eq!(value_hash(&out.value), value_hash(&replay_v));
}
