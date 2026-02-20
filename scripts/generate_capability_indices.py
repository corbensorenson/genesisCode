#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re
from collections import defaultdict


HOST_OP_RE = re.compile(r'"([A-Za-z0-9_./-]+::[A-Za-z0-9_./:-]+)"')
PRELUDE_PERFORM_RE = re.compile(
    r"core/caps::perform\s+\(quote\s+([A-Za-z0-9_./-]+::[A-Za-z0-9_./:-]+)\)"
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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True)
    parser.add_argument("--out-host", required=True)
    parser.add_argument("--out-prelude", required=True)
    args = parser.parse_args()

    root = pathlib.Path(args.root).resolve()
    out_host = pathlib.Path(args.out_host).resolve()
    out_prelude = pathlib.Path(args.out_prelude).resolve()
    out_host.parent.mkdir(parents=True, exist_ok=True)
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
