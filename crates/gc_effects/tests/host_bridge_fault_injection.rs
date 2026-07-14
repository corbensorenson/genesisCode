use gc_coreform::{Term, TermOrdKey, hash_module, parse_module};
use gc_effects::{CapsPolicy, EffectLog, replay, run};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

fn sealed_error_code(value: &Value, error_tok: gc_kernel::SealId) -> Option<String> {
    let Value::Sealed { token, payload } = value else {
        return None;
    };
    if *token != error_tok {
        return None;
    }
    let Some(Term::Map(m)) = payload.as_ref().as_data() else {
        return None;
    };
    match m.get(&TermOrdKey(Term::symbol(":error/code"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn run_and_replay_code(src: &str, policy_toml: &str) -> (String, [u8; 32], [u8; 32]) {
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(policy_toml).expect("policy");

    let mut run_ctx = EvalCtx::new();
    let error_tok = run_ctx.protocol.expect("protocol").error;
    let mut run_env = build_prelude(&mut run_ctx).env;
    let run_prog = eval_module(&mut run_ctx, &mut run_env, &forms).expect("eval run");
    let run_out = run(
        &mut run_ctx,
        &policy,
        run_prog,
        h,
        "host-bridge-fault-injection".to_string(),
    )
    .expect("run");
    let run_hash = value_hash(&run_out.value);
    let code = sealed_error_code(&run_out.value, error_tok).unwrap_or_else(|| {
        panic!(
            "expected sealed error code, got {}",
            run_out.value.debug_repr()
        )
    });

    let replay_log = EffectLog::from_term(&run_out.log.to_term()).expect("decode log");
    let mut replay_ctx = EvalCtx::new();
    let mut replay_env = build_prelude(&mut replay_ctx).env;
    let replay_prog = eval_module(&mut replay_ctx, &mut replay_env, &forms).expect("eval replay");
    let replay_value = replay(&mut replay_ctx, replay_prog, &replay_log).expect("replay");
    let replay_hash = value_hash(&replay_value);

    (code, run_hash, replay_hash)
}

fn file_sha256_hex(path: &std::path::Path) -> String {
    use std::io::Read as _;

    let mut file = std::fs::File::open(path).expect("open bridge for hash");
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 8 * 1024];
    loop {
        let n = file.read(&mut buf).expect("read bridge for hash");
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    format!("{:x}", hasher.finalize())
}

#[test]
fn host_bridge_fault_injection_matrix_is_deterministic() {
    let tmp = tempdir().expect("tempdir");
    let bridge_path = tmp.path().join("bridge_fail.sh");
    let bridge_src = r#"#!/usr/bin/env sh
set -eu
IFS= read -r req_len || exit 42
dd bs=1 count="$req_len" status=none >/dev/null 2>/dev/null || true
exit 42
"#;
    fs::write(&bridge_path, bridge_src).expect("write bridge script");
    let mut perms = fs::metadata(&bridge_path)
        .expect("stat bridge")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&bridge_path, perms).expect("chmod bridge");
    let bridge = bridge_path.display().to_string();
    let bridge_sha256 = file_sha256_hex(&bridge_path);

    let fs_source = "fault-fs-source.txt";
    fs::write(fs_source, "fault-source").expect("write fs source");
    let fs_src = r#"
      (def prog (((core/fs::rename "fault-fs-source.txt") ".") false))
      prog
    "#;
    let fs_policy = r#"
allow = ["io/fs::rename"]
[op."io/fs::rename"]
base_dir = "."
"#;
    let (code, run_hash, replay_hash) = run_and_replay_code(fs_src, fs_policy);
    let _ = fs::remove_file(fs_source);
    assert!(
        code.starts_with("core/caps/"),
        "unexpected fs code prefix: {code}"
    );
    assert_eq!(run_hash, replay_hash, "fs fault injection replay mismatch");

    let net_src = r#"
      (def prog (core/net::tcp-open "tcp://example.test:443"))
      prog
    "#;
    let net_policy = format!(
        r#"
allow = ["io/net::tcp-open"]
[op."io/net::tcp-open"]
base_dir = "."
bridge_cmd = "{bridge}"
remote_allow = ["tcp://example.test:443"]
"#
    );
    let (code, run_hash, replay_hash) = run_and_replay_code(net_src, &net_policy);
    assert!(
        code.starts_with("net/bridge-"),
        "unexpected net code: {code}"
    );
    assert_eq!(run_hash, replay_hash, "net fault injection replay mismatch");

    let process_src = r#"
      (def prog (((core/process::spawn "echo") ["ok"]) {}))
      prog
    "#;
    let process_policy = format!(
        r#"
allow = ["sys/process::spawn"]
[op."sys/process::spawn"]
allow_programs = ["echo"]
base_dir = "."
bridge_cmd = "{bridge}"
"#
    );
    let (code, run_hash, replay_hash) = run_and_replay_code(process_src, &process_policy);
    assert!(
        code.starts_with("process/bridge-"),
        "unexpected process code: {code}"
    );
    assert_eq!(
        run_hash, replay_hash,
        "process fault injection replay mismatch"
    );

    let plugin_src = r#"
      (def prog (((core/plugin::command "demo") "run") {:x 1}))
      prog
    "#;
    let plugin_policy = format!(
        r#"
allow = ["host/plugin::command"]
[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
base_dir = "."
bridge_cmd = "{bridge}"
bridge_cmd_sha256 = "{bridge_sha256}"
"#
    );
    let (code, run_hash, replay_hash) = run_and_replay_code(plugin_src, &plugin_policy);
    assert!(
        code.starts_with("host/plugin/bridge-"),
        "unexpected plugin code: {code}"
    );
    assert_eq!(
        run_hash, replay_hash,
        "plugin fault injection replay mismatch"
    );
}
