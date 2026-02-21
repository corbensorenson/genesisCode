use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{EffectProgram, EffectRequest, EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn get_req(v: Value) -> EffectRequest {
    let Value::EffectProgram(p) = v else {
        panic!("expected effect program, got {}", v.debug_repr());
    };
    let EffectProgram::Perform { request } = p.as_ref() else {
        panic!("expected perform");
    };
    let Value::Sealed { payload, .. } = request.as_ref() else {
        panic!("expected sealed request");
    };
    let Value::EffectRequest(req) = payload.as_ref() else {
        panic!("expected effect request payload");
    };
    req.clone()
}

#[test]
fn low_level_caps_wrappers_emit_expected_ops() {
    let src = r#"
      {
        :store_verify (core/store::verify "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        :pkg_load_lock (core/pkg::load-lock "genesis.lock")
        :pkg_load_package (core/pkg::load-package "package.toml")
        :pkg_save_lock
          (core/pkg::save-lock
            {
              :lock "genesis.lock"
              :workspace "agent"
              :policy "policy:default-v0.1"
              :requirements {}
              :locked {}
              :registries {}
              :artifacts {}
            })
        :vcs_diff_terms ((core/vcs::diff-terms {:type "demo/base"}) {:type "demo/to"})
        :vcs_apply_patch ((core/vcs::apply-patch {:type "demo/base"}) {:type "vcs/patch" :ops []})
        :vcs_merge3_contract_snapshots
          (((core/vcs::merge3-contract-snapshots {:type "demo/base"}) {:type "demo/left"}) {:type "demo/right"})
        :fs_read (core/fs::read "tmp/in.txt")
        :fs_write ((core/fs::write "tmp/out.txt") "hello")
        :fs_stat (core/fs::stat "tmp/out.txt")
        :fs_list (core/fs::list "tmp")
        :fs_mkdir ((core/fs::mkdir "tmp/nested") true)
        :fs_remove ((core/fs::remove "tmp/old") true)
        :fs_rename (((core/fs::rename "tmp/from.txt") "tmp/to.txt") true)
        :net_dns_resolve (core/net::dns-resolve "example.test")
        :net_tcp_open (core/net::tcp-open "tcp://example.test:443")
        :net_tcp_send ((core/net::tcp-send "stream-1") "ping")
        :net_tcp_recv (core/net::tcp-recv "stream-1")
        :net_tcp_close (core/net::tcp-close "stream-1")
        :net_udp_bind (core/net::udp-bind "udp://127.0.0.1:4200")
        :net_udp_send (((core/net::udp-send "socket-1") "udp://127.0.0.1:4201") "data")
        :net_udp_recv (core/net::udp-recv "socket-1")
        :net_udp_close (core/net::udp-close "socket-1")
        :net_ws_open (core/net::ws-open "wss://example.test/ws")
        :net_ws_send ((core/net::ws-send "ws-1") "frame")
        :net_ws_recv (core/net::ws-recv "ws-1")
        :net_ws_close (core/net::ws-close "ws-1")
        :process_spawn (((core/process::spawn "echo") ["hello"]) {})
        :process_wait (core/process::wait "proc-1")
        :process_kill (core/process::kill "proc-1")
        :process_stdout_read (core/process::stdout-read "proc-1")
        :process_stderr_read (core/process::stderr-read "proc-1")
        :process_stdin_write ((core/process::stdin-write "proc-1") "stdin")
        :time_now (core/time::now nil)
        :plugin_host_command (((core/plugin::command "demo") "run") {:x 1})
        :plugin_editor_command (((core/plugin::editor-command "demo") "run") {:x 1})
        :editor_plugin_host_command (((core/editor/plugin::host-command "demo") "run") {:x 1})
        :gpu_create_kernel (core/gpu/compute::create-kernel {:label "kernel"})
      }
    "#;

    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canonicalize");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let value = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let Value::Map(m) = value else {
        panic!("expected map");
    };

    let expect_op = |key: &str, op: &str| {
        let req = get_req(
            m.get(&TermOrdKey(Term::symbol(key)))
                .unwrap_or_else(|| panic!("missing key {key}"))
                .clone(),
        );
        assert_eq!(req.op, op, "wrapper key {key} op mismatch");
    };

    expect_op(":store_verify", "core/store::verify");
    expect_op(":pkg_load_lock", "core/pkg-low::load-lock");
    expect_op(":pkg_load_package", "core/pkg-low::load-package");
    expect_op(":pkg_save_lock", "core/pkg-low::save-lock");
    expect_op(":vcs_diff_terms", "core/vcs-low::diff-terms");
    expect_op(":vcs_apply_patch", "core/vcs-low::apply-patch");
    expect_op(
        ":vcs_merge3_contract_snapshots",
        "core/vcs-low::merge3-contract-snapshots",
    );
    expect_op(":fs_read", "io/fs::read");
    expect_op(":fs_write", "io/fs::write");
    expect_op(":fs_stat", "io/fs::stat");
    expect_op(":fs_list", "io/fs::list");
    expect_op(":fs_mkdir", "io/fs::mkdir");
    expect_op(":fs_remove", "io/fs::remove");
    expect_op(":fs_rename", "io/fs::rename");
    expect_op(":net_dns_resolve", "io/net::dns-resolve");
    expect_op(":net_tcp_open", "io/net::tcp-open");
    expect_op(":net_tcp_send", "io/net::tcp-send");
    expect_op(":net_tcp_recv", "io/net::tcp-recv");
    expect_op(":net_tcp_close", "io/net::tcp-close");
    expect_op(":net_udp_bind", "io/net::udp-bind");
    expect_op(":net_udp_send", "io/net::udp-send");
    expect_op(":net_udp_recv", "io/net::udp-recv");
    expect_op(":net_udp_close", "io/net::udp-close");
    expect_op(":net_ws_open", "io/net::ws-open");
    expect_op(":net_ws_send", "io/net::ws-send");
    expect_op(":net_ws_recv", "io/net::ws-recv");
    expect_op(":net_ws_close", "io/net::ws-close");
    expect_op(":process_spawn", "sys/process::spawn");
    expect_op(":process_wait", "sys/process::wait");
    expect_op(":process_kill", "sys/process::kill");
    expect_op(":process_stdout_read", "sys/process::stdout-read");
    expect_op(":process_stderr_read", "sys/process::stderr-read");
    expect_op(":process_stdin_write", "sys/process::stdin-write");
    expect_op(":time_now", "sys/time::now");
    expect_op(":plugin_host_command", "host/plugin::command");
    expect_op(":plugin_editor_command", "editor/plugin::command");
    expect_op(":editor_plugin_host_command", "editor/plugin::command");
    expect_op(":gpu_create_kernel", "gpu/compute::create-kernel");
}
