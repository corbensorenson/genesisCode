use std::path::Path;

pub(crate) const CAPS_DEV_DEFAULT: &str = "allow = []\n";
pub(crate) const CAPS_CI_DEFAULT: &str = "allow = []\n";
pub(crate) const CAPS_RELEASE_DEFAULT: &str = "allow = []\n";

const BACKEND_ALLOW_OPS: &[&str] = &[
    "io/net::http-request",
    "io/net::dns-resolve",
    "io/net::tcp-open",
    "io/net::tcp-send",
    "io/net::tcp-recv",
    "io/net::tcp-close",
    "io/net::udp-send",
    "io/net::udp-recv",
    "io/net::udp-close",
    "io/net::ws-open",
    "io/net::ws-send",
    "io/net::ws-recv",
    "io/net::ws-close",
    "io/net::tcp-listen",
    "io/net::tcp-accept",
    "io/net::http-listen",
    "io/net::http-respond",
    "io/net::udp-bind",
    "io/net::ws-accept",
    "io/db::connect",
    "io/db::tx-begin",
    "io/db::query",
    "io/db::exec",
    "io/db::tx-commit",
    "io/db::tx-rollback",
    "io/db::kv-open",
    "io/db::kv-get",
    "io/db::kv-put",
    "io/db::kv-delete",
    "sys/process::exec",
    "sys/process::spawn",
    "sys/process::wait",
    "sys/process::kill",
    "sys/process::stdout-read",
    "sys/process::stderr-read",
    "sys/process::stdin-write",
    "core/crypto::hash",
    "core/crypto::sign",
    "core/crypto::verify",
    "core/crypto::kdf",
    "core/crypto::aead-seal",
    "core/crypto::aead-open",
    "host/plugin::command",
    "editor/plugin::command",
    "host/ffi::call",
    "host/ffi::buffer-pin",
    "host/ffi::buffer-unpin",
    "editor/clipboard::get",
    "editor/clipboard::set",
    "editor/dialog::open",
    "editor/dialog::save",
    "editor/watch::subscribe",
    "editor/watch::poll",
    "editor/watch::unsubscribe",
    "editor/task::spawn",
    "editor/task::poll",
    "editor/task::cancel",
    "editor/task::fmt-coreform",
    "editor/task::lint-module",
    "editor/task::optimize-module",
    "editor/task::parse-module",
    "editor/task::test-pkg",
    "editor/task::typecheck-pkg",
    "gfx/window::create-surface",
    "gfx/window::resize-surface",
    "gfx/window::set-title",
    "gfx/window::request-redraw",
    "gfx/window::surface-info",
    "gfx/input::poll-events",
    "gfx/input::set-cursor-mode",
    "gfx/audio::set-master",
    "gfx/audio::enqueue",
    "gpu/compute::create-buffer",
    "gpu/compute::create-shader-module",
    "gpu/compute::create-bind-group-layout",
    "gpu/compute::create-bind-group",
    "gpu/compute::create-pipeline-layout",
    "gpu/compute::create-compute-pipeline",
    "gpu/compute::create-kernel",
    "gpu/compute::write-buffer",
    "gpu/compute::read-buffer",
    "gpu/compute::submit",
    "gpu/compute::destroy-resource",
    "gpu/compute::limits",
    "gpu/compute::features",
    "gfx/gpu::create-buffer",
    "gfx/gpu::create-texture",
    "gfx/gpu::create-sampler",
    "gfx/gpu::create-shader-module",
    "gfx/gpu::create-bind-group-layout",
    "gfx/gpu::create-bind-group",
    "gfx/gpu::create-pipeline-layout",
    "gfx/gpu::create-render-pipeline",
    "gfx/gpu::write-buffer",
    "gfx/gpu::write-texture",
    "gfx/gpu::read-buffer",
    "gfx/gpu::read-texture",
    "gfx/gpu::submit-frame-graph",
    "gfx/gpu::destroy-resource",
    "gfx/gpu::limits",
    "gfx/gpu::features",
];

