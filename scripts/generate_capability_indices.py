#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re
from collections import defaultdict


HOST_OP_RE = re.compile(r'"([A-Za-z0-9_./-]+::[A-Za-z0-9_./:-]+)"')
PRELUDE_PERFORM_RE = re.compile(
    r"core/(?:caps|effect)::perform\s+\(quote\s+([A-Za-z0-9_./-]+::[A-Za-z0-9_./:-]+)\)"
)


def stable_ops_by_family(ops: list[str]) -> dict[str, list[str]]:
    grouped: dict[str, list[str]] = defaultdict(list)
    for op in ops:
        family = op.split("::", 1)[0]
        grouped[family].append(op)
    return {k: sorted(v) for k, v in sorted(grouped.items())}


def extract_host_ops(root: pathlib.Path) -> list[str]:
    files = [
        root / "crates/gc_effects/src/runner_capability_dispatch.rs",
        root / "crates/gc_effects/src/runner_browser_host.rs",
        root / "crates/gc_effects/src/runner_xr_host.rs",
        root / "crates/gc_effects/src/runner_task.rs",
        root / "crates/gc_effects/src/runner_cap_pkg_low.rs",
        root / "crates/gc_effects/src/runner_cap_vcs_low.rs",
        root / "crates/gc_effects/src/runner_cap_gc_gpk_low.rs",
    ]
    found = set()
    for path in files:
        text = path.read_text(encoding="utf-8")
        found.update(HOST_OP_RE.findall(text))
    return sorted(found)


def extract_prelude_capability_ops(root: pathlib.Path) -> list[str]:
    modules = sorted((root / "prelude/modules").glob("*.gc"))
    found = set()
    for path in modules:
        text = path.read_text(encoding="utf-8")
        found.update(PRELUDE_PERFORM_RE.findall(text))
    return sorted(found)


def field(name: str, typ: str, constraints: list[str] | None = None) -> dict[str, object]:
    out: dict[str, object] = {"name": name, "type": typ}
    if constraints:
        out["constraints"] = constraints
    return out


def default_schema_entry(op: str) -> dict[str, object]:
    return {
        "operation": op,
        "payload": {
            "type": "map",
            "required_fields": [],
            "optional_fields": [],
            "constraints": [
                "deny-by-default policy gate applies",
                "payload is a CoreForm map unless otherwise documented",
            ],
        },
        "response_envelope": {
            "success": {
                "value_kind": "term",
                "shape": "op-specific data term (or nil) on success",
            },
            "error": {
                "sealed": True,
                "code_field": ":error/code",
                "code_prefix": "core/caps/",
            },
        },
    }


def apply_schema_override(entry: dict[str, object], override: dict[str, object]) -> dict[str, object]:
    payload = entry["payload"]
    response = entry["response_envelope"]
    if "payload" in override:
        payload_override = override["payload"]
        if isinstance(payload, dict) and isinstance(payload_override, dict):
            payload.update(payload_override)
    if "response_envelope" in override:
        response_override = override["response_envelope"]
        if isinstance(response, dict) and isinstance(response_override, dict):
            response.update(response_override)
    return entry


