use super::*;
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Output, Stdio};

const PLUGIN_REQUEST_EXEC_V1: &str = "genesis/plugin.request.exec.v1";
const PLUGIN_REQUEST_JSONRPC_V1: &str = "genesis/plugin.request.jsonrpc.v1";
const PLUGIN_RESPONSE_BYTES_V1: &str = "genesis/plugin.response.bytes.v1";
const FFI_REQUEST_CALL_V1: &str = "genesis/ffi.request.call.v1";
const FFI_RESPONSE_CALL_V1: &str = "genesis/ffi.response.call.v1";

fn req_optional_string_or_symbol(payload: &Term, key: &str) -> Result<Option<String>, String> {
    let mm = as_map(payload)?;
    let Some(value) = mm.get(&map_key(key)) else {
        return Ok(None);
    };
    match value {
        Term::Nil => Ok(None),
        Term::Str(s) | Term::Symbol(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Err(format!("`{key}` must not be empty"))
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        _ => Err(format!("`{key}` must be string|symbol")),
    }
}

fn term_has_ok_envelope(term: &Term) -> bool {
    let Term::Map(mm) = term else {
        return false;
    };
    let Some(Term::Bool(ok)) = mm.get(&map_key(":ok")) else {
        return false;
    };
    if *ok {
        mm.contains_key(&map_key(":result"))
            || mm.contains_key(&map_key(":data"))
            || mm.contains_key(&map_key(":status"))
    } else {
        mm.contains_key(&map_key(":error"))
    }
}

fn parse_term_from_stdout(stdout: &[u8]) -> Option<Term> {
    let src = String::from_utf8(stdout.to_vec()).ok()?;
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return None;
    }
    parse_term(trimmed).ok()
}

fn plugin_error_term(
    plugin: String,
    command: String,
    status_code: i64,
    stdout: String,
    stderr: String,
    code: &str,
    message: String,
) -> Term {
    let mut err = BTreeMap::new();
    err.insert(map_key(":code"), Term::Str(code.to_string()));
    err.insert(map_key(":message"), Term::Str(message));

    let mut mm = BTreeMap::new();
    mm.insert(map_key(":ok"), Term::Bool(false));
    mm.insert(map_key(":plugin"), Term::Str(plugin));
    mm.insert(map_key(":command"), Term::Str(command));
    mm.insert(map_key(":status-code"), Term::Int(status_code.into()));
    mm.insert(map_key(":stdout"), Term::Str(stdout));
    mm.insert(map_key(":stderr"), Term::Str(stderr));
    mm.insert(map_key(":error"), Term::Map(err));
    Term::Map(mm)
}

fn ffi_error_term(
    abi_id: String,
    library: String,
    symbol: String,
    status_code: Option<i64>,
    stdout: Option<String>,
    stderr: Option<String>,
    code: &str,
    message: String,
) -> Term {
    let mut err = BTreeMap::new();
    err.insert(map_key(":code"), Term::Str(code.to_string()));
    err.insert(map_key(":message"), Term::Str(message));

    let mut mm = BTreeMap::new();
    mm.insert(map_key(":ok"), Term::Bool(false));
    mm.insert(map_key(":abi-id"), Term::Str(abi_id));
    mm.insert(map_key(":library"), Term::Str(library));
    mm.insert(map_key(":symbol"), Term::Str(symbol));
    mm.insert(map_key(":error"), Term::Map(err));
    if let Some(status) = status_code {
        mm.insert(map_key(":status-code"), Term::Int(status.into()));
    }
    if let Some(stdout) = stdout {
        mm.insert(map_key(":stdout"), Term::Str(stdout));
    }
    if let Some(stderr) = stderr {
        mm.insert(map_key(":stderr"), Term::Str(stderr));
    }
    Term::Map(mm)
}

fn term_to_cli_arg(value: &Term) -> String {
    match value {
        Term::Str(s) | Term::Symbol(s) => s.clone(),
        _ => print_term(value),
    }
}

fn plugin_exec_payload(
    payload_term: &Term,
) -> Result<
    (
        Vec<String>,
        Option<String>,
        Vec<(String, String)>,
        Option<Vec<u8>>,
    ),
    String,
