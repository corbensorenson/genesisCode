#!/usr/bin/env python3
"""Evaluate CI liveness from GitHub Actions run metadata without trusting CI itself."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


KIND = "genesis/ci-liveness-watchdog-v0.1"
POLICY_KIND = "genesis/ci-control-plane-policy-v0.1"
TERMINAL_CONCLUSIONS = {
    "success",
    "failure",
    "cancelled",
    "timed_out",
    "action_required",
    "neutral",
    "skipped",
    "stale",
    "startup_failure",
}


class WatchdogError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise WatchdogError(message)


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise WatchdogError(f"cannot read JSON {path.name}: {exc}") from exc


def parse_time(value: Any, field: str) -> datetime:
    require(isinstance(value, str) and value, f"{field} must be a timestamp")
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise WatchdogError(f"{field} is not RFC3339") from exc
    require(parsed.tzinfo is not None, f"{field} must include an offset")
    return parsed.astimezone(timezone.utc)


def validate_policy(policy: Any) -> dict[str, Any]:
    require(isinstance(policy, dict), "policy must be an object")
    require(policy.get("kind") == POLICY_KIND and policy.get("version") == "0.1", "policy identity mismatch")
    require(policy.get("branch") == "main", "watchdog branch authority mismatch")
    require(
        policy.get("workflows")
        == {"ci": ".github/workflows/ci.yml", "watchdog": ".github/workflows/ci-watchdog.yml"},
        "watchdog workflow authority mismatch",
    )
    limits = policy.get("limitsSeconds")
    require(
        limits
        == {
            "latestMainPushDisposition": 7200,
            "standardRunDisposition": 7200,
            "fullRunTermination": 3600,
            "successfulFullFreshness": 172800,
            "scheduledFullCadence": 93600,
            "runnerPreflight": 300,
        },
        "watchdog limits mismatch",
    )
    incident = policy.get("historicalIncident")
    require(isinstance(incident, dict), "historical incident authority is missing")
    require(
        set(incident)
        == {
            "path",
            "incidentId",
            "lastSuccessfulRunId",
            "firstAffectedRunId",
            "lastAffectedRunId",
            "failedCount",
            "cancelledCount",
            "recordsSha256",
        },
        "historical incident authority fields are not closed",
    )
    chronology = policy.get("releaseFullChronology")
    require(isinstance(chronology, dict), "release-full chronology authority is missing")
    require(
        set(chronology)
        == {
            "path",
            "chronologyId",
            "firstRunId",
            "lastRunId",
            "failedCount",
            "cancelledCount",
            "successfulCount",
            "recordsSha256",
        },
        "release-full chronology authority fields are not closed",
    )
    standard = policy.get("standardChronology")
    require(isinstance(standard, dict), "standard chronology authority is missing")
    require(
        set(standard)
        == {
            "path",
            "chronologyId",
            "firstRunId",
            "lastRunId",
            "failedCount",
            "cancelledCount",
            "successfulCount",
            "recordsSha256",
        },
        "standard chronology authority fields are not closed",
    )
    return policy


def verify_incident(policy: Any, incident: Any) -> None:
    policy = validate_policy(policy)
    authority = policy["historicalIncident"]
    require(isinstance(incident, dict), "incident disposition must be an object")
    require(
        set(incident)
        == {
            "kind",
            "version",
            "incidentId",
            "repository",
            "workflowPath",
            "window",
            "summary",
            "recordsSha256",
            "records",
            "observedControlPlaneFacts",
            "disposition",
        },
        "incident disposition fields are not closed",
    )
    require(incident["kind"] == "genesis/ci-incident-disposition-v0.1" and incident["version"] == "0.1", "incident identity mismatch")
    require(incident["incidentId"] == authority["incidentId"], "incident id mismatch")
    require(incident["repository"] == policy["repository"] and incident["workflowPath"] == policy["workflows"]["ci"], "incident source mismatch")
    require(
        incident["window"]
        == {
            "afterLastSuccessfulRunId": authority["lastSuccessfulRunId"],
            "fromExclusive": "2026-07-18T23:22:55Z",
            "throughInclusive": "2026-08-04T19:18:27Z",
        },
        "incident window mismatch",
    )
    records = incident["records"]
    require(isinstance(records, list) and records, "incident records are missing")
    expected_fields = {
        "conclusion",
        "createdAt",
        "event",
        "headSha",
        "runAttempt",
        "runId",
        "startedAt",
        "status",
        "updatedAt",
        "url",
    }
    for row in records:
        require(isinstance(row, dict) and set(row) == expected_fields, "incident record fields are not closed")
        require(row["status"] == "completed" and row["conclusion"] in {"failure", "cancelled"}, "incident record is not an affected terminal run")
        require(row["event"] in {"push", "schedule", "workflow_dispatch"}, "incident event is invalid")
        require(isinstance(row["runId"], int) and isinstance(row["runAttempt"], int), "incident run identity is invalid")
        require(row["url"] == f"https://github.com/corbensorenson/genesisCode/actions/runs/{row['runId']}", "incident URL mismatch")
        parse_time(row["createdAt"], "incident.createdAt")
        parse_time(row["startedAt"], "incident.startedAt")
        parse_time(row["updatedAt"], "incident.updatedAt")
    require(records == sorted(records, key=lambda row: (row["createdAt"], row["runId"])), "incident records are not chronological")
    require(records[0]["runId"] == authority["firstAffectedRunId"] and records[-1]["runId"] == authority["lastAffectedRunId"], "incident boundary run mismatch")
    digest = hashlib.sha256(json.dumps(records, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    require(digest == incident["recordsSha256"] == authority["recordsSha256"], "incident record digest mismatch")
    summary = {
        "cancelled": sum(row["conclusion"] == "cancelled" for row in records),
        "failed": sum(row["conclusion"] == "failure" for row in records),
        "total": len(records),
    }
    require(
        summary
        == incident["summary"]
        == {
            "cancelled": authority["cancelledCount"],
            "failed": authority["failedCount"],
            "total": authority["cancelledCount"] + authority["failedCount"],
        },
        "incident summary mismatch",
    )
    require(
        incident["observedControlPlaneFacts"]
        == {
            "registeredSelfHostedRunnerCount": 0,
            "sharedPendingConcurrencyClasses": ["push", "schedule", "workflow_dispatch"],
            "unconditionalSelfHostedLabels": ["self-hosted", "linux", "x64", "gpu"],
        },
        "incident control-plane facts mismatch",
    )
    disposition = incident["disposition"]
    require(isinstance(disposition, dict) and disposition.get("status") == "retained-with-open-corrective-action", "incident status must retain open corrective action")
    require(disposition.get("correctiveAction") == "R0.4.j/P1.6", "incident corrective action mismatch")
    require(disposition.get("subsequentMainPushSuccessRunIds") == [30950817979, 30958624492], "incident subsequent disposition mismatch")
    nonclaims = disposition.get("remainingNonclaims")
    require(isinstance(nonclaims, list) and len(nonclaims) == 3 and all(isinstance(row, str) and row for row in nonclaims), "incident nonclaims are incomplete")


def verify_release_full_chronology(policy: Any, chronology: Any) -> None:
    policy = validate_policy(policy)
    authority = policy["releaseFullChronology"]
    require(isinstance(chronology, dict), "release-full chronology must be an object")
    require(
        set(chronology)
        == {
            "kind",
            "version",
            "chronologyId",
            "repository",
            "workflowPath",
            "selection",
            "summary",
            "recordsSha256",
            "records",
            "disposition",
        },
        "release-full chronology fields are not closed",
    )
    require(
        chronology["kind"] == "genesis/ci-release-full-chronology-v0.1"
        and chronology["version"] == "0.1",
        "release-full chronology identity mismatch",
    )
    require(chronology["chronologyId"] == authority["chronologyId"], "release-full chronology id mismatch")
    require(
        chronology["repository"] == policy["repository"]
        and chronology["workflowPath"] == policy["workflows"]["ci"],
        "release-full chronology source mismatch",
    )
    selection = chronology["selection"]
    require(
        isinstance(selection, dict)
        and set(selection)
        == {
            "branch",
            "events",
            "profile",
            "fromRunIdInclusive",
            "throughRunIdInclusive",
            "observedAt",
        },
        "release-full chronology selection is not closed",
    )
    require(
        selection["branch"] == policy["branch"]
        and selection["events"] == ["schedule", "workflow_dispatch"]
        and selection["profile"] == "full"
        and selection["fromRunIdInclusive"] == authority["firstRunId"]
        and selection["throughRunIdInclusive"] == authority["lastRunId"],
        "release-full chronology selection mismatch",
    )
    observed_at = parse_time(selection["observedAt"], "chronology.selection.observedAt")
    records = chronology["records"]
    require(isinstance(records, list) and records, "release-full chronology records are missing")
    expected_fields = {
        "conclusion",
        "createdAt",
        "displayTitle",
        "event",
        "headSha",
        "runAttempt",
        "runId",
        "startedAt",
        "status",
        "updatedAt",
        "url",
    }
    seen_ids: set[int] = set()
    for row in records:
        require(isinstance(row, dict) and set(row) == expected_fields, "release-full chronology record fields are not closed")
        require(
            row["status"] == "completed" and row["conclusion"] in {"failure", "cancelled", "success"},
            "release-full chronology record is not terminal",
        )
        require(row["event"] in {"schedule", "workflow_dispatch"}, "release-full chronology event is invalid")
        require(row["displayTitle"] == f"ci / {row['event']} / full", "release-full chronology profile title mismatch")
        require(
            isinstance(row["runId"], int)
            and row["runId"] > 0
            and row["runId"] not in seen_ids
            and isinstance(row["runAttempt"], int)
            and row["runAttempt"] > 0,
            "release-full chronology run identity is invalid or duplicated",
        )
        seen_ids.add(row["runId"])
        require(isinstance(row["headSha"], str) and len(row["headSha"]) == 40, "release-full chronology head is invalid")
        require(
            row["url"] == f"https://github.com/{policy['repository']}/actions/runs/{row['runId']}",
            "release-full chronology URL mismatch",
        )
        created = parse_time(row["createdAt"], "chronology.createdAt")
        started = parse_time(row["startedAt"], "chronology.startedAt")
        updated = parse_time(row["updatedAt"], "chronology.updatedAt")
        require(created <= started <= updated <= observed_at, "release-full chronology timestamps are inconsistent")
    require(records == sorted(records, key=lambda row: (row["createdAt"], row["runId"])), "release-full chronology records are not chronological")
    require(
        records[0]["runId"] == authority["firstRunId"]
        and records[-1]["runId"] == authority["lastRunId"]
        and records[-1]["conclusion"] == "success",
        "release-full chronology boundary run mismatch",
    )
    digest = hashlib.sha256(json.dumps(records, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    require(digest == chronology["recordsSha256"] == authority["recordsSha256"], "release-full chronology digest mismatch")
    summary = {
        "cancelled": sum(row["conclusion"] == "cancelled" for row in records),
        "failed": sum(row["conclusion"] == "failure" for row in records),
        "successful": sum(row["conclusion"] == "success" for row in records),
        "total": len(records),
    }
    require(
        summary
        == chronology["summary"]
        == {
            "cancelled": authority["cancelledCount"],
            "failed": authority["failedCount"],
            "successful": authority["successfulCount"],
            "total": authority["cancelledCount"] + authority["failedCount"] + authority["successfulCount"],
        },
        "release-full chronology summary mismatch",
    )
    disposition = chronology["disposition"]
    require(
        isinstance(disposition, dict)
        and set(disposition)
        == {
            "status",
            "correctiveAction",
            "firstSuccessfulV02RunId",
            "exactMainSuccessRunId",
            "independentWatchdogRunId",
            "remainingNonclaims",
        },
        "release-full chronology disposition is not closed",
    )
    require(disposition["status"] == "retained-with-corrected-design-observation", "release-full chronology status mismatch")
    require(disposition["correctiveAction"] == "R0.4.j/P1.6", "release-full chronology corrective action mismatch")
    successes = [row["runId"] for row in records if row["conclusion"] == "success"]
    require(
        successes
        and disposition["firstSuccessfulV02RunId"] == successes[0]
        and disposition["exactMainSuccessRunId"] == records[-1]["runId"]
        and isinstance(disposition["independentWatchdogRunId"], int)
        and disposition["independentWatchdogRunId"] > records[-1]["runId"],
        "release-full chronology success disposition mismatch",
    )
    nonclaims = disposition["remainingNonclaims"]
    require(
        isinstance(nonclaims, list)
        and len(nonclaims) == 3
        and all(isinstance(row, str) and row for row in nonclaims),
        "release-full chronology nonclaims are incomplete",
    )


def verify_standard_chronology(policy: Any, chronology: Any) -> None:
    policy = validate_policy(policy)
    authority = policy["standardChronology"]
    require(isinstance(chronology, dict), "standard chronology must be an object")
    require(
        set(chronology)
        == {
            "kind",
            "version",
            "chronologyId",
            "repository",
            "workflowPath",
            "selection",
            "summary",
            "recordsSha256",
            "records",
            "disposition",
        },
        "standard chronology fields are not closed",
    )
    require(
        chronology["kind"] == "genesis/ci-standard-chronology-v0.1"
        and chronology["version"] == "0.1",
        "standard chronology identity mismatch",
    )
    require(chronology["chronologyId"] == authority["chronologyId"], "standard chronology id mismatch")
    require(
        chronology["repository"] == policy["repository"]
        and chronology["workflowPath"] == policy["workflows"]["ci"],
        "standard chronology source mismatch",
    )
    selection = chronology["selection"]
    require(
        isinstance(selection, dict)
        and set(selection)
        == {
            "branch",
            "events",
            "profile",
            "fromRunIdInclusive",
            "throughRunIdInclusive",
            "observedAt",
        },
        "standard chronology selection is not closed",
    )
    require(
        selection["branch"] == policy["branch"]
        and selection["events"] == ["workflow_dispatch"]
        and selection["profile"] == "standard"
        and selection["fromRunIdInclusive"] == authority["firstRunId"]
        and selection["throughRunIdInclusive"] == authority["lastRunId"],
        "standard chronology selection mismatch",
    )
    observed_at = parse_time(selection["observedAt"], "standard.selection.observedAt")
    records = chronology["records"]
    require(isinstance(records, list) and records, "standard chronology records are missing")
    expected_fields = {
        "conclusion",
        "createdAt",
        "displayTitle",
        "event",
        "headSha",
        "runAttempt",
        "runId",
        "startedAt",
        "status",
        "updatedAt",
        "url",
    }
    seen_ids: set[int] = set()
    for row in records:
        require(
            isinstance(row, dict) and set(row) == expected_fields,
            "standard chronology record fields are not closed",
        )
        require(
            row["event"] == "workflow_dispatch"
            and row["displayTitle"] == "ci / workflow_dispatch / standard"
            and row["status"] == "completed"
            and row["conclusion"] in {"failure", "cancelled", "success"},
            "standard chronology record is not a terminal standard run",
        )
        require(
            isinstance(row["runId"], int)
            and row["runId"] > 0
            and row["runId"] not in seen_ids
            and isinstance(row["runAttempt"], int)
            and row["runAttempt"] > 0,
            "standard chronology run identity is invalid or duplicated",
        )
        seen_ids.add(row["runId"])
        require(isinstance(row["headSha"], str) and len(row["headSha"]) == 40, "standard chronology head is invalid")
        require(
            row["url"] == f"https://github.com/{policy['repository']}/actions/runs/{row['runId']}",
            "standard chronology URL mismatch",
        )
        created = parse_time(row["createdAt"], "standard.createdAt")
        started = parse_time(row["startedAt"], "standard.startedAt")
        updated = parse_time(row["updatedAt"], "standard.updatedAt")
        require(created <= started <= updated <= observed_at, "standard chronology timestamps are inconsistent")
    require(
        records == sorted(records, key=lambda row: (row["createdAt"], row["runId"])),
        "standard chronology records are not chronological",
    )
    require(
        records[0]["runId"] == authority["firstRunId"]
        and records[-1]["runId"] == authority["lastRunId"],
        "standard chronology boundary run mismatch",
    )
    digest = hashlib.sha256(json.dumps(records, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    require(digest == chronology["recordsSha256"] == authority["recordsSha256"], "standard chronology digest mismatch")
    summary = {
        "cancelled": sum(row["conclusion"] == "cancelled" for row in records),
        "failed": sum(row["conclusion"] == "failure" for row in records),
        "successful": sum(row["conclusion"] == "success" for row in records),
        "total": len(records),
    }
    require(
        summary
        == chronology["summary"]
        == {
            "cancelled": authority["cancelledCount"],
            "failed": authority["failedCount"],
            "successful": authority["successfulCount"],
            "total": authority["cancelledCount"] + authority["failedCount"] + authority["successfulCount"],
        },
        "standard chronology summary mismatch",
    )
    disposition = chronology["disposition"]
    require(
        isinstance(disposition, dict)
        and set(disposition)
        == {
            "status",
            "correctiveAction",
            "counterexampleRunId",
            "subsequentSuccessRunId",
            "remainingNonclaims",
        },
        "standard chronology disposition is not closed",
    )
    by_id = {row["runId"]: row for row in records}
    require(
        disposition["status"] == "retained-with-open-corrective-action"
        and disposition["correctiveAction"] == "R0.4.j/P1.6"
        and by_id.get(disposition["counterexampleRunId"], {}).get("conclusion") == "failure"
        and by_id.get(disposition["subsequentSuccessRunId"], {}).get("conclusion") == "success",
        "standard chronology disposition mismatch",
    )
    nonclaims = disposition["remainingNonclaims"]
    require(
        isinstance(nonclaims, list)
        and len(nonclaims) == 3
        and all(isinstance(row, str) and row for row in nonclaims),
        "standard chronology nonclaims are incomplete",
    )


def verify_retained_evidence(policy_path: Path) -> None:
    policy = read_json(policy_path)
    repository_root = policy_path.parent.parent
    verify_incident(policy, read_json(repository_root / policy["historicalIncident"]["path"]))
    verify_release_full_chronology(policy, read_json(repository_root / policy["releaseFullChronology"]["path"]))
    verify_standard_chronology(policy, read_json(repository_root / policy["standardChronology"]["path"]))


def normalize_runs(payload: Any) -> list[dict[str, Any]]:
    require(isinstance(payload, dict) and isinstance(payload.get("workflow_runs"), list), "workflow run payload is invalid")
    normalized: list[dict[str, Any]] = []
    seen: set[int] = set()
    for raw in payload["workflow_runs"]:
        require(isinstance(raw, dict), "workflow run row must be an object")
        run_id = raw.get("id")
        require(isinstance(run_id, int) and run_id > 0 and run_id not in seen, "workflow run id is invalid or duplicated")
        seen.add(run_id)
        event = raw.get("event")
        status = raw.get("status")
        conclusion = raw.get("conclusion")
        require(event in {"push", "pull_request", "schedule", "workflow_dispatch"}, f"run {run_id}: event is unsupported")
        require(status in {"queued", "in_progress", "completed", "pending", "waiting", "requested"}, f"run {run_id}: status is invalid")
        if status == "completed":
            require(conclusion in TERMINAL_CONCLUSIONS, f"run {run_id}: terminal conclusion is invalid")
        else:
            require(conclusion is None, f"run {run_id}: nonterminal run has a conclusion")
        head_sha = raw.get("head_sha")
        require(isinstance(head_sha, str) and len(head_sha) == 40, f"run {run_id}: head SHA is invalid")
        normalized.append(
            {
                "id": run_id,
                "path": raw.get("path"),
                "event": event,
                "headBranch": raw.get("head_branch"),
                "headSha": head_sha,
                "status": status,
                "conclusion": conclusion,
                "displayTitle": raw.get("display_title", ""),
                "createdAt": parse_time(raw.get("created_at"), f"run {run_id}.created_at"),
                "startedAt": parse_time(raw.get("run_started_at") or raw.get("created_at"), f"run {run_id}.run_started_at"),
                "updatedAt": parse_time(raw.get("updated_at"), f"run {run_id}.updated_at"),
                "attempt": raw.get("run_attempt", 1),
            }
        )
    return normalized


def is_full_run(run: dict[str, Any]) -> bool:
    if run["event"] == "schedule":
        return run["displayTitle"].strip() == "ci / schedule / full"
    return (
        run["event"] == "workflow_dispatch"
        and run["displayTitle"].strip() == "ci / workflow_dispatch / full"
    )


def is_standard_run(run: dict[str, Any]) -> bool:
    return (
        run["event"] == "workflow_dispatch"
        and run["displayTitle"].strip() == "ci / workflow_dispatch / standard"
    )


def violation(code: str, message: str, run_id: int | None = None) -> dict[str, Any]:
    return {"code": code, "message": message, "runId": run_id}


def evaluate(
    policy: Any,
    payload: Any,
    expected_head: str,
    expected_head_time: str,
    main_history: list[str],
    now_text: str,
) -> dict[str, Any]:
    policy = validate_policy(policy)
    require(len(expected_head) == 40 and expected_head in main_history, "expected head is absent from main history")
    require(len(main_history) == len(set(main_history)), "main history contains duplicate revisions")
    require(all(isinstance(sha, str) and len(sha) == 40 for sha in main_history), "main history contains invalid revisions")
    now = parse_time(now_text, "now")
    head_time = parse_time(expected_head_time, "expected_head_time")
    require(head_time <= now, "expected head time is in the future")
    runs = normalize_runs(payload)
    ci_path = policy["workflows"]["ci"]
    branch = policy["branch"]
    limits = policy["limitsSeconds"]
    ci_runs = [run for run in runs if run["path"] == ci_path and run["headBranch"] == branch]
    violations: list[dict[str, Any]] = []

    exact_pushes = sorted(
        [run for run in ci_runs if run["event"] == "push" and run["headSha"] == expected_head],
        key=lambda run: (run["createdAt"], run["id"]),
        reverse=True,
    )
    head_age = int((now - head_time).total_seconds())
    if not exact_pushes:
        if head_age > limits["latestMainPushDisposition"]:
            violations.append(violation("missing-main-disposition", "latest main revision has no CI push disposition"))
    else:
        latest_push = exact_pushes[0]
        run_age = int((now - latest_push["createdAt"]).total_seconds())
        if latest_push["status"] != "completed" and run_age > limits["latestMainPushDisposition"]:
            violations.append(violation("main-disposition-overdue", "latest main CI push is not terminal within its bound", latest_push["id"]))
        if latest_push["status"] == "completed":
            if latest_push["conclusion"] == "cancelled":
                violations.append(violation("cancelled-only-main", "latest main revision has only a cancelled disposition", latest_push["id"]))
            elif latest_push["conclusion"] != "success":
                violations.append(violation("unsuccessful-main-disposition", "latest main revision did not pass CI", latest_push["id"]))

    standard_runs = [run for run in ci_runs if is_standard_run(run)]
    exact_standard = sorted(
        [run for run in standard_runs if run["headSha"] == expected_head],
        key=lambda run: (run["createdAt"], run["id"], run["attempt"]),
        reverse=True,
    )
    wrong_head_standard = [
        run
        for run in standard_runs
        if run["createdAt"] >= head_time
        and run["headSha"] != expected_head
        and run["status"] == "completed"
        and run["conclusion"] == "success"
    ]
    for run in wrong_head_standard:
        violations.append(
            violation(
                "wrong-head-standard-success",
                "a post-head standard success does not match latest main",
                run["id"],
            )
        )
    latest_standard = exact_standard[0] if exact_standard else None
    if latest_standard is None:
        if head_age > limits["standardRunDisposition"]:
            prior_success = any(
                run["status"] == "completed"
                and run["conclusion"] == "success"
                and run["headSha"] in main_history
                for run in standard_runs
            )
            if prior_success:
                violations.append(
                    violation(
                        "superseded-only-standard",
                        "standard success exists only for a superseded main revision",
                    )
                )
            else:
                violations.append(
                    violation(
                        "missing-standard-disposition",
                        "latest main revision has no exact-head standard disposition",
                    )
                )
    else:
        run_age = int((now - latest_standard["createdAt"]).total_seconds())
        if latest_standard["createdAt"] < head_time:
            violations.append(
                violation(
                    "stale-standard-disposition",
                    "exact-head standard disposition predates the latest main revision",
                    latest_standard["id"],
                )
            )
        if latest_standard["status"] != "completed" and run_age > limits["standardRunDisposition"]:
            violations.append(
                violation(
                    "standard-disposition-overdue",
                    "exact-head standard run is not terminal within its bound",
                    latest_standard["id"],
                )
            )
        if latest_standard["status"] == "completed":
            elapsed = int((latest_standard["updatedAt"] - latest_standard["startedAt"]).total_seconds())
            if elapsed > limits["standardRunDisposition"]:
                violations.append(
                    violation(
                        "stale-standard-disposition",
                        "exact-head standard disposition completed after its bound",
                        latest_standard["id"],
                    )
                )
            if latest_standard["conclusion"] == "cancelled":
                violations.append(
                    violation(
                        "cancelled-only-standard",
                        "latest exact-head standard run is cancelled",
                        latest_standard["id"],
                    )
                )
            elif latest_standard["conclusion"] != "success":
                violations.append(
                    violation(
                        "unsuccessful-standard-disposition",
                        "latest exact-head standard run did not pass",
                        latest_standard["id"],
                    )
                )

    full_runs = [run for run in ci_runs if is_full_run(run)]
    for run in full_runs:
        if run["status"] != "completed":
            elapsed = int((now - run["startedAt"]).total_seconds())
            if elapsed > limits["fullRunTermination"]:
                violations.append(violation("full-run-overdue", "full CI run exceeded the terminal-result bound", run["id"]))

    scheduled = sorted([run for run in full_runs if run["event"] == "schedule"], key=lambda run: (run["createdAt"], run["id"]), reverse=True)
    if not scheduled:
        violations.append(violation("missing-full-schedule", "no scheduled full CI run is present"))
    elif int((now - scheduled[0]["createdAt"]).total_seconds()) > limits["scheduledFullCadence"]:
        violations.append(violation("stale-full-schedule", "latest scheduled full CI run exceeds cadence", scheduled[0]["id"]))

    successful_full = sorted(
        [run for run in full_runs if run["status"] == "completed" and run["conclusion"] == "success"],
        key=lambda run: (run["updatedAt"], run["id"]),
        reverse=True,
    )
    valid_successes = [run for run in successful_full if run["headSha"] in main_history]
    for run in successful_full:
        if run["headSha"] not in main_history:
            violations.append(violation("wrong-head-full-success", "successful full run is not on canonical main history", run["id"]))
    if not valid_successes:
        violations.append(violation("missing-successful-full", "no canonical successful full CI run is present"))
    else:
        success_age = int((now - valid_successes[0]["updatedAt"]).total_seconds())
        if success_age > limits["successfulFullFreshness"]:
            violations.append(violation("stale-successful-full", "latest canonical successful full CI run is stale", valid_successes[0]["id"]))

    latest_full = max(full_runs, key=lambda run: (run["createdAt"], run["id"]), default=None)
    if latest_full and latest_full["status"] == "completed":
        if latest_full["conclusion"] == "cancelled":
            violations.append(violation("cancelled-only-full", "latest full CI run is cancelled", latest_full["id"]))
        elif latest_full["conclusion"] != "success":
            violations.append(violation("unsuccessful-latest-full", "latest full CI run did not pass", latest_full["id"]))

    exact_full_successes = sorted(
        [
            run
            for run in valid_successes
            if run["headSha"] == expected_head
        ],
        key=lambda run: (run["updatedAt"], run["id"]),
        reverse=True,
    )
    exact_standard_success_id = (
        latest_standard["id"]
        if latest_standard is not None
        and latest_standard["status"] == "completed"
        and latest_standard["conclusion"] == "success"
        else None
    )
    exact_full_success_id = exact_full_successes[0]["id"] if exact_full_successes else None

    violations.sort(key=lambda row: (row["code"], row["runId"] or 0))
    return {
        "kind": KIND,
        "version": "0.1",
        "status": "pass" if not violations else "fail",
        "source": {
            "repository": policy["repository"],
            "branch": branch,
            "expectedHead": expected_head,
            "expectedHeadTime": expected_head_time,
            "observedAt": now_text,
            "ciWorkflowPath": ci_path,
            "watchdogWorkflowPath": policy["workflows"]["watchdog"],
        },
        "limitsSeconds": limits,
        "observations": {
            "inputRunCount": len(runs),
            "canonicalCiRunCount": len(ci_runs),
            "standardRunCount": len(standard_runs),
            "exactHeadStandardRunCount": len(exact_standard),
            "latestExactHeadStandardRunId": latest_standard["id"] if latest_standard else None,
            "latestExactHeadStandardStatus": latest_standard["status"] if latest_standard else None,
            "latestExactHeadStandardConclusion": latest_standard["conclusion"] if latest_standard else None,
            "exactHeadSuccessfulStandardRunId": exact_standard_success_id,
            "fullRunCount": len(full_runs),
            "scheduledFullRunCount": len(scheduled),
            "canonicalSuccessfulFullRunCount": len(valid_successes),
            "exactHeadSuccessfulFullRunId": exact_full_success_id,
            "exactHeadStandardFullPair": (
                "complete"
                if exact_standard_success_id is not None and exact_full_success_id is not None
                else "incomplete"
            ),
        },
        "violations": violations,
        "releaseQualified": False,
    }


def fixture_run(
    run_id: int,
    *,
    event: str,
    head_sha: str,
    created: str,
    updated: str | None = None,
    status: str = "completed",
    conclusion: str | None = "success",
    path: str = ".github/workflows/ci.yml",
    title: str = "ci / schedule / full",
) -> dict[str, Any]:
    return {
        "id": run_id,
        "path": path,
        "event": event,
        "head_branch": "main",
        "head_sha": head_sha,
        "status": status,
        "conclusion": conclusion,
        "display_title": title,
        "created_at": created,
        "run_started_at": created,
        "updated_at": updated or created,
        "run_attempt": 1,
    }


def expect_code(policy: Any, payload: Any, expected: str, **kwargs: Any) -> None:
    report = evaluate(policy, payload, **kwargs)
    codes = {row["code"] for row in report["violations"]}
    require(expected in codes, f"negative control did not produce {expected}: {sorted(codes)}")


def self_test(policy_path: Path) -> None:
    policy = read_json(policy_path)
    incident_path = policy_path.parent.parent / policy["historicalIncident"]["path"]
    incident = read_json(incident_path)
    verify_incident(policy, incident)
    chronology_path = policy_path.parent.parent / policy["releaseFullChronology"]["path"]
    chronology = read_json(chronology_path)
    verify_release_full_chronology(policy, chronology)
    standard_chronology_path = policy_path.parent.parent / policy["standardChronology"]["path"]
    standard_chronology = read_json(standard_chronology_path)
    verify_standard_chronology(policy, standard_chronology)
    head = "a" * 40
    old_head = "b" * 40
    now = "2026-08-04T12:00:00Z"
    kwargs = {
        "expected_head": head,
        "expected_head_time": "2026-08-04T11:00:00Z",
        "main_history": [head, old_head],
        "now_text": now,
    }
    good_runs = [
        fixture_run(1, event="push", head_sha=head, created="2026-08-04T11:15:00Z", title="ci / push / fast"),
        fixture_run(2, event="schedule", head_sha=old_head, created="2026-08-04T09:00:00Z", updated="2026-08-04T09:45:00Z"),
        fixture_run(3, event="workflow_dispatch", head_sha=head, created="2026-08-04T11:20:00Z", updated="2026-08-04T11:50:00Z", title="ci / workflow_dispatch / standard"),
    ]
    good = {"workflow_runs": good_runs}
    report = evaluate(policy, good, **kwargs)
    require(report["status"] == "pass" and not report["violations"], "passing watchdog control failed")

    missing_main = {"workflow_runs": [good_runs[1]]}
    old_kwargs = dict(kwargs, expected_head_time="2026-08-04T08:00:00Z")
    expect_code(policy, missing_main, "missing-main-disposition", **old_kwargs)

    cancelled = copy.deepcopy(good)
    cancelled["workflow_runs"][0]["conclusion"] = "cancelled"
    expect_code(policy, cancelled, "cancelled-only-main", **kwargs)

    failed_main = copy.deepcopy(good)
    failed_main["workflow_runs"][0]["conclusion"] = "failure"
    expect_code(policy, failed_main, "unsuccessful-main-disposition", **kwargs)

    missing_standard = {"workflow_runs": good_runs[:2]}
    old_kwargs = dict(kwargs, expected_head_time="2026-08-04T08:00:00Z")
    expect_code(policy, missing_standard, "missing-standard-disposition", **old_kwargs)

    superseded_standard = copy.deepcopy(missing_standard)
    superseded_standard["workflow_runs"].append(
        fixture_run(4, event="workflow_dispatch", head_sha=old_head, created="2026-08-04T07:30:00Z", title="ci / workflow_dispatch / standard")
    )
    expect_code(policy, superseded_standard, "superseded-only-standard", **old_kwargs)

    overdue_standard = copy.deepcopy(good)
    overdue_standard["workflow_runs"][2].update(
        status="in_progress",
        conclusion=None,
        created_at="2026-08-04T09:30:00Z",
        run_started_at="2026-08-04T09:30:00Z",
        updated_at="2026-08-04T09:30:00Z",
    )
    expect_code(policy, overdue_standard, "standard-disposition-overdue", **kwargs)

    cancelled_standard = copy.deepcopy(good)
    cancelled_standard["workflow_runs"][2]["conclusion"] = "cancelled"
    expect_code(policy, cancelled_standard, "cancelled-only-standard", **kwargs)

    failed_standard = copy.deepcopy(good)
    failed_standard["workflow_runs"][2]["conclusion"] = "failure"
    expect_code(policy, failed_standard, "unsuccessful-standard-disposition", **kwargs)

    stale_standard = copy.deepcopy(good)
    stale_standard["workflow_runs"][2].update(
        created_at="2026-08-04T08:30:00Z",
        run_started_at="2026-08-04T08:30:00Z",
        updated_at="2026-08-04T11:50:00Z",
    )
    expect_code(policy, stale_standard, "stale-standard-disposition", **kwargs)

    wrong_standard = copy.deepcopy(good)
    wrong_standard["workflow_runs"].append(
        fixture_run(4, event="workflow_dispatch", head_sha="c" * 40, created="2026-08-04T11:30:00Z", title="ci / workflow_dispatch / standard")
    )
    expect_code(policy, wrong_standard, "wrong-head-standard-success", **kwargs)

    superseding_failure = copy.deepcopy(good)
    superseding_failure["workflow_runs"].append(
        fixture_run(4, event="workflow_dispatch", head_sha=head, created="2026-08-04T11:40:00Z", conclusion="failure", title="ci / workflow_dispatch / standard")
    )
    expect_code(policy, superseding_failure, "unsuccessful-standard-disposition", **kwargs)

    exact_pair = copy.deepcopy(good)
    exact_pair["workflow_runs"][1]["head_sha"] = head
    pair_report = evaluate(policy, exact_pair, **kwargs)
    require(
        pair_report["status"] == "pass"
        and pair_report["observations"]["exactHeadStandardFullPair"] == "complete"
        and pair_report["observations"]["exactHeadSuccessfulStandardRunId"] == 3
        and pair_report["observations"]["exactHeadSuccessfulFullRunId"] == 2,
        "exact-head standard/full pair was not recognized",
    )

    failed_full = copy.deepcopy(good)
    failed_full["workflow_runs"][1]["conclusion"] = "failure"
    expect_code(policy, failed_full, "unsuccessful-latest-full", **kwargs)

    overdue = copy.deepcopy(good)
    overdue["workflow_runs"][1].update(status="in_progress", conclusion=None, created_at="2026-08-04T10:00:00Z", run_started_at="2026-08-04T10:00:00Z", updated_at="2026-08-04T10:00:00Z")
    expect_code(policy, overdue, "full-run-overdue", **kwargs)

    stale = copy.deepcopy(good)
    stale["workflow_runs"][1].update(created_at="2026-08-01T08:00:00Z", run_started_at="2026-08-01T08:00:00Z", updated_at="2026-08-01T09:00:00Z")
    expect_code(policy, stale, "stale-successful-full", **kwargs)
    expect_code(policy, stale, "stale-full-schedule", **kwargs)

    wrong_head = copy.deepcopy(good)
    wrong_head["workflow_runs"][1]["head_sha"] = "c" * 40
    expect_code(policy, wrong_head, "wrong-head-full-success", **kwargs)

    no_schedule = {"workflow_runs": [good_runs[0]]}
    expect_code(policy, no_schedule, "missing-full-schedule", **kwargs)

    self_report = copy.deepcopy(good)
    self_report["workflow_runs"][1]["path"] = policy["workflows"]["watchdog"]
    expect_code(policy, self_report, "missing-successful-full", **kwargs)

    unknown_dispatch = copy.deepcopy(good)
    unknown_dispatch["workflow_runs"][1].update(event="workflow_dispatch", display_title="ci / workflow_dispatch / standard")
    expect_code(policy, unknown_dispatch, "missing-full-schedule", **kwargs)

    with tempfile.TemporaryDirectory(prefix="genesis-ci-watchdog-") as directory:
        output = Path(directory) / "report.json"
        output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        require(read_json(output)["releaseQualified"] is False, "watchdog must not qualify releases")
    tampered_incident = copy.deepcopy(incident)
    tampered_incident["records"][0]["conclusion"] = "success"
    try:
        verify_incident(policy, tampered_incident)
    except WatchdogError:
        pass
    else:
        raise WatchdogError("tampered-incident negative control failed")
    stale_policy = copy.deepcopy(policy)
    stale_policy["historicalIncident"]["recordsSha256"] = "0" * 64
    try:
        verify_incident(stale_policy, incident)
    except WatchdogError:
        pass
    else:
        raise WatchdogError("stale-incident-authority negative control failed")
    tampered_chronology = copy.deepcopy(chronology)
    tampered_chronology["records"][0]["conclusion"] = "success"
    try:
        verify_release_full_chronology(policy, tampered_chronology)
    except WatchdogError:
        pass
    else:
        raise WatchdogError("tampered-release-full-chronology negative control failed")
    missing_chronology_row = copy.deepcopy(chronology)
    del missing_chronology_row["records"][1]
    try:
        verify_release_full_chronology(policy, missing_chronology_row)
    except WatchdogError:
        pass
    else:
        raise WatchdogError("missing-release-full-chronology-row negative control failed")
    stale_chronology_policy = copy.deepcopy(policy)
    stale_chronology_policy["releaseFullChronology"]["recordsSha256"] = "0" * 64
    try:
        verify_release_full_chronology(stale_chronology_policy, chronology)
    except WatchdogError:
        pass
    else:
        raise WatchdogError("stale-release-full-chronology-authority negative control failed")
    false_closing_success = copy.deepcopy(chronology)
    false_closing_success["records"][-1]["conclusion"] = "failure"
    try:
        verify_release_full_chronology(policy, false_closing_success)
    except WatchdogError:
        pass
    else:
        raise WatchdogError("false-release-full-closing-success negative control failed")
    tampered_standard_chronology = copy.deepcopy(standard_chronology)
    tampered_standard_chronology["records"][0]["displayTitle"] = "ci / push / fast"
    try:
        verify_standard_chronology(policy, tampered_standard_chronology)
    except WatchdogError:
        pass
    else:
        raise WatchdogError("tampered-standard-chronology negative control failed")
    missing_standard_chronology_row = copy.deepcopy(standard_chronology)
    del missing_standard_chronology_row["records"][1]
    try:
        verify_standard_chronology(policy, missing_standard_chronology_row)
    except WatchdogError:
        pass
    else:
        raise WatchdogError("missing-standard-chronology-row negative control failed")
    stale_standard_chronology_policy = copy.deepcopy(policy)
    stale_standard_chronology_policy["standardChronology"]["recordsSha256"] = "0" * 64
    try:
        verify_standard_chronology(stale_standard_chronology_policy, standard_chronology)
    except WatchdogError:
        pass
    else:
        raise WatchdogError("stale-standard-chronology-authority negative control failed")
    print("ci-liveness-watchdog: self-test ok (30 controls)")


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    evaluate_parser = subparsers.add_parser("evaluate")
    evaluate_parser.add_argument("--policy", type=Path, required=True)
    evaluate_parser.add_argument("--runs", type=Path, required=True)
    evaluate_parser.add_argument("--expected-head", required=True)
    evaluate_parser.add_argument("--expected-head-time", required=True)
    evaluate_parser.add_argument("--main-history", type=Path, required=True)
    evaluate_parser.add_argument("--now", required=True)
    evaluate_parser.add_argument("--out", type=Path, required=True)
    self_parser = subparsers.add_parser("self-test")
    self_parser.add_argument("--policy", type=Path, required=True)
    retained_parser = subparsers.add_parser("verify-retained")
    retained_parser.add_argument("--policy", type=Path, required=True)
    incident_parser = subparsers.add_parser("verify-incident")
    incident_parser.add_argument("--policy", type=Path, required=True)
    incident_parser.add_argument("--incident", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "self-test":
            self_test(args.policy)
            return 0
        if args.command == "verify-retained":
            verify_retained_evidence(args.policy)
            print("ci-liveness-watchdog: retained evidence verified")
            return 0
        if args.command == "verify-incident":
            verify_incident(read_json(args.policy), read_json(args.incident))
            print("ci-liveness-watchdog: incident verified")
            return 0
        history = [line.strip() for line in args.main_history.read_text(encoding="utf-8").splitlines() if line.strip()]
        report = evaluate(
            read_json(args.policy),
            read_json(args.runs),
            args.expected_head,
            args.expected_head_time,
            history,
            args.now,
        )
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        for row in report["violations"]:
            suffix = f" run={row['runId']}" if row["runId"] is not None else ""
            print(f"ci-liveness-watchdog: {row['code']}: {row['message']}{suffix}")
        print(f"ci-liveness-watchdog: status={report['status']} violations={len(report['violations'])}")
        return 0 if report["status"] == "pass" else 1
    except (OSError, WatchdogError) as exc:
        print(f"ci-liveness-watchdog: {exc}", flush=True)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