pub(crate) fn render_backend_caps_policy(
    bridge_cmd: Option<&Path>,
    bridge_cmd_sha256: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str("allow = [\n");
    for op in BACKEND_ALLOW_OPS {
        out.push_str("  \"");
        out.push_str(op);
        out.push_str("\",\n");
    }
    out.push_str("]\n\n");

    append_net_policy(&mut out, bridge_cmd, bridge_cmd_sha256);
    append_db_policy(&mut out, bridge_cmd, bridge_cmd_sha256);
    append_process_policy(&mut out, bridge_cmd, bridge_cmd_sha256);
    append_crypto_policy(&mut out, bridge_cmd, bridge_cmd_sha256);
    append_plugin_policy(&mut out, bridge_cmd, bridge_cmd_sha256);
    append_ffi_policy(&mut out, bridge_cmd, bridge_cmd_sha256);
    append_editor_first_party_policy(&mut out);
    append_gfx_gpu_first_party_policy(&mut out);

    out
}

fn append_net_policy(out: &mut String, bridge_cmd: Option<&Path>, bridge_cmd_sha256: Option<&str>) {
    for op in [
        "io/net::http-request",
        "io/net::dns-resolve",
        "io/net::tcp-open",
        "io/net::tcp-send",
        "io/net::tcp-recv",
        "io/net::tcp-close",
        "io/net::udp-send",
        "io/net::udp-recv",
        "io/net::udp-close",
        "io/net::ws-open",
        "io/net::ws-send",
        "io/net::ws-recv",
        "io/net::ws-close",
        "io/net::tcp-listen",
        "io/net::tcp-accept",
        "io/net::http-listen",
        "io/net::http-respond",
        "io/net::udp-bind",
        "io/net::ws-accept",
    ] {
        append_op_header(out, op);
        out.push_str("url_allow = [\"*\"]\n");
        out.push_str("allow_http = true\n");
        out.push_str("wasi_network_profile = \"preview2\"\n");
        if matches!(
            op,
            "io/net::tcp-listen" | "io/net::http-listen" | "io/net::udp-bind"
        ) {
            out.push_str("allow_bind_hosts = [\"*\"]\n");
            out.push_str("allow_bind_ports = [\"*\"]\n");
        }
        if matches!(
            op,
            "io/net::tcp-accept" | "io/net::http-listen" | "io/net::ws-accept"
        ) {
            out.push_str("max_request_bytes = 1048576\n");
        }
        append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
        out.push('\n');
    }
}

fn append_db_policy(out: &mut String, bridge_cmd: Option<&Path>, bridge_cmd_sha256: Option<&str>) {
    for op in ["io/db::connect", "io/db::kv-open"] {
        append_op_header(out, op);
        out.push_str("db_target_allow = [\"*\"]\n");
        append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
        out.push('\n');
    }
    append_op_header(out, "io/db::query");
    out.push_str("allow_query_classes = [\"*\"]\n");
    out.push_str("max_result_bytes = 8388608\n");
    out.push_str("max_row_count = 200000\n");
    append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
    out.push('\n');

    append_op_header(out, "io/db::exec");
    out.push_str("allow_query_classes = [\"*\"]\n");
    out.push_str("max_result_bytes = 1048576\n");
    append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
    out.push('\n');

    append_op_header(out, "io/db::kv-get");
    out.push_str("max_result_bytes = 8388608\n");
    append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
    out.push('\n');

    append_op_header(out, "io/db::kv-put");
    out.push_str("max_value_bytes = 8388608\n");
    append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
    out.push('\n');

    for op in [
        "io/db::tx-begin",
        "io/db::tx-commit",
        "io/db::tx-rollback",
        "io/db::kv-delete",
    ] {
        append_op_header(out, op);
        append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
        out.push('\n');
    }
}

fn append_process_policy(
    out: &mut String,
    bridge_cmd: Option<&Path>,
    bridge_cmd_sha256: Option<&str>,
) {
    for op in ["sys/process::exec", "sys/process::spawn"] {
        append_op_header(out, op);
        out.push_str("allow_programs = [\"*\"]\n");
        append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
        out.push('\n');
    }
    for op in [
        "sys/process::wait",
        "sys/process::kill",
        "sys/process::stdout-read",
        "sys/process::stderr-read",
        "sys/process::stdin-write",
    ] {
        append_op_header(out, op);
        append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
        out.push('\n');
    }
}