def explicit_host_schema_overrides() -> dict[str, dict[str, object]]:
    return {
        "browser/window::open": {
            "payload": {
                "required_fields": [],
                "optional_fields": [field(":opts", "map")],
            },
            "response_envelope": {
                "success": {
                    "value_kind": "map",
                    "shape": "{:ok bool :window-id string :width int :height int :title string :visible bool ...}",
                }
            },
        },
        "browser/window::close": {
            "payload": {
                "required_fields": [field(":window-id", "string", ["non-empty"])],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :window-id string :closed bool ...}"}
            },
        },
        "browser/window::info": {
            "payload": {
                "required_fields": [field(":window-id", "string", ["non-empty"])],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {
                    "value_kind": "map",
                    "shape": "{:ok bool :window-id string :width int :height int :title string :visible bool :open bool ...}",
                }
            },
        },
        "browser/input::poll": {
            "payload": {
                "required_fields": [field(":window-id", "string", ["non-empty"])],
                "optional_fields": [field(":max-events", "int")],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :window-id string :events vector ...}"}
            },
        },
        "browser/audio::set-master": {
            "payload": {
                "required_fields": [],
                "optional_fields": [field(":gain", "int")],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :gain int ...}"}
            },
        },
        "browser/audio::enqueue": {
            "payload": {
                "required_fields": [],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :queued int ...}"}
            },
        },
        "browser/storage::set": {
            "payload": {
                "required_fields": [field(":key", "string", ["non-empty"]), field(":value", "term")],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :key string :stored bool ...}"}
            },
        },
        "browser/storage::get": {
            "payload": {
                "required_fields": [field(":key", "string", ["non-empty"])],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :key string :found bool :value term|nil ...}"}
            },
        },
        "browser/storage::delete": {
            "payload": {
                "required_fields": [field(":key", "string", ["non-empty"])],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :key string :deleted bool ...}"}
            },
        },
        "gfx/xr::session-open": {
            "payload": {
                "required_fields": [],
                "optional_fields": [field(":opts", "map")],
            },
            "response_envelope": {
                "success": {
                    "value_kind": "map",
                    "shape": "{:ok bool :session-id string :mode string :reference-space string :backend string :adapter string ...}",
                }
            },
        },
        "gfx/xr::frame-poll": {
            "payload": {
                "required_fields": [field(":session-id", "string", ["non-empty"])],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {
                    "value_kind": "map",
                    "shape": "{:ok bool :session-id string :frame {:frame-index int :predicted-display-time-ms int :views vector} :backend string :adapter string ...}",
                }
            },
        },
        "gfx/xr::input-poll": {
            "payload": {
                "required_fields": [field(":session-id", "string", ["non-empty"])],
                "optional_fields": [field(":max-inputs", "int")],
            },
            "response_envelope": {
                "success": {
                    "value_kind": "map",
                    "shape": "{:ok bool :session-id string :inputs vector :backend string :adapter string ...}",
                }
            },
        },
        "gfx/xr::submit-frame": {
            "payload": {
                "required_fields": [
                    field(":session-id", "string", ["non-empty"]),
                    field(":frame", "map"),
                ],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {
                    "value_kind": "map",
                    "shape": "{:ok bool :session-id string :accepted bool :frame-index int :submitted-frames int :backend string :adapter string ...}",
                }
            },
        },
        "gfx/xr::session-close": {
            "payload": {
                "required_fields": [field(":session-id", "string", ["non-empty"])],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :session-id string :closed bool :backend string :adapter string ...}"}
            },
        },
        "io/fs::read": {
            "payload": {
                "required_fields": [
                    field(":path", "string", ["non-empty", "sandboxed-under-base_dir"])
                ],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {"value_kind": "bytes", "shape": "file bytes"},
            },
        },
        "io/fs::write": {
            "payload": {
                "required_fields": [
                    field(":path", "string", ["non-empty", "sandboxed-under-base_dir"]),
                    field(":data", "bytes|string"),
                ],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {"value_kind": "nil", "shape": "nil"},
            },
        },
        "io/fs::stat": {
            "payload": {
                "required_fields": [
                    field(":path", "string", ["non-empty", "sandboxed-under-base_dir"])
                ],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {
                    "value_kind": "map",
                    "shape": "{:path string :exists bool :kind symbol :len-bytes int :readonly bool}",
                },
            },
        },
        "io/fs::list": {
            "payload": {
                "required_fields": [
                    field(":path", "string", ["non-empty", "sandboxed-under-base_dir"])
                ],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {
                    "value_kind": "vector",
                    "shape": "[{:name string :path string :kind symbol :len-bytes int} ...]",
                },
            },
        },
        "io/fs::mkdir": {
            "payload": {
                "required_fields": [
                    field(":path", "string", ["non-empty", "sandboxed-under-base_dir"])
                ],
                "optional_fields": [field(":parents", "bool")],
            },
            "response_envelope": {"success": {"value_kind": "nil", "shape": "nil"}},
        },
        "io/fs::remove": {
            "payload": {
                "required_fields": [
                    field(":path", "string", ["non-empty", "sandboxed-under-base_dir"])
                ],
                "optional_fields": [field(":recursive", "bool")],
            },
            "response_envelope": {"success": {"value_kind": "nil", "shape": "nil"}},
        },
        "io/fs::rename": {
            "payload": {
                "required_fields": [
                    field(":from", "string", ["non-empty", "sandboxed-under-base_dir"]),
                    field(":to", "string", ["non-empty", "sandboxed-under-base_dir"]),
                ],
                "optional_fields": [field(":overwrite", "bool")],
            },
            "response_envelope": {"success": {"value_kind": "nil", "shape": "nil"}},
        },
        "io/db::connect": {
            "payload": {
                "required_fields": [
                    field(":target", "string", ["non-empty", "db_target_allow policy required"])
                ],
                "optional_fields": [field(":mode", "symbol|string")],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :connection-id string ...}"}
            },
        },
        "io/db::tx-begin": {
            "payload": {
                "required_fields": [field(":connection-id", "string")],
                "optional_fields": [field(":isolation", "symbol|string")],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :tx-id string ...}"}
            },
        },
        "io/db::query": {
            "payload": {
                "required_fields": [
                    field(":connection-id", "string"),
                    field(":query-class", "symbol|string", ["allow_query_classes policy required"]),
                    field(":query", "string"),
                    field(":max-row-count", "int", ["injected from max_row_count policy bound"]),
                    field(
                        ":max-result-bytes",
                        "int",
                        ["injected from max_result_bytes policy bound"],
                    ),
                ],
                "optional_fields": [field(":params", "term"), field(":tx-id", "string")],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :rows vector :row-count int ...}"}
            },
        },
        "io/db::exec": {
            "payload": {
                "required_fields": [
                    field(":connection-id", "string"),
                    field(":query-class", "symbol|string", ["allow_query_classes policy required"]),
                    field(":statement", "string"),
                    field(
                        ":max-result-bytes",
                        "int",
                        ["injected from max_result_bytes policy bound"],
                    ),
                ],
                "optional_fields": [field(":params", "term"), field(":tx-id", "string")],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :affected-rows int ...}"}
            },
        },
        "io/db::tx-commit": {
            "payload": {
                "required_fields": [field(":tx-id", "string")],
                "optional_fields": [],
            }
        },
        "io/db::tx-rollback": {
            "payload": {
                "required_fields": [field(":tx-id", "string")],
                "optional_fields": [],
            }
        },
        "io/db::kv-open": {
            "payload": {
                "required_fields": [
                    field(":target", "string", ["non-empty", "db_target_allow policy required"])
                ],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :store-id string ...}"}
            },
        },
        "io/db::kv-get": {
            "payload": {
                "required_fields": [
                    field(":store-id", "string"),
                    field(":key", "string"),
                    field(
                        ":max-result-bytes",
                        "int",
                        ["injected from max_result_bytes policy bound"],
                    ),
                ],
                "optional_fields": [],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "{:ok bool :found bool :value term ...}"}
            },
        },
        "io/db::kv-put": {
            "payload": {
                "required_fields": [
                    field(":store-id", "string"),
                    field(":key", "string"),
                    field(":value", "term"),
                    field(":max-value-bytes", "int", ["injected from max_value_bytes policy bound"]),
                ],
                "optional_fields": [],
            }
        },
        "io/db::kv-delete": {
            "payload": {
                "required_fields": [field(":store-id", "string"), field(":key", "string")],
                "optional_fields": [],
            }
        },
        "io/net::http-request": {
            "payload": {
                "required_fields": [field(":url", "string", ["non-empty", "allowlisted-prefix"])],
                "optional_fields": [
                    field(":method", "string"),
                    field(":headers", "map|vector"),
                    field(":body", "bytes|string"),
                ],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "bridge/http response map"}
            },
        },
        "io/net::dns-resolve": {
            "payload": {
                "required_fields": [field(":name", "string", ["dns-label-or-fqdn"])],
                "optional_fields": [],
            },
            "response_envelope": {"success": {"value_kind": "map", "shape": "dns records map"}},
        },
        "io/net::tcp-open": {
            "payload": {
                "required_fields": [
                    field(":remote", "string", ["tcp://host:port", "allowlisted-prefix"])
                ],
                "optional_fields": [],
            }
        },
        "io/net::tcp-listen": {
            "payload": {
                "required_fields": [
                    field(
                        ":local",
                        "string",
                        [
                            "tcp://host:port",
                            "allowlisted-prefix",
                            "allow_bind_hosts policy required",
                            "allow_bind_ports policy required",
                        ],
                    )
                ],
                "optional_fields": [],
                "constraints": ["policy-injected :max-request-bytes is not applicable"],
            }
        },
        "io/net::tcp-accept": {
            "payload": {
                "required_fields": [
                    field(":listener-id", "string"),
                    field(
                        ":max-request-bytes",
                        "int",
                        ["injected from max_request_bytes policy bound"],
                    ),
                ],
                "optional_fields": [],
            }
        },
        "io/net::tcp-send": {
            "payload": {
                "required_fields": [field(":stream-id", "string"), field(":data", "bytes|string")],
                "optional_fields": [],
            }
        },
        "io/net::tcp-recv": {
            "payload": {"required_fields": [field(":stream-id", "string")], "optional_fields": []}
        },
        "io/net::tcp-close": {
            "payload": {"required_fields": [field(":stream-id", "string")], "optional_fields": []}
        },
        "io/net::udp-bind": {
            "payload": {
                "required_fields": [field(":local", "string", ["udp://ip:port"])],
                "optional_fields": [],
            }
        },
        "io/net::udp-send": {
            "payload": {
                "required_fields": [
                    field(":socket-id", "string"),
                    field(":remote", "string", ["udp://ip:port"]),
                    field(":data", "bytes|string"),
                ],
                "optional_fields": [],
            }
        },
        "io/net::udp-recv": {
            "payload": {"required_fields": [field(":socket-id", "string")], "optional_fields": []}
        },
        "io/net::udp-close": {
            "payload": {"required_fields": [field(":socket-id", "string")], "optional_fields": []}
        },
        "io/net::ws-open": {
            "payload": {
                "required_fields": [field(":url", "string", ["ws:// or wss://", "allowlisted-prefix"])],
                "optional_fields": [],
            }
        },
        "io/net::http-listen": {
            "payload": {
                "required_fields": [
                    field(
                        ":local",
                        "string",
                        [
                            "http://host:port or https://host:port",
                            "allowlisted-prefix",
                            "allow_bind_hosts policy required",
                            "allow_bind_ports policy required",
                        ],
                    ),
                    field(
                        ":max-request-bytes",
                        "int",
                        ["injected from max_request_bytes policy bound"],
                    ),
                ],
                "optional_fields": [],
            }
        },
        "io/net::http-respond": {
            "payload": {
                "required_fields": [
                    field(":listener-id", "string"),
                    field(":request-id", "string"),
                    field(":status", "int"),
                ],
                "optional_fields": [
                    field(":headers", "map|vector"),
                    field(":body", "bytes|string"),
                ],
            }
        },
        "io/net::ws-accept": {
            "payload": {
                "required_fields": [
                    field(":listener-id", "string"),
                    field(":request-id", "string"),
                    field(
                        ":max-request-bytes",
                        "int",
                        ["injected from max_request_bytes policy bound"],
                    ),
                ],
                "optional_fields": [],
            }
        },
        "io/net::ws-send": {
            "payload": {
                "required_fields": [field(":stream-id", "string"), field(":data", "bytes|string")],
                "optional_fields": [],
            }
        },
        "io/net::ws-recv": {
            "payload": {"required_fields": [field(":stream-id", "string")], "optional_fields": []}
        },
        "io/net::ws-close": {
            "payload": {"required_fields": [field(":stream-id", "string")], "optional_fields": []}
        },
        "sys/process::exec": {
            "payload": {
                "required_fields": [field(":program", "string", ["allow_programs policy required"])],
                "optional_fields": [field(":args", "vector<string>"), field(":env", "map<string,string>")],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "bridge process result map"}
            },
        },
        "sys/process::spawn": {
            "payload": {
                "required_fields": [field(":program", "string", ["allow_programs policy required"])],
                "optional_fields": [field(":args", "vector<string>"), field(":env", "map<string,string>")],
            },
            "response_envelope": {
                "success": {"value_kind": "map", "shape": "bridge spawn result map (typically includes :process-id)"}
            },
        },
        "sys/process::wait": {
            "payload": {"required_fields": [field(":process-id", "string")], "optional_fields": []}
        },
        "sys/process::kill": {
            "payload": {"required_fields": [field(":process-id", "string")], "optional_fields": []}
        },
        "sys/process::stdin-write": {
            "payload": {
                "required_fields": [field(":process-id", "string"), field(":data", "bytes|string")],
                "optional_fields": [],
            }
        },
        "sys/process::stdout-read": {
            "payload": {"required_fields": [field(":process-id", "string")], "optional_fields": []}
        },
        "sys/process::stderr-read": {
            "payload": {"required_fields": [field(":process-id", "string")], "optional_fields": []}
        },
        "host/plugin::command": {
            "payload": {
                "required_fields": [
                    field(":plugin", "string|symbol", ["allow_plugins policy required"]),
                    field(":command", "string|symbol"),
                ],
                "optional_fields": [
                    field(":payload", "term"),
                    field(
                        ":request-schema-id",
                        "string|symbol",
                        ["alias :request-schema accepted", "requires allow_schema_ids when present"],
                    ),
                    field(
                        ":response-schema-id",
                        "string|symbol",
                        ["alias :response-schema accepted", "requires allow_schema_ids when present"],
                    ),
                ],
            }
        },
        "editor/plugin::command": {
            "payload": {
                "required_fields": [
                    field(":plugin", "string|symbol", ["allow_plugins policy required"]),
                    field(":command", "string|symbol"),
                ],
                "optional_fields": [
                    field(":payload", "term"),
                    field(
                        ":request-schema-id",
                        "string|symbol",
                        ["alias :request-schema accepted", "requires allow_schema_ids when present"],
                    ),
                    field(
                        ":response-schema-id",
                        "string|symbol",
                        ["alias :response-schema accepted", "requires allow_schema_ids when present"],
                    ),
                ],
            }
        },
        "core/store::put": {
            "payload": {"required_fields": [field(":artifact", "term")], "optional_fields": []},
            "response_envelope": {"success": {"value_kind": "map", "shape": "{:hash hex64}"}},
        },
        "core/store::get": {
            "payload": {"required_fields": [field(":hash", "hex64")], "optional_fields": []},
            "response_envelope": {"success": {"value_kind": "map", "shape": "{:artifact term}"}},
        },
        "core/store::has": {
            "payload": {"required_fields": [field(":hash", "hex64")], "optional_fields": []},
            "response_envelope": {"success": {"value_kind": "map", "shape": "{:present bool}"}},
        },
        "core/refs::get": {
            "payload": {"required_fields": [field(":name", "string")], "optional_fields": []}
        },
        "core/refs::set": {
            "payload": {
                "required_fields": [field(":name", "string"), field(":hash", "hex64")],
                "optional_fields": [field(":policy", "hex64"), field(":expected-old", "hex64|nil")],
            }
        },
    }


