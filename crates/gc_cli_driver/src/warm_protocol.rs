use std::io::{self, BufRead, BufReader, Read};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, JoinHandle};

use serde_json::{Map, Value, json};

pub(super) const WARM_PROTOCOL_V02: &str = "genesis/warm-protocol-v0.2";
pub(super) const WARM_RESPONSE_V02: &str = "genesis/warm-response-v0.2";
pub(super) const WARM_ERROR_V02: &str = "genesis/warm-protocol-error-v0.2";
pub(super) const MAX_DEADLINE_MS: u64 = 86_400_000;

#[derive(Debug, Clone)]
pub(super) struct WorkspaceRef {
    pub(super) id: String,
    pub(super) root: String,
}

#[derive(Debug, Clone)]
pub(super) enum WarmMethod {
    Initialize {
        client_name: String,
        client_version: String,
    },
    Execute {
        workspace: WorkspaceRef,
        argv: Vec<String>,
        deadline_ms: Option<u64>,
    },
    Cancel {
        target_id: String,
    },
    Restart,
    Shutdown,
    Ping,
}

#[derive(Debug, Clone)]
pub(super) struct WarmFrame {
    pub(super) id: String,
    pub(super) method: WarmMethod,
}

#[derive(Debug, Clone)]
pub(super) struct ProtocolError {
    pub(super) request_id: Option<String>,
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) retryable: bool,
    pub(super) details: Value,
}

impl ProtocolError {
    fn new(
        request_id: Option<String>,
        code: &'static str,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            request_id,
            code,
            message: message.into(),
            retryable,
            details: json!({}),
        }
    }

    pub(super) fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
}

fn object<'a>(value: &'a Value, context: &str) -> Result<&'a Map<String, Value>, ProtocolError> {
    value.as_object().ok_or_else(|| {
        ProtocolError::new(
            None,
            "warm/frame-shape",
            format!("{context} must be a JSON object"),
            false,
        )
    })
}

fn string_field(
    object: &Map<String, Value>,
    field: &str,
    request_id: Option<&str>,
) -> Result<String, ProtocolError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            ProtocolError::new(
                request_id.map(str::to_string),
                "warm/frame-field",
                format!("`{field}` must be a non-empty string"),
                false,
            )
        })
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn require_fields(
    object: &Map<String, Value>,
    required: &[&str],
    request_id: Option<&str>,
) -> Result<(), ProtocolError> {
    let mut actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    let mut expected = required.to_vec();
    actual.sort_unstable();
    expected.sort_unstable();
    if actual != expected {
        return Err(ProtocolError::new(
            request_id.map(str::to_string),
            "warm/frame-fields",
            "frame fields do not match the selected method",
            false,
        )
        .with_details(json!({"expected": expected, "observed": actual})));
    }
    Ok(())
}

fn parse_workspace(value: &Value, request_id: &str) -> Result<WorkspaceRef, ProtocolError> {
    let workspace = object(value, "workspace").map_err(|mut error| {
        error.request_id = Some(request_id.to_string());
        error
    })?;
    require_fields(workspace, &["id", "root"], Some(request_id))?;
    let id = string_field(workspace, "id", Some(request_id))?;
    if !valid_id(&id) {
        return Err(ProtocolError::new(
            Some(request_id.to_string()),
            "warm/workspace-id",
            "workspace ID must be 1..128 ASCII identifier characters",
            false,
        ));
    }
    let root = string_field(workspace, "root", Some(request_id))?;
    Ok(WorkspaceRef { id, root })
}

fn parse_argv(value: &Value, request_id: &str) -> Result<Vec<String>, ProtocolError> {
    let values = value.as_array().ok_or_else(|| {
        ProtocolError::new(
            Some(request_id.to_string()),
            "warm/argv-shape",
            "`argv` must be a non-empty string array",
            false,
        )
    })?;
    if values.is_empty() || values.len() > 256 {
        return Err(ProtocolError::new(
            Some(request_id.to_string()),
            "warm/argv-bound",
            "`argv` must contain 1..256 entries",
            false,
        ));
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|item| !item.is_empty() && item.len() <= 16_384)
                .map(str::to_string)
                .ok_or_else(|| {
                    ProtocolError::new(
                        Some(request_id.to_string()),
                        "warm/argv-entry",
                        "every argv entry must be a non-empty string of at most 16384 bytes",
                        false,
                    )
                })
        })
        .collect()
}

