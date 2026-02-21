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

fn map_get<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else {
        return None;
    };
    m.get(&TermOrdKey(Term::symbol(key)))
}

#[cfg(unix)]
#[test]
fn editor_watch_ops_use_bridge_and_replay_deterministically() {
    use std::os::unix::fs::PermissionsExt;

    let td = tempfile::tempdir().expect("tempdir");
    let bridge = td.path().join("bridge.sh");
    std::fs::write(
        &bridge,
        r#"#!/bin/sh
if [ "$1" = "editor/watch::subscribe" ]; then
  resp='{:ok true :watch-id "watch-bridge-0"}'
elif [ "$1" = "editor/watch::poll" ]; then
  resp='{:events [{:kind :create :path "new.gc"}] :root "."}'
else
  resp='{:ok false :bridge-op "unexpected"}'
fi
printf '%s\n%s' "${#resp}" "$resp"
"#,
    )
    .expect("write bridge");
    let mut perms = std::fs::metadata(&bridge).expect("metadata").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bridge, perms).expect("chmod");

    let base = toml_escape(td.path().to_string_lossy().as_ref());
    let bridge_name = toml_escape(
        bridge
            .file_name()
            .and_then(|x| x.to_str())
            .expect("bridge name"),
    );
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["editor/watch::subscribe", "editor/watch::poll", "io/fs::write"]

[op."editor/watch::subscribe"]
base_dir = "{base}"
bridge_cmd = "{bridge_name}"

[op."editor/watch::poll"]
base_dir = "{base}"
bridge_cmd = "{bridge_name}"

[op."io/fs::write"]
base_dir = "{base}"
create_dirs = true
"#
    ))
    .expect("caps");

    let src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'editor/watch::subscribe
                                {:globs ["*.gc"] :root "."}
                                (fn (x) (core/effect::pure x))))
            (fn (watch-resp)
              (let ((watch-id ((core/map::get watch-resp) ':watch-id)))
                ((core/effect::bind (core/effect::perform
                                      'io/fs::write
                                      {:data b"(def y 2)\n" :path "new.gc"}
                                      (fn (x) (core/effect::pure x))))
                  (fn (_)
                    (core/effect::perform
                      'editor/watch::poll
                      {:watch-id watch-id}
                      (fn (x) (core/effect::pure x)))))))))
        prog
    "#;

    let mut ctx1 = EvalCtx::new();
    let (prog1, h) = parse_and_eval(&mut ctx1, src);
    let run_out = run(&mut ctx1, &policy, prog1, h, "gc_effects-test".to_string()).expect("run");

    let Value::Data(resp) = &run_out.value else {
        panic!("watch poll should return data map");
    };
    let Some(Term::Vector(events)) = map_get(resp, ":events") else {
        panic!("watch poll :events vector expected");
    };
    assert!(
        events.iter().any(|evt| {
            map_get(evt, ":kind") == Some(&Term::symbol(":create"))
                && map_get(evt, ":path") == Some(&Term::Str("new.gc".to_string()))
        }),
        "watch poll should report create for new.gc"
    );
    let mut ctx2 = EvalCtx::new();
    let (prog2, _) = parse_and_eval(&mut ctx2, src);
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
}

#[cfg(unix)]
#[test]
fn editor_plugin_command_uses_bridge_command_response() {
    use std::os::unix::fs::PermissionsExt;

    let td = tempfile::tempdir().expect("tempdir");
    let bridge = td.path().join("bridge.sh");
    std::fs::write(
        &bridge,
        r#"#!/bin/sh
if [ "$1" = "editor/plugin::command" ]; then
  resp='{:ok true :bridge-op "editor/plugin::command" :result {:status "ok"}}'
else
  resp='{:ok false :bridge-op "unexpected"}'
fi
printf '%s\n%s' "${#resp}" "$resp"
"#,
    )
    .expect("write bridge");
    let mut perms = std::fs::metadata(&bridge).expect("metadata").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bridge, perms).expect("chmod");

    let base = toml_escape(td.path().to_string_lossy().as_ref());
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["editor/plugin::command"]

