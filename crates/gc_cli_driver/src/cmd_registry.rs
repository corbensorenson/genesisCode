use super::*;

pub(super) fn cmd_registry(
    cli: &Cli,
    flavor: Flavor,
    cmd: &RegistryCmd,
) -> Result<CmdOut, CliError> {
    match cmd {
        RegistryCmd::Serve {
            addr,
            root,
            max_chunk_bytes,
            max_requests,
        } => cmd_registry_serve(cli, flavor, addr, root, *max_chunk_bytes, *max_requests),
    }
}

fn cmd_registry_serve(
    cli: &Cli,
    flavor: Flavor,
    addr: &str,
    root: &Path,
    max_chunk_bytes: u64,
    max_requests: Option<u64>,
) -> Result<CmdOut, CliError> {
    if matches!(flavor, Flavor::Wasi) {
        return Err(cli_err(
            EX_VERIFY,
            "registry/unsupported",
            "registry serve is not supported in WASI binaries; run native `genesis registry serve`",
        ));
    }
    if max_chunk_bytes == 0 {
        return Err(cli_err(
            EX_PARSE,
            "registry/config",
            "--max-chunk-bytes must be greater than 0",
        ));
    }

    let cfg = gc_registry::HttpRegistryServerConfig {
        addr: addr.to_string(),
        root: root.to_path_buf(),
        max_chunk_bytes,
        max_requests,
    };
    let handle = gc_registry::spawn_http_file_registry_server(cfg)
        .map_err(|e| cli_err(EX_INTERNAL, "registry/serve", format!("{e}")))?;

    let bound_addr = handle.bound_addr().to_string();
    // Blocking serve loop. For tests/use-cases that need deterministic completion, pass
    // --max-requests so the server exits after N handled requests.
    handle
        .join()
        .map_err(|e| cli_err(EX_INTERNAL, "registry/serve", format!("{e}")))?;

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/registry-serve-v0.1",
        data: Some(serde_json::json!({
            "bound_addr": bound_addr,
            "root": root.display().to_string(),
            "max_chunk_bytes": max_chunk_bytes,
            "max_requests": max_requests,
            "status": "stopped",
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("registry serve stopped {bound_addr}\n")
        },
        json: json_envelope_value(env)?,
    })
}