def build_host_schema_index(host_ops: list[str]) -> dict[str, object]:
    overrides = explicit_host_schema_overrides()
    schemas: dict[str, dict[str, object]] = {}
    for op in host_ops:
        entry = default_schema_entry(op)
        override = overrides.get(op)
        if override is not None:
            entry = apply_schema_override(entry, override)
        schemas[op] = entry
    return {
        "kind": "genesis/host-abi-schema-index-v0.1",
        "generated_from": [
            "crates/gc_effects/src/runner_capability_dispatch.rs",
            "crates/gc_effects/src/runner_task.rs",
            "crates/gc_effects/src/runner_cap_pkg_low.rs",
            "crates/gc_effects/src/runner_cap_vcs_low.rs",
            "crates/gc_effects/src/runner_cap_gc_gpk_low.rs",
            "docs/spec/HOST_ABI.md",
        ],
        "operations": schemas,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    parser.add_argument("--out-host", required=True)
    parser.add_argument("--out-host-schema", required=True)
    parser.add_argument("--out-prelude", required=True)
    args = parser.parse_args()

    root = pathlib.Path(args.root).resolve()
    out_host = pathlib.Path(args.out_host).resolve()
    out_host_schema = pathlib.Path(args.out_host_schema).resolve()
    out_prelude = pathlib.Path(args.out_prelude).resolve()
    out_host.parent.mkdir(parents=True, exist_ok=True)
    out_host_schema.parent.mkdir(parents=True, exist_ok=True)
    out_prelude.parent.mkdir(parents=True, exist_ok=True)

    host_ops = extract_host_ops(root)
    prelude_ops = extract_prelude_capability_ops(root)

    host_payload = {
        "kind": "genesis/host-abi-index-v0.1",
        "generated_from": [
            "crates/gc_effects/src/runner_capability_dispatch.rs",
            "crates/gc_effects/src/runner_task.rs",
            "crates/gc_effects/src/runner_cap_pkg_low.rs",
            "crates/gc_effects/src/runner_cap_vcs_low.rs",
            "crates/gc_effects/src/runner_cap_gc_gpk_low.rs",
        ],
        "operations": host_ops,
        "families": stable_ops_by_family(host_ops),
    }
    out_host.write_text(json.dumps(host_payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    out_host_schema.write_text(
        json.dumps(build_host_schema_index(host_ops), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    prelude_payload = {
        "kind": "genesis/prelude-capability-index-v0.1",
        "generated_from_glob": "prelude/modules/*.gc",
        "operations": prelude_ops,
        "families": stable_ops_by_family(prelude_ops),
    }
    out_prelude.write_text(
        json.dumps(prelude_payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