> {
    let Term::Map(mm) = payload_term else {
        return Err(format!(
            "`:payload` must be map for schema `{PLUGIN_REQUEST_EXEC_V1}`"
        ));
    };
    let Some(args_term) = mm.get(&map_key(":args")) else {
        return Err("`:payload/:args` is required for exec schema".to_string());
    };
    let Term::Vector(args_raw) = args_term else {
        return Err("`:payload/:args` must be vector".to_string());
    };
    let args = args_raw.iter().map(term_to_cli_arg).collect::<Vec<_>>();

    let cwd = match mm.get(&map_key(":cwd")) {
        None | Some(Term::Nil) => None,
        Some(Term::Str(s)) => Some(s.clone()),
        Some(_) => return Err("`:payload/:cwd` must be string|nil".to_string()),
    };
    let stdin = match mm.get(&map_key(":stdin")) {
        None | Some(Term::Nil) => None,
        Some(Term::Str(s)) => Some(s.as_bytes().to_vec()),
        Some(Term::Bytes(bytes)) => Some(bytes.to_vec()),
        Some(_) => return Err("`:payload/:stdin` must be string|bytes|nil".to_string()),
    };
    let mut env = Vec::new();
    if let Some(env_term) = mm.get(&map_key(":env")) {
        let Term::Map(env_map) = env_term else {
            return Err("`:payload/:env` must be map<string,string>".to_string());
        };
        for (k, v) in env_map {
            let Term::Str(key) = &k.0 else {
                return Err("`:payload/:env` keys must be strings".to_string());
            };
            let Term::Str(value) = v else {
                return Err("`:payload/:env` values must be strings".to_string());
            };
            env.push((key.clone(), value.clone()));
        }
    }
    Ok((args, cwd, env, stdin))
}

fn run_command_with_optional_stdin(
    mut cmd: Command,
    stdin_bytes: Option<Vec<u8>>,
) -> Result<Output, String> {
    if let Some(stdin_bytes) = stdin_bytes {
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(&stdin_bytes).map_err(|e| e.to_string())?;
        }
        child.wait_with_output().map_err(|e| e.to_string())
    } else {
        cmd.output().map_err(|e| e.to_string())
    }
}

pub(super) fn plugin_command(op: &str, payload: &Term) -> Result<Term, String> {
    let plugin = req_string(payload, ":plugin")?;
    let command = req_string(payload, ":command")?;
    let request_schema_id = req_optional_string_or_symbol(payload, ":request-schema-id")?;
    let response_schema_id = req_optional_string_or_symbol(payload, ":response-schema-id")?;
    let payload_term = as_map(payload)?
        .get(&map_key(":payload"))
        .cloned()
        .unwrap_or(Term::Nil);

    if plugin == "demo" {
        let status = if op.starts_with("editor/") {
            "editor-ok"
        } else {
            "host-ok"
        };
        if response_schema_id.as_deref() == Some(PLUGIN_RESPONSE_BYTES_V1) {
            return Ok(ok_term(vec![
                (":plugin", Term::Str(plugin)),
                (":command", Term::Str(command)),
                (":status", Term::Str(status.to_string())),
                (
                    ":data",
                    Term::Bytes(print_term(&payload_term).into_bytes().into()),
                ),
            ]));
        }
        return Ok(ok_term(vec![
            (":plugin", Term::Str(plugin)),
            (":command", Term::Str(command)),
            (":status", Term::Str(status.to_string())),
        ]));
    }

    let mut cmd = Command::new(&plugin);
    let mut stdin_bytes = None;
    match request_schema_id.as_deref() {
        Some(PLUGIN_REQUEST_EXEC_V1) => {
            let (args, cwd, env, stdin) = plugin_exec_payload(&payload_term)?;
            cmd.arg(&command);
            for arg in args {
                cmd.arg(arg);
            }
            if let Some(cwd) = cwd {
                cmd.current_dir(cwd);
            }
            for (k, v) in env {
                cmd.env(k, v);
            }
            stdin_bytes = stdin;
        }
        Some(PLUGIN_REQUEST_JSONRPC_V1) => {
            cmd.arg(&command);
            stdin_bytes = Some(print_term(&payload_term).into_bytes());
        }
        _ => {
            cmd.arg(&command);
            cmd.arg(print_term(&payload_term));
        }
    }
    let output = run_command_with_optional_stdin(cmd, stdin_bytes)
        .map_err(|e| format!("spawn plugin `{plugin}` failed: {e}"))?;
    let status_code = output.status.code().unwrap_or(1) as i64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let message = if stderr.trim().is_empty() {
            format!("plugin `{plugin}` command `{command}` exited with status {status_code}")
        } else {
            stderr.trim().to_string()
        };
        return Ok(plugin_error_term(
            plugin,
            command,
            status_code,
            stdout,
            stderr,
            "host/plugin::command",
            message,
        ));
    }

    if let Some(term) = parse_term_from_stdout(&output.stdout)
        && term_has_ok_envelope(&term)
    {
        return Ok(term);
    }
    if response_schema_id.as_deref() == Some(PLUGIN_RESPONSE_BYTES_V1) {
        return Ok(ok_term(vec![
            (":plugin", Term::Str(plugin)),
            (":command", Term::Str(command)),
            (":status-code", Term::Int(status_code.into())),
            (":data", Term::Bytes(output.stdout.into())),
            (":stderr", Term::Str(stderr)),
        ]));
    }
    Ok(ok_term(vec![
        (":plugin", Term::Str(plugin)),
        (":command", Term::Str(command)),
        (":status-code", Term::Int(status_code.into())),
        (":stdout", Term::Str(stdout)),
        (":stderr", Term::Str(stderr)),
    ]))
}