pub(super) fn parse_frame(line: &str) -> Result<WarmFrame, ProtocolError> {
    let value: Value = serde_json::from_str(line).map_err(|error| {
        ProtocolError::new(
            None,
            "warm/frame-json",
            format!("invalid JSON frame: {error}"),
            false,
        )
    })?;
    let frame = object(&value, "frame")?;
    let request_id = frame.get("id").and_then(Value::as_str).map(str::to_string);
    let id = string_field(frame, "id", request_id.as_deref())?;
    if !valid_id(&id) {
        return Err(ProtocolError::new(
            Some(id),
            "warm/request-id",
            "request ID must be 1..128 ASCII identifier characters",
            false,
        ));
    }
    let protocol = string_field(frame, "protocol", Some(&id))?;
    if protocol != WARM_PROTOCOL_V02 {
        return Err(ProtocolError::new(
            Some(id),
            "warm/protocol-version",
            "unsupported warm protocol version",
            false,
        )
        .with_details(json!({"supported": [WARM_PROTOCOL_V02], "received": protocol})));
    }
    let method = string_field(frame, "method", Some(&id))?;
    let method = match method.as_str() {
        "initialize" => {
            require_fields(frame, &["protocol", "id", "method", "client"], Some(&id))?;
            let client = object(&frame["client"], "client").map_err(|mut error| {
                error.request_id = Some(id.clone());
                error
            })?;
            require_fields(client, &["name", "version"], Some(&id))?;
            WarmMethod::Initialize {
                client_name: string_field(client, "name", Some(&id))?,
                client_version: string_field(client, "version", Some(&id))?,
            }
        }
        "execute" => {
            let allowed = if frame.contains_key("deadline_ms") {
                vec![
                    "protocol",
                    "id",
                    "method",
                    "workspace",
                    "argv",
                    "deadline_ms",
                ]
            } else {
                vec!["protocol", "id", "method", "workspace", "argv"]
            };
            require_fields(frame, &allowed, Some(&id))?;
            let deadline_ms = match frame.get("deadline_ms") {
                None | Some(Value::Null) => None,
                Some(value) => {
                    let deadline = value
                        .as_u64()
                        .filter(|value| *value > 0 && *value <= MAX_DEADLINE_MS)
                        .ok_or_else(|| {
                            ProtocolError::new(
                                Some(id.clone()),
                                "warm/deadline-bound",
                                format!("deadline_ms must be in 1..={MAX_DEADLINE_MS}"),
                                false,
                            )
                        })?;
                    Some(deadline)
                }
            };
            WarmMethod::Execute {
                workspace: parse_workspace(&frame["workspace"], &id)?,
                argv: parse_argv(&frame["argv"], &id)?,
                deadline_ms,
            }
        }
        "cancel" => {
            require_fields(frame, &["protocol", "id", "method", "target_id"], Some(&id))?;
            let target_id = string_field(frame, "target_id", Some(&id))?;
            if !valid_id(&target_id) {
                return Err(ProtocolError::new(
                    Some(id),
                    "warm/target-id",
                    "target request ID is invalid",
                    false,
                ));
            }
            WarmMethod::Cancel { target_id }
        }
        "restart" => {
            require_fields(frame, &["protocol", "id", "method"], Some(&id))?;
            WarmMethod::Restart
        }
        "shutdown" => {
            require_fields(frame, &["protocol", "id", "method"], Some(&id))?;
            WarmMethod::Shutdown
        }
        "ping" => {
            require_fields(frame, &["protocol", "id", "method"], Some(&id))?;
            WarmMethod::Ping
        }
        _ => {
            return Err(ProtocolError::new(
                Some(id),
                "warm/method",
                "unknown warm protocol method",
                false,
            )
            .with_details(json!({
                "supported": ["initialize", "execute", "cancel", "restart", "shutdown", "ping"]
            })));
        }
    };
    Ok(WarmFrame { id, method })
}

#[derive(Debug)]
pub(super) enum InputEvent {
    Line(String),
    Oversize,
    InvalidUtf8,
    Eof,
    IoError(String),
}

pub(super) fn read_bounded_event<R: BufRead>(reader: &mut R, max_bytes: usize) -> InputEvent {
    let mut bytes = Vec::with_capacity(max_bytes.min(4096).saturating_add(1));
    let read_result = reader
        .by_ref()
        .take(max_bytes.saturating_add(1) as u64)
        .read_until(b'\n', &mut bytes);
    let read = match read_result {
        Ok(read) => read,
        Err(error) => return InputEvent::IoError(error.to_string()),
    };
    if read == 0 {
        return InputEvent::Eof;
    }
    if bytes.len() > max_bytes {
        if !bytes.ends_with(b"\n") {
            loop {
                let available = match reader.fill_buf() {
                    Ok(available) => available,
                    Err(error) => return InputEvent::IoError(error.to_string()),
                };
                if available.is_empty() {
                    break;
                }
                if let Some(index) = available.iter().position(|byte| *byte == b'\n') {
                    reader.consume(index + 1);
                    break;
                }
                let length = available.len();
                reader.consume(length);
            }
        }
        return InputEvent::Oversize;
    }
    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
        bytes.pop();
    }
    match String::from_utf8(bytes) {
        Ok(line) => InputEvent::Line(line),
        Err(_) => InputEvent::InvalidUtf8,
    }
}

pub(super) fn spawn_bounded_reader(
    max_bytes: usize,
    capacity: usize,
) -> io::Result<(Receiver<InputEvent>, JoinHandle<()>)> {
    let (sender, receiver) = sync_channel(capacity);
    let handle = thread::Builder::new()
        .name("genesis-warm-reader".to_string())
        .spawn(move || read_stdin(sender, max_bytes))?;
    Ok((receiver, handle))
}

fn read_stdin(sender: SyncSender<InputEvent>, max_bytes: usize) {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    loop {
        let event = read_bounded_event(&mut reader, max_bytes);
        let terminal = matches!(event, InputEvent::Eof | InputEvent::IoError(_));
        if sender.send(event).is_err() || terminal {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn bounded_reader_discards_the_rest_of_an_oversized_frame() {
        let mut reader = Cursor::new(b"123456789\n{}\n".to_vec());
        assert!(matches!(
            read_bounded_event(&mut reader, 4),
            InputEvent::Oversize
        ));
        assert!(
            matches!(read_bounded_event(&mut reader, 4), InputEvent::Line(line) if line == "{}")
        );
    }

    #[test]
    fn parser_rejects_unknown_fields_and_version_drift() {
        let unknown =
            r#"{"protocol":"genesis/warm-protocol-v0.2","id":"x","method":"ping","extra":1}"#;
        assert_eq!(parse_frame(unknown).unwrap_err().code, "warm/frame-fields");
        let version = r#"{"protocol":"genesis/warm-protocol-v9","id":"x","method":"ping"}"#;
        assert_eq!(
            parse_frame(version).unwrap_err().code,
            "warm/protocol-version"
        );
    }
}
