#!/usr/bin/env python3
"""Capture and verify the fail-closed local MLX execution custody boundary."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import platform
import re
import shutil
import socket
import subprocess
import sys
import tempfile
from pathlib import Path, PurePosixPath
from typing import Any

import genesisbench_local_models


ROOT = Path(__file__).resolve().parents[2]
SCHEMA_PATH = ROOT / "docs/spec/GENESISBENCH_MLX_CUSTODY_v0.1.schema.json"
PLAN_PATH = ROOT / "benchmarks/genesisbench/v0.1/local-models/preselection.json"
INVENTORY_PATH = ROOT / "benchmarks/genesisbench/v0.1/local-models/inventory.json"
KIND = "genesis/genesisbench-mlx-custody-v0.1"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:@+-]{0,191}$")
REL_RE = re.compile(r"^(?!/)(?!.*(?:^|/)\.\.(?:/|$))[A-Za-z0-9._+@/-]{1,768}$")
ADAPTER_PATHS = (
    "scripts/lib/genesisbench_mlx_custody.py",
    "scripts/lib/genesisbench_mlx_responses.py",
)
MAX_RUNTIME_FILES = 20_000
MAX_RUNTIME_BYTES = 4 * 1024 * 1024 * 1024


class CustodyError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise CustodyError(message)


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def object_identity(value: dict[str, Any]) -> str:
    candidate = copy.deepcopy(value)
    candidate["contentIdentitySha256"] = ""
    return sha256_bytes(canonical_bytes(candidate))


def identified(value: dict[str, Any]) -> dict[str, Any]:
    out = copy.deepcopy(value)
    out["contentIdentitySha256"] = object_identity(out)
    return out


def load_json(path: Path) -> Any:
    require(path.is_file() and not path.is_symlink(), f"missing regular JSON file: {path.name}")
    with path.open("r", encoding="ascii") as stream:
        return json.load(stream)


def validate_schema() -> None:
    schema = load_json(SCHEMA_PATH)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "MLX custody schema draft drift")
    require(schema.get("$id") == "https://genesiscode.dev/schemas/genesisbench-mlx-custody-v0.1.json", "MLX custody schema id drift")

    def walk(value: Any, label: str) -> None:
        if isinstance(value, dict):
            if value.get("type") == "object":
                require(value.get("additionalProperties") is False, f"open MLX custody schema object: {label}")
                require(set(value.get("required", [])) == set(value.get("properties", {})), f"optional MLX custody schema field: {label}")
            for key, child in value.items():
                walk(child, f"{label}/{key}")
        elif isinstance(value, list):
            for index, child in enumerate(value):
                walk(child, f"{label}/{index}")

    walk(schema, "schema")


def closed(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    require(isinstance(value, dict) and set(value) == fields, f"{label} fields are not closed")
    return value


def safe_id(value: Any, label: str) -> str:
    require(isinstance(value, str) and ID_RE.fullmatch(value) is not None, f"invalid {label}")
    return value


def safe_relative(value: Any, label: str) -> str:
    require(isinstance(value, str) and REL_RE.fullmatch(value) is not None, f"invalid {label}")
    path = PurePosixPath(value)
    require(all(part not in {"", ".", ".."} for part in path.parts), f"unsafe {label}")
    return path.as_posix()


def regular_file(path: Path, label: str) -> Path:
    require(path.is_file() and not path.is_symlink(), f"{label} must be a regular non-symlink file")
    return path.resolve(strict=True)


def file_row(path: Path, logical_path: str) -> dict[str, Any]:
    source = regular_file(path, logical_path)
    return {"bytes": source.stat().st_size, "path": safe_relative(logical_path, "file path"), "sha256": sha256_file(source)}


def rows_identity(rows: list[dict[str, Any]]) -> str:
    return sha256_bytes(canonical_bytes(rows))


def adapter_rows() -> list[dict[str, Any]]:
    return [file_row(ROOT / path, path) for path in ADAPTER_PATHS]


def selected_candidate(model_id: str) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any], dict[str, Any]]:
    plan = genesisbench_local_models.validate_plan(load_json(PLAN_PATH))
    inventory = genesisbench_local_models.validate_inventory(load_json(INVENTORY_PATH), plan)
    plans = {row["id"]: row for row in plan["candidates"]}
    artifacts = {row["id"]: row for row in inventory["candidates"]}
    require(model_id in plans and model_id in artifacts, "model is absent from the sealed local inventory")
    candidate = plans[model_id]
    require(candidate["selection"]["status"] == "selected", "model was not score-blind preselected")
    return plan, inventory, candidate, artifacts[model_id]


_RUNTIME_PROBE = r'''
import hashlib, importlib.metadata as md, json, os, pathlib, platform, sys
from packaging.requirements import Requirement

def norm(name): return name.lower().replace('_','-').replace('.','-')
pending=['mlx','mlx-lm']; seen={}; missing=[]
while pending:
 name=norm(pending.pop())
 if name in seen: continue
 try: dist=md.distribution(name)
 except md.PackageNotFoundError:
  missing.append(name); continue
 seen[name]=dist
 for raw in dist.requires or []:
  req=Requirement(raw)
  if req.marker is None or req.marker.evaluate({'extra':''}): pending.append(req.name)
rows=[]; count=0; total=0
for name,dist in sorted(seen.items()):
 files_by_path={}
 for entry in sorted(dist.files or [],key=lambda x:str(x)):
  source=pathlib.Path(dist.locate_file(entry))
  if not source.exists() or source.is_dir(): continue
  if source.is_symlink(): raise RuntimeError('runtime distribution contains symlink: '+str(entry))
  raw_logical=str(entry).replace(os.sep,'/')
  parts=[part for part in raw_logical.split('/') if part not in ('','.')]
  if raw_logical.startswith('/'):
   raise RuntimeError('absolute runtime distribution path: '+raw_logical)
  escaped=any(part == '..' for part in parts)
  logical='/'.join(part for part in parts if part != '..')
  if escaped: logical='external/'+logical
  payload=source.read_bytes()
  row={'path':logical,'bytes':len(payload),'sha256':hashlib.sha256(payload).hexdigest()}
  if logical in files_by_path and files_by_path[logical] != row:
   raise RuntimeError('conflicting runtime distribution path: '+logical)
  files_by_path[logical]=row
 files=[files_by_path[path] for path in sorted(files_by_path)]
 count+=len(files); total+=sum(row['bytes'] for row in files)
 rows.append({'name':name,'version':dist.version,'files':files})
print(json.dumps({'implementation':platform.python_implementation(),'pythonVersion':platform.python_version(),'distributions':rows,'missing':sorted(set(missing)),'fileCount':count,'bytes':total},sort_keys=True,separators=(',',':')))
'''


def runtime_probe(python: Path) -> dict[str, Any]:
    executable = regular_file(python, "MLX Python executable")
    result = subprocess.run(
        [str(executable), "-I", "-c", _RUNTIME_PROBE],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=120,
        env={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "NO_COLOR": "1"},
    )
    require(result.returncode == 0, "MLX runtime inventory probe failed")
    require(len(result.stdout) <= 64 * 1024 * 1024 and result.stdout.isascii(), "invalid MLX runtime inventory output")
    probe = json.loads(result.stdout)
    require(probe["missing"] == [], "MLX runtime has unresolved active dependencies")
    require(0 < probe["fileCount"] <= MAX_RUNTIME_FILES, "MLX runtime file count is outside custody limits")
    require(0 < probe["bytes"] <= MAX_RUNTIME_BYTES, "MLX runtime bytes are outside custody limits")
    return {
        "bytes": executable.stat().st_size,
        "distributions": probe["distributions"],
        "executableSha256": sha256_file(executable),
        "fileCount": probe["fileCount"],
        "implementation": safe_id(probe["implementation"], "Python implementation"),
        "pythonVersion": safe_id(probe["pythonVersion"], "Python version"),
        "runtimeIdentitySha256": rows_identity(probe["distributions"]),
        "totalDistributionBytes": probe["bytes"],
    }


def capture(model_id: str, model_root: Path, python: Path) -> dict[str, Any]:
    plan, inventory, candidate, artifact = selected_candidate(model_id)
    actual_files = genesisbench_local_models.artifact_rows(model_root.resolve())
    require(actual_files == artifact["files"], "local model bytes differ from sealed inventory")
    adapters = adapter_rows()
    return identified({
        "adapter": {
            "files": adapters,
            "identitySha256": rows_identity(adapters),
            "protocol": "responses-to-mlx-chat-completions-v0.1",
        },
        "contentIdentitySha256": "",
        "inventoryIdentitySha256": inventory["contentIdentitySha256"],
        "isolation": {
            "acceleratorPolicy": "apple-metal-system-graphics",
            "backend": "darwin-sandbox-exec-v0.1",
            "failClosedOnUnsupportedHost": True,
            "modelRootForwardedToAgent": False,
            "network": "two-exact-loopback-ports-no-other-network",
            "readPolicy": "closed-system-runtime-model-stage-roots",
            "writePolicy": "closed-ephemeral-stage-roots",
        },
        "kind": KIND,
        "license": {
            "benchmarkUseCompatible": candidate["license"]["benchmarkUseCompatible"],
            "evidence": candidate["evidence"],
            "id": candidate["license"]["id"],
        },
        "model": {
            "artifactIdentitySha256": artifact["artifactIdentitySha256"],
            "bytes": artifact["bytes"],
            "fileCount": artifact["fileCount"],
            "files": artifact["files"],
            "id": candidate["id"],
            "repository": candidate["repository"],
            "revision": candidate["revision"],
            "role": candidate["selection"]["role"],
        },
        "policy": {
            "authorizationHeadersAllowed": False,
            "hiddenRetriesAllowed": False,
            "maxBackendRequestsPerAgentTurn": 1,
            "serverFallbackAllowed": False,
            "streamRetries": 0,
            "trustRemoteCodeAllowed": False,
            "webSearchAllowed": False,
        },
        "preselectionIdentitySha256": plan["contentIdentitySha256"],
        "runtime": runtime_probe(python),
        "version": "0.1.0",
    })


def validate_file_rows(value: Any, label: str, *, max_files: int, max_bytes: int) -> list[dict[str, Any]]:
    require(isinstance(value, list) and 0 < len(value) <= max_files, f"invalid {label} file count")
    paths: list[str] = []
    total = 0
    for index, raw in enumerate(value):
        row = closed(raw, {"bytes", "path", "sha256"}, f"{label}[{index}]")
        paths.append(safe_relative(row["path"], f"{label} path"))
        require(isinstance(row["bytes"], int) and row["bytes"] >= 0, f"invalid {label} bytes")
        require(SHA_RE.fullmatch(row["sha256"] or "") is not None, f"invalid {label} digest")
        total += row["bytes"]
    require(paths == sorted(set(paths)), f"{label} paths are not sorted and unique")
    require(total <= max_bytes, f"{label} exceeds byte limit")
    return value


def validate(document: Any, *, check_local: bool = False, model_root: Path | None = None, python: Path | None = None) -> dict[str, Any]:
    doc = closed(document, {
        "adapter", "contentIdentitySha256", "inventoryIdentitySha256", "isolation", "kind", "license",
        "model", "policy", "preselectionIdentitySha256", "runtime", "version",
    }, "MLX custody manifest")
    require(doc["kind"] == KIND and doc["version"] == "0.1.0", "MLX custody kind/version drift")
    require(SHA_RE.fullmatch(doc["contentIdentitySha256"] or "") is not None and object_identity(doc) == doc["contentIdentitySha256"], "MLX custody identity drift")
    plan, inventory, candidate, artifact = selected_candidate(doc["model"].get("id", ""))
    require(doc["preselectionIdentitySha256"] == plan["contentIdentitySha256"], "preselection identity drift")
    require(doc["inventoryIdentitySha256"] == inventory["contentIdentitySha256"], "inventory identity drift")
    adapter = closed(doc["adapter"], {"files", "identitySha256", "protocol"}, "adapter")
    adapters = validate_file_rows(adapter["files"], "adapter files", max_files=16, max_bytes=4 * 1024 * 1024)
    require(adapter["protocol"] == "responses-to-mlx-chat-completions-v0.1", "adapter protocol drift")
    require(adapter["identitySha256"] == rows_identity(adapters), "adapter identity drift")
    license_record = closed(doc["license"], {"benchmarkUseCompatible", "evidence", "id"}, "license")
    require(license_record == {"benchmarkUseCompatible": candidate["license"]["benchmarkUseCompatible"], "evidence": candidate["evidence"], "id": candidate["license"]["id"]}, "license custody drift")
    require(license_record["benchmarkUseCompatible"] is True, "model license is not benchmark compatible")
    model = closed(doc["model"], {"artifactIdentitySha256", "bytes", "fileCount", "files", "id", "repository", "revision", "role"}, "model")
    require(model == {**artifact, "repository": candidate["repository"], "role": candidate["selection"]["role"]}, "model custody drift")
    isolation = closed(doc["isolation"], {"acceleratorPolicy", "backend", "failClosedOnUnsupportedHost", "modelRootForwardedToAgent", "network", "readPolicy", "writePolicy"}, "isolation")
    require(isolation == {
        "acceleratorPolicy": "apple-metal-system-graphics", "backend": "darwin-sandbox-exec-v0.1", "failClosedOnUnsupportedHost": True,
        "modelRootForwardedToAgent": False, "network": "two-exact-loopback-ports-no-other-network",
        "readPolicy": "closed-system-runtime-model-stage-roots", "writePolicy": "closed-ephemeral-stage-roots",
    }, "isolation policy drift")
    require(doc["policy"] == {
        "authorizationHeadersAllowed": False, "hiddenRetriesAllowed": False,
        "maxBackendRequestsPerAgentTurn": 1, "serverFallbackAllowed": False, "streamRetries": 0,
        "trustRemoteCodeAllowed": False, "webSearchAllowed": False,
    }, "local execution policy drift")
    runtime = closed(doc["runtime"], {"bytes", "distributions", "executableSha256", "fileCount", "implementation", "pythonVersion", "runtimeIdentitySha256", "totalDistributionBytes"}, "runtime")
    require(SHA_RE.fullmatch(runtime["executableSha256"] or "") is not None, "invalid Python executable digest")
    require(isinstance(runtime["bytes"], int) and runtime["bytes"] > 0, "invalid Python executable bytes")
    require(isinstance(runtime["distributions"], list) and runtime["distributions"], "runtime distributions are empty")
    names: list[str] = []
    count = total = 0
    for index, raw in enumerate(runtime["distributions"]):
        dist = closed(raw, {"files", "name", "version"}, f"runtime distribution[{index}]")
        names.append(safe_id(dist["name"], "distribution name")); safe_id(dist["version"], "distribution version")
        rows = validate_file_rows(dist["files"], f"runtime {dist['name']}", max_files=MAX_RUNTIME_FILES, max_bytes=MAX_RUNTIME_BYTES)
        count += len(rows); total += sum(row["bytes"] for row in rows)
    require(names == sorted(set(names)) and {"mlx", "mlx-lm"}.issubset(names), "runtime distribution closure drift")
    require(count == runtime["fileCount"] and total == runtime["totalDistributionBytes"], "runtime aggregate drift")
    require(runtime["runtimeIdentitySha256"] == rows_identity(runtime["distributions"]), "runtime identity drift")
    if check_local:
        require(model_root is not None and python is not None, "local verification requires model root and Python")
        require(genesisbench_local_models.artifact_rows(model_root.resolve()) == model["files"], "local model byte substitution")
        require(runtime_probe(python) == runtime, "local MLX runtime byte substitution")
        require(adapter_rows() == adapters, "local adapter byte substitution")
    return doc


def quote_sb(path: Path) -> str:
    value = str(path.resolve())
    require('"' not in value and "\n" not in value, "sandbox path cannot be represented safely")
    return '"' + value.replace("\\", "\\\\") + '"'


def darwin_profile(
    *, read_roots: list[Path], write_roots: list[Path], connect_ports: list[int],
    listen_ports: list[int], allow_graphics: bool = False,
) -> str:
    require(sys.platform == "darwin" and shutil.which("sandbox-exec") is not None, "Darwin sandbox-exec custody backend is unavailable")
    reads = sorted({quote_sb(path) for path in read_roots})
    writes = sorted({quote_sb(path) for path in write_roots})
    ancestors: set[str] = set()
    for root in [*read_roots, *write_roots]:
        current = root.resolve()
        while current != current.parent:
            ancestors.add(quote_sb(current))
            current = current.parent
    for port in [*connect_ports, *listen_ports]:
        require(isinstance(port, int) and 1024 <= port <= 65535, "invalid sandbox loopback port")
    # Apple's system baseline grants only the runtime services required for a
    # normal process to start; file and network authority remain explicit.
    rules = ["(version 1)", "(deny default)", '(import "system.sb")', "(allow process*)", "(allow signal)"]
    if allow_graphics:
        rules.append("(system-graphics)")
    rules.extend(f"(allow file-read* (subpath {path}))" for path in reads)
    rules.extend(f"(allow file-read-metadata (literal {path}))" for path in sorted(ancestors))
    rules.extend(f"(allow file-write* (subpath {path}))" for path in writes)
    rules.extend(f'(allow network-outbound (remote ip "localhost:{port}"))' for port in sorted(set(connect_ports)))
    rules.extend(f'(allow network-inbound (local ip "localhost:{port}"))' for port in sorted(set(listen_ports)))
    return "\n".join(rules) + "\n"


def sandbox_prefix(profile: str) -> list[str]:
    executable = shutil.which("sandbox-exec")
    require(executable is not None, "sandbox-exec is unavailable")
    return [executable, "-p", profile]


def self_test() -> int:
    controls = 0
    base = capture_fixture()
    for label, mutate in (
        ("identity", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64)),
        ("model", lambda d: d["model"].__setitem__("revision", "forged")),
        ("license", lambda d: d["license"].__setitem__("benchmarkUseCompatible", False)),
        ("runtime", lambda d: d["runtime"]["distributions"][0]["files"][0].__setitem__("sha256", "0" * 64)),
        ("adapter", lambda d: d["adapter"]["files"].pop()),
        ("fallback", lambda d: d["policy"].__setitem__("serverFallbackAllowed", True)),
        ("auth", lambda d: d["policy"].__setitem__("authorizationHeadersAllowed", True)),
    ):
        candidate = copy.deepcopy(base); mutate(candidate)
        if label != "identity": candidate["contentIdentitySha256"] = object_identity(candidate)
        try:
            validate(candidate)
        except CustodyError:
            controls += 1
        else:
            raise CustodyError(f"negative custody control accepted: {label}")
    if sys.platform == "darwin" and shutil.which("sandbox-exec"):
        with tempfile.TemporaryDirectory(prefix="genesisbench-custody-sandbox-") as raw:
            root = Path(raw); allowed = root / "allowed"; denied = root / "denied"
            allowed.mkdir(); denied.mkdir(); (allowed / "read").write_text("ok", encoding="ascii"); (denied / "secret").write_text("secret", encoding="ascii")
            profile = darwin_profile(read_roots=[Path("/System"), Path("/usr"), Path("/bin"), allowed], write_roots=[allowed], connect_ports=[], listen_ports=[])
            allowed_read = (allowed / "read").resolve()
            denied_secret = (denied / "secret").resolve()
            denied_write = denied.resolve() / "write"
            ok = subprocess.run([*sandbox_prefix(profile), "/bin/cat", str(allowed_read)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
            blocked_read = subprocess.run([*sandbox_prefix(profile), "/bin/cat", str(denied_secret)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
            blocked_write = subprocess.run([*sandbox_prefix(profile), "/usr/bin/touch", str(denied_write)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
            require(ok.returncode == 0 and ok.stdout == b"ok", "sandbox denied declared read")
            require(blocked_read.returncode != 0 and blocked_write.returncode != 0 and not (denied / "write").exists(), "sandbox admitted ambient file authority")
            controls += 3
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as allowed_listener, socket.socket(socket.AF_INET, socket.SOCK_STREAM) as denied_listener:
                allowed_listener.bind(("127.0.0.1", 0)); allowed_listener.listen(1)
                denied_listener.bind(("127.0.0.1", 0)); denied_listener.listen(1)
                allowed_port = int(allowed_listener.getsockname()[1])
                denied_port = int(denied_listener.getsockname()[1])
                network_profile = darwin_profile(
                    read_roots=[Path("/System"), Path("/usr"), Path("/bin")],
                    write_roots=[], connect_ports=[allowed_port], listen_ports=[],
                )
                network_prefix = sandbox_prefix(network_profile)
                allowed_connect = subprocess.run(
                    [*network_prefix, "/usr/bin/nc", "-z", "127.0.0.1", str(allowed_port)],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False, timeout=5,
                )
                denied_connect = subprocess.run(
                    [*network_prefix, "/usr/bin/nc", "-z", "127.0.0.1", str(denied_port)],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False, timeout=5,
                )
                external_connect = subprocess.run(
                    [*network_prefix, "/usr/bin/nc", "-z", "-w", "1", "1.1.1.1", "53"],
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False, timeout=5,
                )
                require(allowed_connect.returncode == 0, "sandbox denied declared loopback provider port")
                require(denied_connect.returncode != 0, "sandbox admitted undeclared loopback port")
                require(external_connect.returncode != 0, "sandbox admitted external network authority")
                controls += 3
    return controls


def capture_fixture() -> dict[str, Any]:
    """Build a structurally valid fixture without claiming host runtime custody."""
    plan, inventory, candidate, artifact = selected_candidate("qwen3-4b-4bit")
    adapters = adapter_rows()
    runtime_files = [{"bytes": 1, "path": "mlx/__init__.py", "sha256": sha256_bytes(b"x")}]
    distributions = [
        {"files": runtime_files, "name": "mlx", "version": "0.0.0"},
        {"files": [{"bytes": 1, "path": "mlx_lm/__init__.py", "sha256": sha256_bytes(b"y")}], "name": "mlx-lm", "version": "0.0.0"},
    ]
    return identified({
        "adapter": {"files": adapters, "identitySha256": rows_identity(adapters), "protocol": "responses-to-mlx-chat-completions-v0.1"},
        "contentIdentitySha256": "", "inventoryIdentitySha256": inventory["contentIdentitySha256"],
        "isolation": {"acceleratorPolicy": "apple-metal-system-graphics", "backend": "darwin-sandbox-exec-v0.1", "failClosedOnUnsupportedHost": True, "modelRootForwardedToAgent": False, "network": "two-exact-loopback-ports-no-other-network", "readPolicy": "closed-system-runtime-model-stage-roots", "writePolicy": "closed-ephemeral-stage-roots"},
        "kind": KIND,
        "license": {"benchmarkUseCompatible": candidate["license"]["benchmarkUseCompatible"], "evidence": candidate["evidence"], "id": candidate["license"]["id"]},
        "model": {**artifact, "repository": candidate["repository"], "role": candidate["selection"]["role"]},
        "policy": {"authorizationHeadersAllowed": False, "hiddenRetriesAllowed": False, "maxBackendRequestsPerAgentTurn": 1, "serverFallbackAllowed": False, "streamRetries": 0, "trustRemoteCodeAllowed": False, "webSearchAllowed": False},
        "preselectionIdentitySha256": plan["contentIdentitySha256"],
        "runtime": {"bytes": 1, "distributions": distributions, "executableSha256": sha256_bytes(b"python"), "fileCount": 2, "implementation": "CPython", "pythonVersion": "0.0.0", "runtimeIdentitySha256": rows_identity(distributions), "totalDistributionBytes": 2},
        "version": "0.1.0",
    })


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument("--capture", action="store_true")
    modes.add_argument("--check", action="store_true")
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--model-id")
    parser.add_argument("--model-root", type=Path)
    parser.add_argument("--python", type=Path)
    parser.add_argument("--verify-local", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    validate_schema()
    if args.capture:
        require(args.model_id and args.model_root and args.python, "capture requires model id, root, and Python")
        require(not args.manifest.exists(), "custody manifest already exists")
        document = capture(args.model_id, args.model_root, args.python)
        args.manifest.parent.mkdir(parents=True, exist_ok=True)
        args.manifest.write_text(json.dumps(document, indent=2, sort_keys=True) + "\n", encoding="ascii")
    else:
        document = validate(load_json(args.manifest), check_local=args.verify_local, model_root=args.model_root, python=args.python)
    controls = self_test() if args.self_test else 0
    print(f"genesisbench-mlx-custody: ok controls={controls} identity={document['contentIdentitySha256']}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (CustodyError, OSError, UnicodeError, json.JSONDecodeError, subprocess.SubprocessError) as exc:
        print(f"genesisbench-mlx-custody: {exc}", file=sys.stderr)
        raise SystemExit(1)
