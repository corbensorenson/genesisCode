#!/usr/bin/env python3
"""Finite public admission controls for idle allocation and live reservations."""
from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

import deterministic_cleanup as cleanup
import generated_state as state


def _require(condition: bool, diagnostic: str) -> None:
    if not condition:
        raise AssertionError(f"generated-state-accounting: {diagnostic}")


def accounting_self_test(source_root: Path) -> int:
    checks = []
    with tempfile.TemporaryDirectory(prefix="generated-state-accounting-") as temporary:
        root = Path(temporary)
        (root / "policies").mkdir()
        (root / cleanup.POLICY_REL).write_bytes((source_root / cleanup.POLICY_REL).read_bytes())
        policy, _, _ = state.load_policy(source_root)
        policy = copy.deepcopy(policy)
        policy["limits"].update(
            softBytes=229376, hardBytes=262144, minFreeBytes=0, maxEntries=32, maxLeases=16,
        )
        reservations = {"cargo-host": 131072, "cargo-wasm": 196608}
        for item in policy["sizeClasses"]:
            item["reservationBytes"] = reservations.get(item["id"], 0)
        (root / state.POLICY_REL).write_bytes(state.pretty_bytes(policy))
        (root / ".gitignore").write_text(".genesis/\n", encoding="utf-8")
        subprocess.run(["git", "init", "-q", str(root)], check=True)
        subprocess.run(["git", "add", ".gitignore"], cwd=root, check=True)
        (root / ".genesis/build").mkdir(parents=True)
        cleanup.initialize_root_marker(root, ".genesis/build", "accounting-controls")
        host = ".genesis/build/cargo-cache/v1/root/host/cache"
        wasm = ".genesis/build/cargo-cache/v1/root/wasm/cache"

        def admit(path: str, size_class: str, key: str) -> dict:
            return state.admit(
                root, "cargo-cache", key * 64, path, size_class,
                pid=os.getpid(), free_bytes_override=1 << 30,
            )

        native = admit(host, "cargo-host", "a")
        target = root / host
        target.mkdir(parents=True)
        payload = target / "payload"
        payload.write_bytes(b"a" * 4096)

        def identity() -> tuple:
            metadata = payload.stat()
            return metadata.st_dev, metadata.st_ino, payload.read_bytes()

        original = identity()
        state.release(root, native["leaseToken"])
        idle = state.allocated_bytes(target)
        report = json.loads(subprocess.check_output([
            sys.executable, str(source_root / "scripts/lib/generated_state.py"),
            "--root", str(root), "status", "--format", "json",
        ], text=True))
        _require(report["accountingBytes"] == idle and report["activeLeases"] == 0,
                 "public idle status charged unused growth")
        checks.append("public-idle-status-charges-observed-allocation")

        first = admit(wasm, "cargo-wasm", "b")
        _require(not first["reclaimedEntryIds"] and identity() == original,
                 "fitting idle cache was reclaimed or changed")
        _require(first["accountingBytes"] == idle + 196608, "idle/live total is incorrect")
        checks.append("fitting-idle-cache-preserves-inode-and-content")

        shared = admit(wasm, "cargo-wasm", "b")
        _require(shared["accountingBytes"] == first["accountingBytes"]
                 and shared["pendingGrowthBytes"] == 196608, "shared reservation was duplicated")
        state.release(root, first["leaseToken"])
        _require(state.status(root)["accountingBytes"] == idle + 196608,
                 "one lease release discarded a live reservation")
        checks.append("shared-live-reservation-survives-one-lease-release")

        try:
            admit(".genesis/build/cargo-cache/v1/root/host/other", "cargo-host", "c")
        except state.GeneratedStateError as exc:
            _require("hard quota admission denied" in str(exc), "wrong hard-quota diagnostic")
        else:
            raise AssertionError("distinct active/requested reservations exceeded hard quota")
        _require(state.validate_lease(root, shared["leaseToken"], wasm)["valid"],
                 "denial invalidated the live lease")
        _require(identity() == original, "impossible request discarded an idle cache")
        checks.append("distinct-live-writers-still-denied-above-hard-quota")

        # An oversized requested cache has an earlier eviction path. Prove
        # that even that path cannot delete data for an impossible replacement.
        payload.write_bytes(b"o" * 300000)
        oversized = identity()
        try:
            admit(host, "cargo-host", "a")
        except state.GeneratedStateError as exc:
            _require("hard quota admission denied" in str(exc), "wrong replacement diagnostic")
        else:
            raise AssertionError("impossible oversized-cache replacement admitted")
        _require(identity() == oversized, "impossible replacement deleted the requested cache")
        _require(state.validate_lease(root, shared["leaseToken"], wasm)["valid"],
                 "replacement denial invalidated the live lease")
        payload.write_bytes(original[2])
        checks.append("oversized-request-preserved-before-impossible-replacement")

        state.release(root, shared["leaseToken"])
        _require(state.status(root)["accountingBytes"] == idle,
                 "last release retained unused growth or discarded stored allocation")
        checks.append("last-release-removes-only-the-growth-reservation")

        # External idle growth must be observed before admission: the old
        # release snapshot cannot authorize retaining an oversized cache.
        payload.write_bytes(b"x" * 300000)
        final = admit(wasm, "cargo-wasm", "b")
        _require(native["entryId"] in final["reclaimedEntryIds"] and not target.exists(),
                 "stale idle observation bypassed actual excess reclamation")
        _require(final["accountingBytes"] == 196608, "requested growth was not charged")
        state.release(root, final["leaseToken"])
        checks.append("idle-growth-remeasured-before-quota-admission")

    _require(len(checks) == 7, "control inventory drift")
    print("generated-state-accounting: " + json.dumps({"controls": checks}, sort_keys=True))
    return len(checks)


if __name__ == "__main__":
    accounting_self_test(Path(__file__).resolve().parents[2])