fn append_crypto_policy(
    out: &mut String,
    bridge_cmd: Option<&Path>,
    bridge_cmd_sha256: Option<&str>,
) {
    append_op_header(out, "core/crypto::hash");
    out.push_str("allow_algorithms = [\"*\"]\n");
    out.push_str("max_input_bytes = 16777216\n");
    append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
    out.push('\n');

    for op in ["core/crypto::sign", "core/crypto::verify"] {
        append_op_header(out, op);
        out.push_str("allow_algorithms = [\"*\"]\n");
        out.push_str("allow_key_ids = [\"*\"]\n");
        out.push_str("max_message_bytes = 16777216\n");
        out.push_str("max_signature_bytes = 16384\n");
        out.push_str("max_context_bytes = 4096\n");
        append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
        out.push('\n');
    }

    append_op_header(out, "core/crypto::kdf");
    out.push_str("allow_algorithms = [\"*\"]\n");
    out.push_str("allow_key_ids = [\"*\"]\n");
    out.push_str("max_info_bytes = 4096\n");
    out.push_str("max_output_bytes = 1048576\n");
    append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
    out.push('\n');

    for op in ["core/crypto::aead-seal", "core/crypto::aead-open"] {
        append_op_header(out, op);
        out.push_str("allow_algorithms = [\"*\"]\n");
        out.push_str("allow_key_ids = [\"*\"]\n");
        out.push_str("max_plaintext_bytes = 16777216\n");
        out.push_str("max_ciphertext_bytes = 16793600\n");
        out.push_str("max_aad_bytes = 1048576\n");
        out.push_str("max_nonce_bytes = 64\n");
        append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
        out.push('\n');
    }
}

fn append_plugin_policy(
    out: &mut String,
    bridge_cmd: Option<&Path>,
    bridge_cmd_sha256: Option<&str>,
) {
    for op in ["host/plugin::command", "editor/plugin::command"] {
        append_op_header(out, op);
        out.push_str("allow_plugins = [\"*\"]\n");
        out.push_str("allow_commands = [\"*\"]\n");
        append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
        out.push('\n');
    }
}

fn append_ffi_policy(out: &mut String, bridge_cmd: Option<&Path>, bridge_cmd_sha256: Option<&str>) {
    append_op_header(out, "host/ffi::call");
    out.push_str("allow_abi_ids = [\"*\"]\n");
    out.push_str("allow_libraries = [\"*\"]\n");
    out.push_str("allow_symbols = [\"*\"]\n");
    append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
    out.push('\n');

    for op in ["host/ffi::buffer-pin", "host/ffi::buffer-unpin"] {
        append_op_header(out, op);
        out.push_str("allow_abi_ids = [\"*\"]\n");
        out.push_str("max_buffer_bytes = 16777216\n");
        append_bridge_policy(out, bridge_cmd, bridge_cmd_sha256);
        out.push('\n');
    }
}

fn append_editor_first_party_policy(out: &mut String) {
    append_op_header(out, "editor/task::spawn");
    out.push_str("first_party_profile = \"headless\"\n\n");
}

fn append_gfx_gpu_first_party_policy(out: &mut String) {
    append_op_header(out, "gfx/window::create-surface");
    out.push_str("first_party_profile = \"headless\"\n\n");

    append_op_header(out, "gpu/compute::limits");
    out.push_str("gpu_backend = \"first-party-runtime\"\n\n");
}

fn append_op_header(out: &mut String, op: &str) {
    out.push_str("[op.\"");
    out.push_str(op);
    out.push_str("\"]\n");
}

fn append_bridge_policy(
    out: &mut String,
    bridge_cmd: Option<&Path>,
    bridge_cmd_sha256: Option<&str>,
) {
    if let Some(path) = bridge_cmd {
        out.push_str("bridge_cmd = \"");
        out.push_str(&path_to_slash(path));
        out.push_str("\"\n");
    }
    if let Some(digest) = bridge_cmd_sha256 {
        out.push_str("bridge_cmd_sha256 = \"sha256:");
        out.push_str(digest.trim_start_matches("sha256:"));
        out.push_str("\"\n");
    }
}

fn path_to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
