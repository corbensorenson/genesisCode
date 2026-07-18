#!/usr/bin/env python3
"""Supervised, evidence-retaining Responses-to-MLX Chat Completions adapter."""

from __future__ import annotations

import argparse
import copy
import hashlib
import http.client
import http.server
import json
import os
import signal
import socket
import subprocess
import sys
import threading
import time
import tempfile
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


MAX_BODY_BYTES = 16 * 1024 * 1024
MAX_BACKEND_BYTES = 16 * 1024 * 1024
EXPECTED_REQUEST_FIELDS = {
    "client_metadata", "include", "input", "instructions", "model", "parallel_tool_calls",
    "prompt_cache_key", "reasoning", "store", "stream", "tool_choice", "tools",
}
EXPECTED_TOOL_NAMES = (
    "exec_command",
    "request_user_input",
    "update_plan",
    "view_image",
    "write_stdin",
)
FORBIDDEN_HEADERS = {"authorization", "cookie", "proxy-authorization", "x-api-key"}
FORBIDDEN_CONTEXT_MARKERS = (
    "<skills_instructions>",
    "<apps_instructions>",
    "<plugins_instructions>",
    "<memories>",
    "mcp__",
)


class AdapterError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AdapterError(message)


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def atomic_write(path: Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    with temp.open("xb") as stream:
        stream.write(payload)
        stream.flush()
        os.fsync(stream.fileno())
    temp.chmod(0o600)
    temp.replace(path)


def content_text(content: Any, label: str) -> str:
    if isinstance(content, str):
        return content
    require(isinstance(content, list), f"{label} content must be text or an array")
    parts: list[str] = []
    for index, raw in enumerate(content):
        require(isinstance(raw, dict), f"{label} content[{index}] is not an object")
        require(raw.get("type") in {"input_text", "output_text"}, f"unsupported {label} content type")
        require(isinstance(raw.get("text"), str), f"invalid {label} text")
        parts.append(raw["text"])
    return "\n".join(parts)


def validate_request(body: Any, headers: dict[str, str], model_id: str, seen_turns: set[str], seen_ids: set[str]) -> str:
    require(isinstance(body, dict) and set(body) == EXPECTED_REQUEST_FIELDS, "Responses request field drift")
    lowered = {name.lower(): value for name, value in headers.items()}
    require(not FORBIDDEN_HEADERS.intersection(lowered), "credential-bearing request header rejected")
    request_id = lowered.get("x-client-request-id")
    require(isinstance(request_id, str) and 1 <= len(request_id) <= 256 and request_id.isascii(), "missing or invalid client request id")
    require(request_id not in seen_ids, "duplicate client request id rejected")
    require(body["model"] == model_id, "requested model substitution")
    require(body["stream"] is True and body["store"] is False, "Responses transport policy drift")
    require(body["parallel_tool_calls"] is False and body["tool_choice"] == "auto", "tool dispatch policy drift")
    require(isinstance(body["instructions"], str) and isinstance(body["input"], list), "invalid Responses context")
    tools = body["tools"]
    require(isinstance(tools, list), "Responses tools must be an array")
    names: list[str] = []
    for tool in tools:
        require(isinstance(tool, dict) and tool.get("type") == "function", "non-function tool rejected")
        require(isinstance(tool.get("name"), str), "unnamed function tool rejected")
        names.append(tool["name"])
    require(tuple(sorted(names)) == EXPECTED_TOOL_NAMES and len(names) == len(set(names)), "undeclared Codex tool scaffold")
    context = body["instructions"] + "\n" + json.dumps(body["input"], sort_keys=True, ensure_ascii=True)
    require(not any(marker in context for marker in FORBIDDEN_CONTEXT_MARKERS), "ambient skill/plugin/app/MCP context rejected")
    turn_identity = sha256_bytes(canonical_bytes(body["input"]))
    require(turn_identity not in seen_turns, "duplicate agent turn rejected as a hidden retry")
    seen_turns.add(turn_identity); seen_ids.add(request_id)
    return request_id


def responses_to_chat(body: dict[str, Any], max_tokens: int, backend_model: str = "$GENESISBENCH_MODEL_ROOT") -> dict[str, Any]:
    messages: list[dict[str, Any]] = []
    if body["instructions"]:
        messages.append({"role": "system", "content": body["instructions"]})
    pending_calls: list[dict[str, Any]] = []
    for index, item in enumerate(body["input"]):
        require(isinstance(item, dict), f"Responses input[{index}] is not an object")
        kind = item.get("type")
        if kind == "message":
            role = item.get("role")
            require(role in {"developer", "system", "user", "assistant"}, "unsupported Responses message role")
            mapped = "system" if role in {"developer", "system"} else role
            messages.append({"role": mapped, "content": content_text(item.get("content"), f"input[{index}]")})
        elif kind == "function_call":
            require(isinstance(item.get("call_id"), str) and isinstance(item.get("name"), str), "invalid historical function call")
            arguments = item.get("arguments")
            require(isinstance(arguments, str), "historical function arguments must be a string")
            pending_calls.append({
                "id": item["call_id"], "type": "function",
                "function": {"name": item["name"], "arguments": arguments},
            })
        elif kind == "function_call_output":
            require(pending_calls and isinstance(item.get("call_id"), str), "orphan function call output")
            messages.append({"role": "assistant", "content": None, "tool_calls": pending_calls})
            pending_calls = []
            output = item.get("output")
            require(isinstance(output, str), "function call output must be text")
            messages.append({"role": "tool", "tool_call_id": item["call_id"], "content": output})
        elif kind in {"reasoning", "compaction"}:
            continue
        else:
            raise AdapterError(f"unsupported Responses input item: {kind}")
    if pending_calls:
        messages.append({"role": "assistant", "content": None, "tool_calls": pending_calls})
    tools = [{
        "type": "function",
        "function": {
            "name": tool["name"],
            "description": tool.get("description", ""),
            "parameters": tool.get("parameters", {"type": "object", "properties": {}}),
        },
    } for tool in body["tools"]]
    return {
        "max_completion_tokens": max_tokens,
        "messages": messages,
        "model": backend_model,
        "seed": 0,
        "stream": False,
        "temperature": 0.0,
        "tool_choice": "auto",
        "tools": tools,
        "top_p": 1.0,
    }


def backend_completion(port: int, request: dict[str, Any]) -> dict[str, Any]:
    payload = canonical_bytes(request)
    http_request = urllib.request.Request(
        f"http://127.0.0.1:{port}/v1/chat/completions",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(http_request, timeout=1800) as response:
            raw = response.read(MAX_BACKEND_BYTES + 1)
    except urllib.error.HTTPError as exc:
        detail = exc.read(4097)
        if len(detail) > 4096:
            detail = detail[:4096]
        message = detail.decode("utf-8", errors="replace").replace("\n", " ")
        raise AdapterError(f"MLX backend HTTP {exc.code} without retry: {message}") from exc
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise AdapterError(f"MLX backend request failed without retry: {type(exc).__name__}") from exc
    require(len(raw) <= MAX_BACKEND_BYTES, "MLX backend response exceeded capture limit")
    try:
        document = json.loads(raw)
    except (json.JSONDecodeError, UnicodeError) as exc:
        raise AdapterError("MLX backend returned invalid JSON") from exc
    return document


def response_message(document: Any) -> tuple[str, list[dict[str, Any]], dict[str, int]]:
    require(isinstance(document, dict) and isinstance(document.get("choices"), list) and len(document["choices"]) == 1, "invalid MLX completion choices")
    message = document["choices"][0].get("message")
    require(isinstance(message, dict), "MLX completion message is absent")
    content = message.get("content") or ""
    require(isinstance(content, str), "MLX completion content must be text")
    tool_calls = message.get("tool_calls") or []
    require(isinstance(tool_calls, list), "MLX tool calls must be an array")
    normalized: list[dict[str, Any]] = []
    for index, raw in enumerate(tool_calls):
        require(isinstance(raw, dict) and raw.get("type") == "function", "unsupported MLX tool call")
        function = raw.get("function")
        require(isinstance(function, dict) and isinstance(function.get("name"), str), "invalid MLX function call")
        arguments = function.get("arguments")
        if not isinstance(arguments, str):
            arguments = json.dumps(arguments, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        json.loads(arguments)
        call_id = raw.get("id") or f"call_{index}_{sha256_bytes(arguments.encode('utf-8'))[:16]}"
        require(isinstance(call_id, str) and call_id, "invalid MLX call id")
        normalized.append({"call_id": call_id, "name": function["name"], "arguments": arguments})
    usage = document.get("usage") or {}
    normalized_usage = {
        "input_tokens": int(usage.get("prompt_tokens", 0)),
        "output_tokens": int(usage.get("completion_tokens", 0)),
        "total_tokens": int(usage.get("total_tokens", 0)),
    }
    require(all(value >= 0 for value in normalized_usage.values()), "invalid MLX token accounting")
    return content, normalized, normalized_usage


def event(kind: str, sequence: int, **fields: Any) -> bytes:
    data = {"type": kind, "sequence_number": sequence, **fields}
    return f"event: {kind}\ndata: {json.dumps(data, sort_keys=True, separators=(',', ':'), ensure_ascii=True)}\n\n".encode("ascii")


def response_base(response_id: str, model: str, output: list[dict[str, Any]], usage: dict[str, int] | None, status: str) -> dict[str, Any]:
    return {
        "created_at": 0,
        "error": None,
        "id": response_id,
        "incomplete_details": None,
        "instructions": None,
        "max_output_tokens": None,
        "metadata": {},
        "model": model,
        "object": "response",
        "output": output,
        "parallel_tool_calls": False,
        "previous_response_id": None,
        "reasoning": {"effort": None, "summary": None},
        "status": status,
        "store": False,
        "temperature": None,
        "text": {"format": {"type": "text"}},
        "tool_choice": "auto",
        "tools": [],
        "top_p": None,
        "truncation": "disabled",
        "usage": None if usage is None else {
            "input_tokens": usage["input_tokens"],
            "input_tokens_details": {"cached_tokens": 0},
            "output_tokens": usage["output_tokens"],
            "output_tokens_details": {"reasoning_tokens": 0},
            "total_tokens": usage["total_tokens"],
        },
    }


def responses_events(model: str, request_id: str, content: str, calls: list[dict[str, Any]], usage: dict[str, int]) -> bytes:
    response_id = "resp_" + sha256_bytes(request_id.encode("ascii"))[:24]
    output: list[dict[str, Any]] = []
    if content:
        output.append({
            "content": [{"annotations": [], "logprobs": [], "text": content, "type": "output_text"}],
            "id": "msg_" + response_id[5:], "role": "assistant", "status": "completed", "type": "message",
        })
    for call in calls:
        output.append({"arguments": call["arguments"], "call_id": call["call_id"], "id": "fc_" + sha256_bytes(call["call_id"].encode("utf-8"))[:24], "name": call["name"], "status": "completed", "type": "function_call"})
    sequence = 0
    chunks = [event("response.created", sequence, response=response_base(response_id, model, [], None, "in_progress"))]
    sequence += 1
    for output_index, item in enumerate(output):
        if item["type"] == "message":
            pending = {**item, "content": [], "status": "in_progress"}
            part = item["content"][0]
            chunks.append(event("response.output_item.added", sequence, output_index=output_index, item=pending)); sequence += 1
            chunks.append(event("response.content_part.added", sequence, item_id=item["id"], output_index=output_index, content_index=0, part={**part, "text": ""})); sequence += 1
            chunks.append(event("response.output_text.delta", sequence, item_id=item["id"], output_index=output_index, content_index=0, delta=content, logprobs=[])); sequence += 1
            chunks.append(event("response.output_text.done", sequence, item_id=item["id"], output_index=output_index, content_index=0, text=content, logprobs=[])); sequence += 1
            chunks.append(event("response.content_part.done", sequence, item_id=item["id"], output_index=output_index, content_index=0, part=part)); sequence += 1
            chunks.append(event("response.output_item.done", sequence, output_index=output_index, item=item)); sequence += 1
        else:
            pending = {**item, "arguments": "", "status": "in_progress"}
            chunks.append(event("response.output_item.added", sequence, output_index=output_index, item=pending)); sequence += 1
            chunks.append(event("response.function_call_arguments.delta", sequence, item_id=item["id"], output_index=output_index, delta=item["arguments"])); sequence += 1
            chunks.append(event("response.function_call_arguments.done", sequence, item_id=item["id"], output_index=output_index, arguments=item["arguments"])); sequence += 1
            chunks.append(event("response.output_item.done", sequence, output_index=output_index, item=item)); sequence += 1
    chunks.append(event("response.completed", sequence, response=response_base(response_id, model, output, usage, "completed")))
    return b"".join(chunks)


class State:
    def __init__(self, evidence: Path, model_id: str, backend_model: str, backend_port: int, max_tokens: int) -> None:
        self.evidence = evidence
        self.model_id = model_id
        self.backend_model = backend_model
        self.backend_port = backend_port
        self.max_tokens = max_tokens
        self.lock = threading.Lock()
        self.seen_turns: set[str] = set()
        self.seen_ids: set[str] = set()
        self.records: list[dict[str, Any]] = []
        self.rejections: list[dict[str, Any]] = []
        self.authorization_headers_observed = False
        self.hidden_retries_observed = False

    def execute(self, body: dict[str, Any], headers: dict[str, str]) -> bytes:
        with self.lock:
            request_id = validate_request(body, headers, self.model_id, self.seen_turns, self.seen_ids)
            index = len(self.records)
            backend_request = responses_to_chat(body, self.max_tokens, self.backend_model)
            atomic_write(self.evidence / f"turn-{index:03d}-responses-request.json", canonical_bytes(body) + b"\n")
            retained_backend_request = {**backend_request, "model": "$GENESISBENCH_MODEL_ROOT"}
            atomic_write(self.evidence / f"turn-{index:03d}-backend-request.json", canonical_bytes(retained_backend_request) + b"\n")
            backend_response = backend_completion(self.backend_port, backend_request)
            atomic_write(self.evidence / f"turn-{index:03d}-backend-response.json", canonical_bytes(backend_response) + b"\n")
            content, calls, usage = response_message(backend_response)
            payload = responses_events(self.model_id, request_id, content, calls, usage)
            atomic_write(self.evidence / f"turn-{index:03d}-responses-events.sse", payload)
            self.records.append({
                "backendRequestCount": 1,
                "clientRequestId": request_id,
                "clientRequestIdSha256": sha256_bytes(request_id.encode("ascii")),
                "index": index,
                "requestIdentitySha256": sha256_bytes(canonical_bytes(body)),
                "responseIdentitySha256": sha256_bytes(payload),
                "toolCalls": len(calls),
            })
            return payload

    def reject(self, raw: bytes, headers: dict[str, str], message: str) -> None:
        with self.lock:
            lowered = {name.lower() for name in headers}
            self.authorization_headers_observed |= bool(FORBIDDEN_HEADERS.intersection(lowered))
            self.hidden_retries_observed |= "duplicate" in message or "hidden retry" in message
            # A backend failure can occur after the request-side files are written.
            # Rejected turns are represented only by their closed rejection record.
            turn_index = len(self.records)
            for suffix in (
                "backend-request.json", "backend-response.json", "responses-events.sse",
                "responses-request.json",
            ):
                (self.evidence / f"turn-{turn_index:03d}-{suffix}").unlink(missing_ok=True)
            index = len(self.rejections)
            record = {
                "bodySha256": sha256_bytes(raw),
                "headerNames": sorted(lowered),
                "index": index,
                "reason": message[:1024],
            }
            atomic_write(self.evidence / f"rejection-{index:03d}.json", canonical_bytes(record) + b"\n")
            self.rejections.append(record)

    def summary(self, backend_pid: int, backend_reaped: bool) -> dict[str, Any]:
        return {
            "authorizationHeadersObserved": self.authorization_headers_observed,
            "backendPid": backend_pid,
            "backendReaped": backend_reaped,
            "backendRequestCount": len(self.records),
            "hiddenRetriesObserved": self.hidden_retries_observed,
            "kind": "genesis/genesisbench-mlx-adapter-session-v0.1",
            "modelId": self.model_id,
            "records": self.records,
            "rejections": self.rejections,
            "serverFallbackObserved": False,
            "version": "0.1.0",
        }


class AdapterHandler(http.server.BaseHTTPRequestHandler):
    state: State

    def log_message(self, _format: str, *_args: Any) -> None:
        return

    def do_GET(self) -> None:
        if self.path != "/__genesisbench__/health":
            self.send_error(404); return
        payload = b'{"status":"ready"}\n'
        self.send_response(200); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(payload))); self.end_headers(); self.wfile.write(payload)

    def do_POST(self) -> None:
        if self.path != "/v1/responses":
            self.send_error(404); return
        try:
            length = int(self.headers.get("content-length", "-1"))
            require(0 <= length <= MAX_BODY_BYTES, "Responses request exceeds capture limit")
            raw = self.rfile.read(length)
            body = json.loads(raw.decode("utf-8"))
            payload = self.state.execute(body, dict(self.headers))
        except (AdapterError, json.JSONDecodeError, UnicodeError, ValueError) as exc:
            self.state.reject(raw if "raw" in locals() else b"", dict(self.headers), str(exc))
            error = canonical_bytes({"error": {"code": "genesisbench_adapter_rejected", "message": str(exc), "type": "invalid_request_error"}})
            self.send_response(400); self.send_header("content-type", "application/json"); self.send_header("content-length", str(len(error))); self.end_headers(); self.wfile.write(error); return
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.send_header("cache-control", "no-cache"); self.send_header("content-length", str(len(payload))); self.end_headers(); self.wfile.write(payload)


def port_ready(port: int, timeout: float) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return True
        except OSError:
            time.sleep(0.05)
    return False


def terminate(process: subprocess.Popen[bytes]) -> bool:
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill(); process.wait(timeout=5)
    return process.poll() is not None


def serve(args: argparse.Namespace) -> int:
    evidence = args.evidence.resolve()
    require(not evidence.exists(), "adapter evidence directory already exists")
    evidence.mkdir(parents=True, mode=0o700)
    model_root = args.model_root.resolve(strict=True)
    require(model_root.is_dir() and not model_root.is_symlink(), "model root is not a regular directory")
    environment = {
        "HF_DATASETS_OFFLINE": "1", "HF_HUB_OFFLINE": "1", "HOME": str(args.home.resolve()),
        "NO_COLOR": "1", "NO_PROXY": "127.0.0.1,localhost", "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "TOKENIZERS_PARALLELISM": "false", "TRANSFORMERS_OFFLINE": "1",
    }
    command = [
        str(Path(sys.executable).resolve()), "-I", "-m", "mlx_lm", "server",
        "--model", str(model_root), "--host", "127.0.0.1", "--port", str(args.backend_port),
        "--log-level", "WARNING", "--temp", "0", "--top-p", "1", "--max-tokens", str(args.max_tokens),
        "--chat-template-args", '{"enable_thinking":false}',
    ]
    backend_stdout = (evidence / "backend-stdout.txt").open("xb")
    backend_stderr = (evidence / "backend-stderr.txt").open("xb")
    backend = subprocess.Popen(command, env=environment, cwd=args.home, stdin=subprocess.DEVNULL, stdout=backend_stdout, stderr=backend_stderr)
    state = State(evidence, args.model_id, str(model_root), args.backend_port, args.max_tokens)
    server: http.server.ThreadingHTTPServer | None = None
    stopped = threading.Event()

    def request_stop(_signum: int, _frame: Any) -> None:
        stopped.set()
        if server is not None:
            threading.Thread(target=server.shutdown, daemon=True).start()

    signal.signal(signal.SIGTERM, request_stop); signal.signal(signal.SIGINT, request_stop)
    reaped = False
    try:
        require(port_ready(args.backend_port, 120), "MLX backend did not become ready")
        AdapterHandler.state = state
        server = http.server.ThreadingHTTPServer(("127.0.0.1", args.listen_port), AdapterHandler)
        atomic_write(evidence / "ready.json", canonical_bytes({"backendReady": True, "listenReady": True}) + b"\n")
        server.serve_forever(poll_interval=0.1)
    finally:
        if server is not None:
            server.server_close()
        reaped = terminate(backend)
        backend_stdout.close(); backend_stderr.close()
        atomic_write(evidence / "session.json", canonical_bytes(state.summary(backend.pid, reaped)) + b"\n")
    return 0


def fixture_request() -> dict[str, Any]:
    tools = [{"type": "function", "name": name, "description": name, "parameters": {"type": "object", "properties": {}}} for name in EXPECTED_TOOL_NAMES]
    return {
        "client_metadata": {}, "include": [], "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "test"}]}],
        "instructions": "fixed", "model": "fixture", "parallel_tool_calls": False, "prompt_cache_key": "fixture",
        "reasoning": {"effort": "low", "summary": "auto"}, "store": False, "stream": True,
        "tool_choice": "auto", "tools": tools,
    }


def self_test() -> int:
    controls = 0
    base = fixture_request()
    for label, mutate, headers in (
        ("auth", lambda d: None, {"x-client-request-id": "a", "authorization": "secret"}),
        ("fields", lambda d: d.__setitem__("extra", True), {"x-client-request-id": "b"}),
        ("model", lambda d: d.__setitem__("model", "other"), {"x-client-request-id": "c"}),
        ("retry-policy", lambda d: d.__setitem__("parallel_tool_calls", True), {"x-client-request-id": "d"}),
        ("tool", lambda d: d["tools"].append({"type": "function", "name": "web_search"}), {"x-client-request-id": "e"}),
        ("skill", lambda d: d.__setitem__("instructions", "<skills_instructions>"), {"x-client-request-id": "f"}),
        ("mcp", lambda d: d["input"][0]["content"][0].__setitem__("text", "mcp__ambient"), {"x-client-request-id": "g"}),
    ):
        candidate = copy.deepcopy(base); mutate(candidate)
        try:
            validate_request(candidate, headers, "fixture", set(), set())
        except AdapterError:
            controls += 1
        else:
            raise AdapterError(f"negative adapter control accepted: {label}")
    seen_turns: set[str] = set(); seen_ids: set[str] = set()
    validate_request(base, {"x-client-request-id": "one"}, "fixture", seen_turns, seen_ids)
    for label, request_id in (("duplicate-id", "one"), ("hidden-retry", "two")):
        try:
            validate_request(base, {"x-client-request-id": request_id}, "fixture", seen_turns, seen_ids)
        except AdapterError:
            controls += 1
        else:
            raise AdapterError(f"negative adapter control accepted: {label}")
    chat = responses_to_chat(base, 128)
    require(chat["temperature"] == 0.0 and chat["seed"] == 0 and len(chat["tools"]) == len(EXPECTED_TOOL_NAMES), "adapter translation drift")
    content, calls, usage = response_message({"choices": [{"message": {"content": "ok", "tool_calls": []}}], "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}})
    payload = responses_events("fixture", "request", content, calls, usage)
    require(b"response.completed" in payload and b'"text":"ok"' in payload, "Responses event closure drift")
    controls += 2
    with tempfile.TemporaryDirectory(prefix="genesisbench-mlx-adapter-controls-") as raw:
        evidence = Path(raw)
        for name in ("backend-stderr.txt", "backend-stdout.txt"):
            atomic_write(evidence / name, b"")
        atomic_write(evidence / "ready.json", canonical_bytes({"backendReady": True, "listenReady": True}) + b"\n")
        state = State(evidence, "fixture", "$GENESISBENCH_MODEL_ROOT", 1, 128)
        state.reject(canonical_bytes(base), {"Authorization": "redacted"}, "duplicate agent turn rejected as a hidden retry")
        atomic_write(evidence / "session.json", canonical_bytes(state.summary(1, True)) + b"\n")
        session = validate_evidence(evidence, "fixture")
        require(session["authorizationHeadersObserved"] and session["hiddenRetriesObserved"] and len(session["rejections"]) == 1, "rejection evidence was not retained")
        controls += 1
        session["authorizationHeadersObserved"] = False
        atomic_write(evidence / "session.json", canonical_bytes(session) + b"\n")
        try:
            validate_evidence(evidence, "fixture")
        except AdapterError:
            controls += 1
        else:
            raise AdapterError("rejection observation tampering accepted")
    return controls


def validate_evidence(root: Path, model_id: str) -> dict[str, Any]:
    require(root.is_dir() and not root.is_symlink(), "MLX adapter evidence root is unavailable")
    session_path = root / "session.json"
    require(session_path.is_file() and not session_path.is_symlink(), "MLX adapter session is unavailable")
    session = json.loads(session_path.read_text(encoding="ascii"))
    require(isinstance(session, dict) and set(session) == {
        "authorizationHeadersObserved", "backendPid", "backendReaped", "backendRequestCount",
        "hiddenRetriesObserved", "kind", "modelId", "records", "rejections", "serverFallbackObserved", "version",
    }, "MLX adapter session fields are not closed")
    require(session["kind"] == "genesis/genesisbench-mlx-adapter-session-v0.1" and session["version"] == "0.1.0", "MLX adapter session kind/version drift")
    require(session["modelId"] == model_id and session["backendReaped"] is True, "MLX adapter model or reap evidence drift")
    require(isinstance(session["authorizationHeadersObserved"], bool), "invalid authorization-header observation")
    require(isinstance(session["hiddenRetriesObserved"], bool), "invalid hidden-retry observation")
    require(session["serverFallbackObserved"] is False, "MLX adapter server fallback is forbidden")
    require(isinstance(session["backendPid"], int) and session["backendPid"] > 0, "invalid MLX backend pid evidence")
    records = session["records"]
    rejections = session["rejections"]
    require(isinstance(rejections, list), "MLX adapter rejections must be an array")
    for index, rejection in enumerate(rejections):
        require(isinstance(rejection, dict) and set(rejection) == {"bodySha256", "headerNames", "index", "reason"}, "MLX adapter rejection fields are not closed")
        require(rejection["index"] == index and isinstance(rejection["reason"], str), "MLX adapter rejection order drift")
        require(isinstance(rejection["bodySha256"], str) and len(rejection["bodySha256"]) == 64, "invalid rejected body digest")
        require(rejection["headerNames"] == sorted(set(rejection["headerNames"])), "rejected header names drift")
        path = root / f"rejection-{index:03d}.json"
        require(path.is_file() and json.loads(path.read_text(encoding="ascii")) == rejection, "retained rejection drift")
    observed_auth = any(FORBIDDEN_HEADERS.intersection(rejection["headerNames"]) for rejection in rejections)
    observed_retry = any("duplicate" in rejection["reason"] or "hidden retry" in rejection["reason"] for rejection in rejections)
    require(session["authorizationHeadersObserved"] is observed_auth, "authorization-header observation derivation drift")
    require(session["hiddenRetriesObserved"] is observed_retry, "hidden-retry observation derivation drift")
    require(isinstance(records, list) and session["backendRequestCount"] == len(records), "MLX adapter request aggregate drift")
    seen_turns: set[str] = set(); seen_ids: set[str] = set()
    for index, record in enumerate(records):
        require(isinstance(record, dict) and set(record) == {
            "backendRequestCount", "clientRequestId", "clientRequestIdSha256", "index", "requestIdentitySha256",
            "responseIdentitySha256", "toolCalls",
        }, "MLX adapter record fields are not closed")
        require(record["index"] == index and record["backendRequestCount"] == 1, "MLX adapter turn/retry drift")
        require(all(isinstance(record[field], str) and len(record[field]) == 64 for field in ("clientRequestIdSha256", "requestIdentitySha256", "responseIdentitySha256")), "invalid MLX adapter record digest")
        request_path = root / f"turn-{index:03d}-responses-request.json"
        backend_request_path = root / f"turn-{index:03d}-backend-request.json"
        backend_response_path = root / f"turn-{index:03d}-backend-response.json"
        events_path = root / f"turn-{index:03d}-responses-events.sse"
        require(all(path.is_file() and not path.is_symlink() for path in (request_path, backend_request_path, backend_response_path, events_path)), "MLX adapter turn evidence is incomplete")
        request = json.loads(request_path.read_text(encoding="ascii"))
        request_id = record["clientRequestId"]
        require(isinstance(request_id, str) and sha256_bytes(request_id.encode("ascii")) == record["clientRequestIdSha256"], "MLX client request id drift")
        validate_request(request, {"x-client-request-id": request_id}, model_id, seen_turns, seen_ids)
        require(record["requestIdentitySha256"] == sha256_bytes(canonical_bytes(request)), "MLX adapter request identity drift")
        backend_request = json.loads(backend_request_path.read_text(encoding="ascii"))
        expected_backend = responses_to_chat(request, backend_request.get("max_completion_tokens", 0))
        require(backend_request == expected_backend, "MLX backend request translation drift")
        backend_response = json.loads(backend_response_path.read_text(encoding="ascii"))
        content, calls, usage = response_message(backend_response)
        events = events_path.read_bytes()
        require(record["responseIdentitySha256"] == sha256_bytes(events), "MLX Responses event identity drift")
        require(events == responses_events(model_id, request_id, content, calls, usage), "MLX Responses event closure drift")
        require(record["toolCalls"] == len(calls), "MLX tool-call count drift")
    expected = {"backend-stderr.txt", "backend-stdout.txt", "ready.json", "session.json"}
    for index in range(len(records)):
        expected.update({
            f"turn-{index:03d}-backend-request.json", f"turn-{index:03d}-backend-response.json",
            f"turn-{index:03d}-responses-events.sse", f"turn-{index:03d}-responses-request.json",
        })
    expected.update(f"rejection-{index:03d}.json" for index in range(len(rejections)))
    require({path.name for path in root.iterdir()} == expected, "MLX adapter evidence topology drift")
    return session


def parser() -> argparse.ArgumentParser:
    out = argparse.ArgumentParser(description=__doc__)
    modes = out.add_subparsers(dest="command", required=True)
    serve_parser = modes.add_parser("serve")
    serve_parser.add_argument("--model-root", required=True, type=Path); serve_parser.add_argument("--model-id", required=True)
    serve_parser.add_argument("--listen-port", required=True, type=int); serve_parser.add_argument("--backend-port", required=True, type=int)
    serve_parser.add_argument("--evidence", required=True, type=Path); serve_parser.add_argument("--home", required=True, type=Path)
    serve_parser.add_argument("--max-tokens", type=int, default=4096)
    modes.add_parser("self-test")
    return out


def main() -> int:
    args = parser().parse_args()
    if args.command == "serve":
        return serve(args)
    controls = self_test()
    print(f"genesisbench-mlx-responses: ok controls={controls}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AdapterError, OSError, UnicodeError, json.JSONDecodeError, subprocess.SubprocessError) as exc:
        print(f"genesisbench-mlx-responses: {exc}", file=sys.stderr)
        raise SystemExit(1)
