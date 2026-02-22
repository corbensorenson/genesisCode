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
        :media_hash (core/media::asset-hash "asset-bytes")
        :media_image_transcode ((((core/media::image-transcode "rgba8") "gray8") 2) 1 "ABCDEFGH")
        :media_audio_transcode ((((core/media::audio-transcode "pcm-s16le") "pcm-f32le") 1) 44100 "abcdefgh")
        :db_connect (core/db::connect "sqlite://data/app.db")
        :db_tx_begin (core/db::tx-begin "db-1")
        :db_query ((((core/db::query "db-1") (quote read-only)) "select 1") {})
        :db_exec ((((core/db::exec "db-1") (quote write)) "update kv set v=1 where k='a'") {})
        :db_tx_commit (core/db::tx-commit "tx-1")
        :db_tx_rollback (core/db::tx-rollback "tx-1")
        :db_kv_open (core/db::kv-open "kv://state/main")
        :db_kv_get ((core/db::kv-get "kv-1") "alpha")
        :db_kv_put (((core/db::kv-put "kv-1") "alpha") "v1")
        :db_kv_delete ((core/db::kv-delete "kv-1") "alpha")
        :net_dns_resolve (core/net::dns-resolve "example.test")
        :net_tcp_open (core/net::tcp-open "tcp://example.test:443")
        :net_tcp_listen (core/net::tcp-listen "tcp://127.0.0.1:9000")
        :net_tcp_accept (core/net::tcp-accept "listener-1")
        :net_tcp_send ((core/net::tcp-send "stream-1") "ping")
        :net_tcp_recv (core/net::tcp-recv "stream-1")
        :net_tcp_close (core/net::tcp-close "stream-1")
        :net_udp_bind (core/net::udp-bind "udp://127.0.0.1:4200")
        :net_udp_send (((core/net::udp-send "socket-1") "udp://127.0.0.1:4201") "data")
        :net_udp_recv (core/net::udp-recv "socket-1")
        :net_udp_close (core/net::udp-close "socket-1")
        :net_ws_open (core/net::ws-open "wss://example.test/ws")
        :net_http_listen (core/net::http-listen "http://127.0.0.1:8080")
        :net_http_respond ((((core/net::http-respond "listener-1") "request-1") 200) "ok")
        :net_ws_accept ((core/net::ws-accept "listener-1") "request-1")
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
        :plugin_host_typed_command
          (((((core/plugin::typed-command "demo") "run")
             "genesis/plugin.request.exec.v1")
            "genesis/plugin.response.result.v1")
           {:args ["--help"]})
        :plugin_editor_command (((core/plugin::editor-command "demo") "run") {:x 1})
        :plugin_editor_typed_command
          (((((core/plugin::typed-editor-command "demo") "run")
             "genesis/plugin.request.exec.v1")
            "genesis/plugin.response.result.v1")
           {:args ["--help"]})
        :editor_plugin_host_command (((core/editor/plugin::host-command "demo") "run") {:x 1})
        :editor_plugin_host_typed_command
          (((((core/editor/plugin::typed-host-command "demo") "run")
             "genesis/plugin.request.exec.v1")
            "genesis/plugin.response.result.v1")
           {:args ["--help"]})
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
    expect_op(":media_hash", "core/media::asset-hash");
    expect_op(":media_image_transcode", "core/media::image-transcode");
    expect_op(":media_audio_transcode", "core/media::audio-transcode");
    expect_op(":db_connect", "io/db::connect");
    expect_op(":db_tx_begin", "io/db::tx-begin");
    expect_op(":db_query", "io/db::query");
    expect_op(":db_exec", "io/db::exec");
    expect_op(":db_tx_commit", "io/db::tx-commit");
    expect_op(":db_tx_rollback", "io/db::tx-rollback");
    expect_op(":db_kv_open", "io/db::kv-open");
    expect_op(":db_kv_get", "io/db::kv-get");
    expect_op(":db_kv_put", "io/db::kv-put");
    expect_op(":db_kv_delete", "io/db::kv-delete");
    expect_op(":net_dns_resolve", "io/net::dns-resolve");
    expect_op(":net_tcp_open", "io/net::tcp-open");
    expect_op(":net_tcp_listen", "io/net::tcp-listen");
    expect_op(":net_tcp_accept", "io/net::tcp-accept");
    expect_op(":net_tcp_send", "io/net::tcp-send");
    expect_op(":net_tcp_recv", "io/net::tcp-recv");
    expect_op(":net_tcp_close", "io/net::tcp-close");
    expect_op(":net_udp_bind", "io/net::udp-bind");
    expect_op(":net_udp_send", "io/net::udp-send");
    expect_op(":net_udp_recv", "io/net::udp-recv");
    expect_op(":net_udp_close", "io/net::udp-close");
    expect_op(":net_ws_open", "io/net::ws-open");
    expect_op(":net_http_listen", "io/net::http-listen");
    expect_op(":net_http_respond", "io/net::http-respond");
    expect_op(":net_ws_accept", "io/net::ws-accept");
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
    expect_op(":plugin_host_typed_command", "host/plugin::command");
    expect_op(":plugin_editor_command", "editor/plugin::command");
    expect_op(":plugin_editor_typed_command", "editor/plugin::command");
    expect_op(":editor_plugin_host_command", "editor/plugin::command");
    expect_op(
        ":editor_plugin_host_typed_command",
        "editor/plugin::command",
    );
    expect_op(":gpu_create_kernel", "gpu/compute::create-kernel");
}

#[test]
fn typed_plugin_wrappers_emit_schema_fields() {
    let src = r#"
      {
        :typed_host
          (((((core/plugin::typed-command "demo") "run")
             "genesis/plugin.request.exec.v1")
            "genesis/plugin.response.result.v1")
           {:args ["--help"]})
        :typed_editor
          (((((core/plugin::typed-editor-command "demo") "run")
             "genesis/plugin.request.exec.v1")
            "genesis/plugin.response.result.v1")
           {:args ["--help"]})
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

    let host_req = get_req(
        m.get(&TermOrdKey(Term::symbol(":typed_host")))
            .expect("typed_host")
            .clone(),
    );
    let editor_req = get_req(
        m.get(&TermOrdKey(Term::symbol(":typed_editor")))
            .expect("typed_editor")
            .clone(),
    );

    let expect_schema_fields = |req: &EffectRequest| {
        let Term::Map(payload) = &req.payload else {
            panic!("expected payload map");
        };
        assert_eq!(
            payload.get(&TermOrdKey(Term::symbol(":request-schema-id"))),
            Some(&Term::Str("genesis/plugin.request.exec.v1".to_string()))
        );
        assert_eq!(
            payload.get(&TermOrdKey(Term::symbol(":response-schema-id"))),
            Some(&Term::Str("genesis/plugin.response.result.v1".to_string()))
        );
    };

    assert_eq!(host_req.op, "host/plugin::command");
    expect_schema_fields(&host_req);
    assert_eq!(editor_req.op, "editor/plugin::command");
    expect_schema_fields(&editor_req);
}