fn ffi_buffer_dir() -> Result<PathBuf, String> {
    let dir = state_root()?.join("ffi").join("buffers");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

pub(super) fn ffi_buffer_pin(payload: &Term) -> Result<Term, String> {
    let _abi_id = req_string(payload, ":abi-id")?;
    let bytes = req_bytes(payload, ":bytes")?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let handle = format!(
        "ffi-buffer-{}-{}",
        &digest[..12],
        next_counter("ffi_buffer")?
    );
    let path = ffi_buffer_dir()?.join(format!("{handle}.bin"));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(ok_term(vec![(":handle", Term::Str(handle))]))
}

pub(super) fn ffi_buffer_unpin(payload: &Term) -> Result<Term, String> {
    let _abi_id = req_string(payload, ":abi-id")?;
    let handle = req_string(payload, ":handle")?;
    let path = ffi_buffer_dir()?.join(format!("{handle}.bin"));
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(ok_term(vec![(
        ":status",
        Term::Str("unpinned".to_string()),
    )]))
}

fn ffi_call_external(payload: &Term, abi_id: String, library: String, symbol: String) -> Term {
    let mm = match as_map(payload) {
        Ok(mm) => mm,
        Err(err) => {
            return ffi_error_term(
                abi_id,
                library,
                symbol,
                None,
                None,
                None,
                "ffi/payload",
                err,
            );
        }
    };
    let mut request = BTreeMap::new();
    request.insert(map_key(":abi-id"), Term::Str(abi_id.clone()));
    request.insert(map_key(":library"), Term::Str(library.clone()));
    request.insert(map_key(":symbol"), Term::Str(symbol.clone()));
    if let Some(args) = mm.get(&map_key(":args")) {
        request.insert(map_key(":args"), args.clone());
    }
    if let Some(payload_term) = mm.get(&map_key(":payload")) {
        request.insert(map_key(":payload"), payload_term.clone());
    }
    if let Some(mode) = mm.get(&map_key(":mode")) {
        request.insert(map_key(":mode"), mode.clone());
    }
    if let Some(schema) = mm.get(&map_key(":request-schema-id")) {
        request.insert(map_key(":request-schema-id"), schema.clone());
    }
    if let Some(schema) = mm.get(&map_key(":response-schema-id")) {
        request.insert(map_key(":response-schema-id"), schema.clone());
    }
    let request_term = Term::Map(request);
    let request_src = print_term(&request_term);
    let command = library
        .strip_prefix("cmd://")
        .unwrap_or(&library)
        .to_string();
    let mut cmd = Command::new(&command);
    cmd.arg(&symbol);
    cmd.arg(&request_src);
    cmd.env("GENESIS_FFI_ABI_ID", &abi_id);
    cmd.env("GENESIS_FFI_LIBRARY", &library);
    cmd.env("GENESIS_FFI_SYMBOL", &symbol);

    let output = match run_command_with_optional_stdin(cmd, Some(request_src.into_bytes())) {
        Ok(output) => output,
        Err(err) => {
            return ffi_error_term(
                abi_id,
                library,
                symbol,
                None,
                None,
                None,
                "ffi/exec-failed",
                err,
            );
        }
    };
    let status_code = output.status.code().unwrap_or(1) as i64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        let message = if stderr.trim().is_empty() {
            format!("ffi command exited with status {status_code}")
        } else {
            stderr.trim().to_string()
        };
        return ffi_error_term(
            abi_id,
            library,
            symbol,
            Some(status_code),
            Some(stdout),
            Some(stderr),
            "ffi/exec-exit",
            message,
        );
    }
    if let Some(term) = parse_term_from_stdout(&output.stdout) {
        if term_has_ok_envelope(&term) {
            return term;
        }
        return ok_term(vec![
            (":abi-id", Term::Str(abi_id)),
            (":library", Term::Str(library)),
            (":symbol", Term::Str(symbol)),
            (":status-code", Term::Int(status_code.into())),
            (":result", term),
            (":stderr", Term::Str(stderr)),
        ]);
    }
    ok_term(vec![
        (":abi-id", Term::Str(abi_id)),
        (":library", Term::Str(library)),
        (":symbol", Term::Str(symbol)),
        (":status-code", Term::Int(status_code.into())),
        (":result", Term::Str(stdout)),
        (":stderr", Term::Str(stderr)),
    ])
}

