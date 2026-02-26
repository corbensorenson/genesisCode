#!/usr/bin/env python3
"""Emit deterministic runtime profile reports and enforce elapsed/p95 budgets."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import math
from pathlib import Path
from typing import Any


def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser()
    ap.add_argument("--profile", required=True)
    ap.add_argument("--kind", default="genesis/test-profile-runtime-v0.1")
    ap.add_argument("--report", required=True)
    ap.add_argument("--history", required=True)
    ap.add_argument("--elapsed-ms", required=True, type=int)
    ap.add_argument("--budget-ms", required=True, type=int)
    ap.add_argument("--min-history", required=True, type=int)
    ap.add_argument("--baseline-history", default="")
    ap.add_argument("--history-scope-key", default="")
    ap.add_argument("--require-min-history", action="store_true")
    ap.add_argument("--extra-json", default="")
    return ap.parse_args()


def require_positive(name: str, value: int) -> None:
    if value <= 0:
        raise SystemExit(f"profile-runtime-budget: {name} must be > 0")


def compute_p95(samples: list[int]) -> int:
    idx = max(0, math.ceil(0.95 * len(samples)) - 1)
    return sorted(samples)[idx]


def read_history(
    path: Path,
    profile: str,
    budget_ms: int,
    history_scope_key: str,
) -> list[dict[str, Any]]:
    if not path.is_file():
        return []
    rows: list[dict[str, Any]] = []
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (
            isinstance(row, dict)
            and row.get("profile") == profile
            and isinstance(row.get("elapsed_ms"), int)
            and int(row.get("budget_ms", -1)) == budget_ms
        ):
            if history_scope_key:
                if row.get("history_scope_key") != history_scope_key:
                    continue
            rows.append(row)
    return rows


def parse_extra(extra_json: str) -> dict[str, Any]:
    if not extra_json.strip():
        return {}
    try:
        parsed = json.loads(extra_json)
    except json.JSONDecodeError as exc:
        raise SystemExit(f"profile-runtime-budget: --extra-json is invalid JSON: {exc}") from exc
    if not isinstance(parsed, dict):
        raise SystemExit("profile-runtime-budget: --extra-json must decode to an object")
    return parsed


def main() -> None:
    args = parse_args()
    require_positive("elapsed-ms", args.elapsed_ms)
    require_positive("budget-ms", args.budget_ms)
    require_positive("min-history", args.min_history)

    report_path = Path(args.report)
    history_path = Path(args.history)
    baseline_history_path = Path(args.baseline_history) if args.baseline_history.strip() else None
    history_scope_key = args.history_scope_key.strip()
    extra = parse_extra(args.extra_json)

    history_rows: list[dict[str, Any]] = []
    baseline_rows: list[dict[str, Any]] = []
    if baseline_history_path is not None:
        if not baseline_history_path.is_file():
            raise SystemExit(
                f"profile-runtime-budget: baseline history file not found: {baseline_history_path}"
            )
        baseline_rows = read_history(
            baseline_history_path,
            args.profile,
            args.budget_ms,
            history_scope_key,
        )
        history_rows.extend(baseline_rows)
    history_rows.extend(
        read_history(
            history_path,
            args.profile,
            args.budget_ms,
            history_scope_key,
        )
    )
    now_utc = dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")
    current_row = {
        "kind": args.kind,
        "profile": args.profile,
        "elapsed_ms": args.elapsed_ms,
        "budget_ms": args.budget_ms,
        "timestamp_utc": now_utc,
    }
    if history_scope_key:
        current_row["history_scope_key"] = history_scope_key
    current_row.update(extra)

    report_path.parent.mkdir(parents=True, exist_ok=True)
    history_path.parent.mkdir(parents=True, exist_ok=True)

    with history_path.open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(current_row, sort_keys=True) + "\n")

    history_rows.append(current_row)
    elapsed_samples = [int(row["elapsed_ms"]) for row in history_rows]
    p95_ms = compute_p95(elapsed_samples)

    previous_elapsed = None
    if report_path.is_file():
        try:
            previous_doc = json.loads(report_path.read_text(encoding="utf-8"))
            if (
                isinstance(previous_doc, dict)
                and previous_doc.get("kind") == args.kind
                and previous_doc.get("profile") == args.profile
                and isinstance(previous_doc.get("elapsed_ms"), int)
            ):
                previous_elapsed = int(previous_doc["elapsed_ms"])
        except json.JSONDecodeError:
            previous_elapsed = None

    history_samples = len(elapsed_samples)
    history_p95_enforced = history_samples >= args.min_history
    history_p95_ok = (not history_p95_enforced) or (p95_ms <= args.budget_ms)
    history_min_ok = history_samples >= args.min_history

    elapsed_fail = args.elapsed_ms > args.budget_ms
    p95_fail = history_p95_enforced and p95_ms > args.budget_ms
    min_history_fail = args.require_min_history and not history_min_ok

    fail_reasons: list[str] = []
    if elapsed_fail:
        fail_reasons.append("elapsed-budget")
    if p95_fail:
        fail_reasons.append("history-p95-budget")
    if min_history_fail:
        fail_reasons.append("insufficient-history")

    total_checks = 2 + (1 if args.require_min_history else 0)
    passed_checks = 0
    if not elapsed_fail:
        passed_checks += 1
    if not p95_fail:
        passed_checks += 1
    if not min_history_fail and args.require_min_history:
        passed_checks += 1
    score_percent = round((passed_checks / total_checks) * 100.0, 2)
    ok = not (elapsed_fail or p95_fail or min_history_fail)

    report_doc = {
        "kind": args.kind,
        "profile": args.profile,
        "elapsed_ms": args.elapsed_ms,
        "budget_ms": args.budget_ms,
        "history_samples": history_samples,
        "history_p95_ms": p95_ms,
        "history_p95_enforced": history_p95_enforced,
        "history_p95_ok": history_p95_ok,
        "history_min_ok": history_min_ok,
        "history_file": str(history_path),
        "baseline_history_file": str(baseline_history_path) if baseline_history_path else None,
        "baseline_history_samples": len(baseline_rows),
        "ok": ok,
        "score_percent": score_percent,
        "fail_reasons": fail_reasons,
        "timestamp_utc": now_utc,
    }
    if history_scope_key:
        report_doc["history_scope_key"] = history_scope_key
    if previous_elapsed is not None:
        report_doc["previous_elapsed_ms"] = previous_elapsed
        report_doc["elapsed_delta_ms"] = args.elapsed_ms - previous_elapsed
        report_doc["wall_time_trend_ms"] = report_doc["elapsed_delta_ms"]
    report_doc.update(extra)

    report_path.write_text(
        json.dumps(report_doc, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(
        "profile-runtime-budget: "
        f"profile={args.profile} elapsed_ms={args.elapsed_ms} budget_ms={args.budget_ms} "
        f"history_samples={len(elapsed_samples)} history_p95_ms={p95_ms} report={report_path}"
    )

    if elapsed_fail:
        raise SystemExit(
            f"profile-runtime-budget: elapsed budget exceeded for {args.profile}: "
            f"{args.elapsed_ms} > {args.budget_ms}"
        )
    if min_history_fail:
        raise SystemExit(
            "profile-runtime-budget: insufficient history samples for "
            f"{args.profile}: {len(elapsed_samples)} < {args.min_history}"
        )
    if p95_fail:
        raise SystemExit(
            f"profile-runtime-budget: history p95 budget exceeded for {args.profile}: "
            f"{p95_ms} > {args.budget_ms} with {len(elapsed_samples)} samples"
        )


if __name__ == "__main__":
    main()