[op."editor/plugin::command"]
base_dir = "{base}"
bridge_cmd = "bridge.sh"
allow_plugins = ["demo"]
allow_commands = ["run"]
"#
    ))
    .expect("caps");

    let src = r#"
        (def prog
          (core/effect::perform
            'editor/plugin::command
            {:command "run" :payload {:x 1} :plugin "demo"}
            (fn (x) (core/effect::pure x))))
        prog
    "#;

    let mut ctx1 = EvalCtx::new();
    let (prog1, h) = parse_and_eval(&mut ctx1, src);
    let run_out = run(&mut ctx1, &policy, prog1, h, "gc_effects-test".to_string()).expect("run");
    let Value::Data(resp) = &run_out.value else {
        panic!("plugin bridge should return data");
    };
    assert_eq!(
        map_get(resp, ":bridge-op"),
        Some(&Term::Str("editor/plugin::command".to_string()))
    );
    assert_eq!(map_get(resp, ":ok"), Some(&Term::Bool(true)));

    let mut ctx2 = EvalCtx::new();
    let (prog2, _) = parse_and_eval(&mut ctx2, src);
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
}

#[test]
fn editor_plugin_command_without_bridge_cmd_returns_error() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["editor/plugin::command"]

[op."editor/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
"#,
    )
    .expect("caps policy");
    let src = r#"
        (def prog
          (core/effect::perform
            'editor/plugin::command
            {:command "run" :payload {:x 1} :plugin "demo"}
            (fn (x) (core/effect::pure x))))
        prog
    "#;

    let mut ctx = EvalCtx::new();
    let (prog, h) = parse_and_eval(&mut ctx, src);
    let error_tok = ctx.protocol.expect("protocol").error;
    let run_out = run(&mut ctx, &policy, prog, h, "gc_effects-test".to_string()).expect("run");
    let Value::Sealed { token, payload } = run_out.value else {
        panic!("plugin command without bridge should return sealed error");
    };
    assert_eq!(token, error_tok);
    let Value::Data(Term::Map(mm)) = payload.as_ref() else {
        panic!("sealed error payload map expected");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("editor/bridge-required".to_string()))
    );
}

#[test]
fn host_plugin_command_wasi_profile_is_replay_deterministic() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status \"ok\" :bridge-op \"host/plugin::command\"}"
"#,
    )
    .expect("caps policy");
    let src = r#"
        (def prog
          (core/effect::perform
            'host/plugin::command
            {:command "run" :payload {:x 1} :plugin "demo"}
            (fn (x) (core/effect::pure x))))
        prog
    "#;

    let mut ctx1 = EvalCtx::new();
    let (prog1, h) = parse_and_eval(&mut ctx1, src);
    let run_out = run(&mut ctx1, &policy, prog1, h, "gc_effects-test".to_string()).expect("run");
    let Value::Data(resp) = &run_out.value else {
        panic!("host plugin command should return data");
    };
    assert_eq!(
        map_get(resp, ":bridge-op"),
        Some(&Term::Str("host/plugin::command".to_string()))
    );
    assert_eq!(map_get(resp, ":ok"), Some(&Term::Bool(true)));

    let mut ctx2 = EvalCtx::new();
    let (prog2, _) = parse_and_eval(&mut ctx2, src);
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
}

#[test]
fn host_plugin_command_requires_allowlisted_plugin() {
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["host/plugin::command"]

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true}"
"#,
    )
    .expect("caps policy");
    let src = r#"
        (def prog
          (core/effect::perform
            'host/plugin::command
            {:command "run" :payload {:x 1} :plugin "other"}
            (fn (x) (core/effect::pure x))))
        prog
    "#;

    let mut ctx = EvalCtx::new();
    let (prog, h) = parse_and_eval(&mut ctx, src);
    let error_tok = ctx.protocol.expect("protocol").error;
    let run_out = run(&mut ctx, &policy, prog, h, "gc_effects-test".to_string()).expect("run");
    let Value::Sealed { token, payload } = run_out.value else {
        panic!("host plugin command denied plugin should return sealed error");
    };
    assert_eq!(token, error_tok);
    let Value::Data(Term::Map(mm)) = payload.as_ref() else {
        panic!("sealed error payload map expected");
    };
    assert_eq!(
        mm.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("core/caps/policy-error".to_string()))
    );
}