pub(super) fn ffi_call(payload: &Term) -> Result<Term, String> {
    let abi_id = req_string(payload, ":abi-id")?;
    let library = req_string(payload, ":library")?;
    let symbol = req_string(payload, ":symbol")?;
    let _request_schema_id = req_optional_string_or_symbol(payload, ":request-schema-id")?;
    let _response_schema_id = req_optional_string_or_symbol(payload, ":response-schema-id")?;
    let mm = as_map(payload)?;
    let args = match mm.get(&map_key(":args")) {
        Some(Term::Vector(values)) => values.clone(),
        Some(Term::Nil) | None => Vec::new(),
        _ => return Err("`:args` must be vector|nil".to_string()),
    };

    if abi_id == "libc.v1" && symbol == "strlen" {
        let Some(arg0) = args.first() else {
            return Err("ffi strlen requires one argument".to_string());
        };
        let len = match arg0 {
            Term::Bytes(bytes) => bytes.len() as i64,
            Term::Str(s) => s.len() as i64,
            _ => return Err("ffi strlen arg must be bytes|string".to_string()),
        };
        return Ok(ok_term(vec![(":result", Term::Int(len.into()))]));
    }

    if abi_id == "genesis/ffi.memory.v1" && symbol == "buffer-len" {
        let Some(Term::Str(handle)) = args.first() else {
            return Err("ffi buffer-len requires handle string arg".to_string());
        };
        let path = ffi_buffer_dir()?.join(format!("{handle}.bin"));
        let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len() as i64;
        return Ok(ok_term(vec![(":result", Term::Int(len.into()))]));
    }

    if abi_id == "genesis/ffi.memory.v1" && symbol == "buffer-read" {
        let Some(Term::Str(handle)) = args.first() else {
            return Err("ffi buffer-read requires handle string arg".to_string());
        };
        let path = ffi_buffer_dir()?.join(format!("{handle}.bin"));
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        return Ok(ok_term(vec![(":result", Term::Bytes(bytes.into()))]));
    }

    if matches!(
        _request_schema_id.as_deref(),
        Some(FFI_REQUEST_CALL_V1) | None
    ) && matches!(
        _response_schema_id.as_deref(),
        Some(FFI_RESPONSE_CALL_V1) | None
    ) {
        return Ok(ffi_call_external(payload, abi_id, library, symbol));
    }

    Ok(ffi_error_term(
        abi_id,
        library,
        symbol,
        None,
        None,
        None,
        "ffi/unsupported-schema",
        "unsupported ffi schema combination for first-party bridge runtime".to_string(),
    ))
}
