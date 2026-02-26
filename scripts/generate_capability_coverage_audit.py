#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import pathlib
import re
from collections import Counter


def load_json(path: pathlib.Path) -> dict:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise SystemExit(f"capability-coverage-audit: invalid JSON in {path}: {exc}") from exc


def parse_open_upgrade_ids(plan_path: pathlib.Path) -> set[str]:
    text = plan_path.read_text(encoding="utf-8")
    return set(re.findall(r"^- \[ \] (P\d+\.\d+)\b", text, flags=re.MULTILINE))


def ensure_script_exists(root: pathlib.Path, rel: str) -> None:
    path = root / rel
    if not path.is_file():
        raise SystemExit(f"capability-coverage-audit: release gate script missing: {rel}")


def family_for_op(op: str) -> str:
    return op.split("::", 1)[0]


def markdown_table(rows: list[dict]) -> str:
    out = []
    out.append(
        "| Family | Status | Plan ID | Host Ops | Prelude Ops | Host-Only Ops | Release Gates |"
    )
    out.append(
        "|---|---|---|---:|---:|---:|---|"
    )
    for row in rows:
        gates = "<br>".join(row["release_gates"]) if row["release_gates"] else "-"
        out.append(
            f"| `{row['family']}` | `{row['status']}` | `{row['plan_id'] or '-'}` | "
            f"{row['host_ops_count']} | {row['prelude_ops_count']} | {len(row['host_only_ops'])} | {gates} |"
        )
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description="Generate deterministic capability coverage audit")
    ap.add_argument("--root", type=pathlib.Path, default=pathlib.Path("."))
    ap.add_argument("--host-index", type=pathlib.Path, required=True)
    ap.add_argument("--prelude-index", type=pathlib.Path, required=True)
    ap.add_argument("--status", type=pathlib.Path, required=True)
    ap.add_argument("--upgrade-plan", type=pathlib.Path, required=True)
    ap.add_argument("--out-json", type=pathlib.Path, required=True)
    ap.add_argument("--out-md", type=pathlib.Path, required=True)
    args = ap.parse_args()

    root = args.root.resolve()
    host_index = load_json(root / args.host_index)
    prelude_index = load_json(root / args.prelude_index)
    status_doc = load_json(root / args.status)
    open_ids = parse_open_upgrade_ids(root / args.upgrade_plan)

    host_ops = sorted(set(host_index.get("operations", [])))
    prelude_ops = sorted(set(prelude_index.get("operations", [])))
    host_families = sorted(set(host_index.get("families", {}).keys()))
    if not host_ops:
        raise SystemExit("capability-coverage-audit: host index has zero operations")
    if not host_families:
        raise SystemExit("capability-coverage-audit: host index has zero families")

    status_overrides = status_doc.get("families", {})
    unknown_overrides = sorted(set(status_overrides.keys()) - set(host_families))
    if unknown_overrides:
        raise SystemExit(
            "capability-coverage-audit: status file contains unknown families: "
            + ", ".join(unknown_overrides)
        )

    rows: list[dict] = []
    for family in host_families:
        override = status_overrides.get(family, {})
        status = override.get("status", "implemented")
        if status not in {"implemented", "policy-disabled", "planned"}:
            raise SystemExit(
                f"capability-coverage-audit: invalid status `{status}` for family `{family}`"
            )
        plan_id = override.get("plan_id")
        release_gates = override.get("release_gates", [])
        notes = override.get("notes", "")
        if not isinstance(release_gates, list):
            raise SystemExit(
                f"capability-coverage-audit: release_gates must be list for family `{family}`"
            )
        for gate in release_gates:
            if not isinstance(gate, str) or not gate.strip():
                raise SystemExit(
                    f"capability-coverage-audit: invalid release gate entry for family `{family}`"
                )
            ensure_script_exists(root, gate)

        family_host_ops = [op for op in host_ops if family_for_op(op) == family]
        family_prelude_ops = [op for op in prelude_ops if family_for_op(op) == family]
        host_only_ops = sorted(set(family_host_ops) - set(family_prelude_ops))

        if status == "planned":
            if not plan_id:
                raise SystemExit(
                    f"capability-coverage-audit: planned family `{family}` missing plan_id"
                )
            if plan_id not in open_ids:
                raise SystemExit(
                    "capability-coverage-audit: planned family "
                    f"`{family}` points to non-open plan id `{plan_id}`"
                )
            if not release_gates:
                raise SystemExit(
                    f"capability-coverage-audit: planned family `{family}` missing release_gates"
                )
        elif status == "policy-disabled":
            if plan_id:
                raise SystemExit(
                    f"capability-coverage-audit: policy-disabled family `{family}` must not set plan_id"
                )
            if not release_gates:
                raise SystemExit(
                    f"capability-coverage-audit: policy-disabled family `{family}` missing release_gates"
                )
        else:
            if plan_id:
                raise SystemExit(
                    f"capability-coverage-audit: implemented family `{family}` must not set plan_id"
                )

        rows.append(
            {
                "family": family,
                "status": status,
                "plan_id": plan_id,
                "release_gates": sorted(release_gates),
                "host_ops_count": len(family_host_ops),
                "prelude_ops_count": len(family_prelude_ops),
                "host_only_ops": host_only_ops,
                "notes": notes,
            }
        )

    status_counts = Counter(row["status"] for row in rows)
    planned_ids = sorted(
        {row["plan_id"] for row in rows if row["status"] == "planned" and row["plan_id"]}
    )

    audit = {
        "kind": "genesis/capability-coverage-audit-v0.1",
        "generated_from": {
            "host_index": args.host_index.as_posix(),
            "prelude_index": args.prelude_index.as_posix(),
            "status_overrides": args.status.as_posix(),
            "upgrade_plan": args.upgrade_plan.as_posix(),
        },
        "summary": {
            "family_count": len(rows),
            "host_operation_count": len(host_ops),
            "prelude_operation_count": len(prelude_ops),
            "implemented_families": status_counts.get("implemented", 0),
            "policy_disabled_families": status_counts.get("policy-disabled", 0),
            "planned_families": status_counts.get("planned", 0),
            "planned_upgrade_ids": planned_ids,
        },
        "families": rows,
    }

    md_lines = []
    md_lines.append("# Capability Coverage Audit v0.1")
    md_lines.append("")
    md_lines.append("Generated from:")
    md_lines.append(f"- `{args.host_index.as_posix()}`")
    md_lines.append(f"- `{args.prelude_index.as_posix()}`")
    md_lines.append(f"- `{args.status.as_posix()}`")
    md_lines.append(f"- `{args.upgrade_plan.as_posix()}`")
    md_lines.append("")
    md_lines.append("Summary:")
    md_lines.append(
        f"- Families: {audit['summary']['family_count']} "
        f"(implemented={audit['summary']['implemented_families']}, "
        f"policy-disabled={audit['summary']['policy_disabled_families']}, "
        f"planned={audit['summary']['planned_families']})"
    )
    md_lines.append(
        f"- Operations: host={audit['summary']['host_operation_count']} prelude={audit['summary']['prelude_operation_count']}"
    )
    if planned_ids:
        md_lines.append(f"- Planned upgrade IDs: {', '.join(planned_ids)}")
    else:
        md_lines.append("- Planned upgrade IDs: none")
    md_lines.append("")
    md_lines.append("## Coverage Table")
    md_lines.append("")
    md_lines.append(markdown_table(rows))

    out_json = root / args.out_json
    out_md = root / args.out_md
    out_json.parent.mkdir(parents=True, exist_ok=True)
    out_md.parent.mkdir(parents=True, exist_ok=True)
    out_json.write_text(json.dumps(audit, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    out_md.write_text("\n".join(md_lines) + "\n", encoding="utf-8")
    print(
        "capability-coverage-audit: generated "
        f"families={len(rows)} host_ops={len(host_ops)} prelude_ops={len(prelude_ops)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
