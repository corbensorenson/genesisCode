#!/usr/bin/env python3
"""Bounded actual-WASI development controls with independent host snapshots.

Consumes an already built exact wasm32-wasip1 test executable. This is local
E0 behavior evidence, not an independent verifier or supported-host qualification.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time


def snapshot(root: Path) -> dict:
    entries = {}
    for directory, children, files in os.walk(root, followlinks=False):
        for path in [Path(directory), *(Path(directory) / name for name in children + files)]:
            key = str(path.relative_to(root))
            metadata = path.lstat()
            if path.is_symlink():
                value = ["link", os.readlink(path)]
            elif path.is_dir():
                value = ["directory"]
            elif path.is_file():
                value = ["file", hashlib.sha256(path.read_bytes()).hexdigest()]
            else:
                raise RuntimeError(f"unexpected fixture entry {path}")
            entries[key] = [metadata.st_dev, metadata.st_ino, value]
    return entries


def prepare(root: Path) -> list[Path]:
    (root / "runtime").mkdir()
    outside = root / "outside"
    outside.mkdir()
    (outside / "retained").write_bytes(b"retained")
    preserved = [outside]
    for tier in range(2):
        for case in ("ordinary", "denied", "entries", "documents", "replay"):
            fixture = root / f"{case}-{tier}"
            fixture.mkdir()
            if case == "ordinary":
                (fixture / "replacement").write_bytes(b"old destination")
            else:
                (fixture / "source").write_bytes(b"source")
            if case in ("denied", "entries", "documents"):
                (fixture / "directory").mkdir()
                (fixture / "directory/retained").write_bytes(b"retained")
            if case in ("denied", "documents"):
                (fixture / "escape").symlink_to("../outside", target_is_directory=True)
            if case == "denied":
                (fixture / "inside-link").symlink_to("source")
                nested = fixture / "deep"
                for _ in range(257):
                    nested.mkdir()
                    nested /= "x"
                preserved.append(fixture)
            if case == "entries":
                (fixture / "target").write_bytes(b"retained")
                for name, target in {
                    "file-link": "target", "directory-link": "directory",
                    "dangling-link": "absent", "outside-link": "../outside/retained",
                    "destination-link": "target",
                }.items():
                    (fixture / name).symlink_to(target)
            if case == "documents":
                (fixture / "genesis.lock").symlink_to("../outside/retained")
    return preserved


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--wasmtime", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    args = parser.parse_args()
    repository = Path(__file__).resolve().parents[2]
    binary, runtime = args.binary.resolve(strict=True), args.wasmtime.resolve(strict=True)
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
    version = subprocess.check_output([str(runtime), "--version"], text=True).strip()
    material_names = ("crates/gc_effects/src/rooted_fs_wasi.rs",
                      "crates/gc_effects/tests/wasi_rooted_security.rs",
                      "crates/gc_effects/Cargo.toml", "crates/gc_effects/src/lib.rs",
                      "scripts/lib/wasi_rooted_controls.py")
    source_hashes = {name: hashlib.sha256((repository / name).read_bytes()).hexdigest()
                     for name in material_names}
    with tempfile.TemporaryDirectory(prefix="genesis-wasi-rooted-") as temporary:
        fixture = Path(temporary)
        preserved = prepare(fixture)
        before = {str(path.relative_to(fixture)): snapshot(path) for path in preserved}
        command = [str(runtime), "run", "--dir", f"{fixture}::/controls",
                   "--dir", f"{repository}::/workspace",
                   "--env", "GENESIS_WASI_FS_FIXTURES=/controls",
                   "--env", "GENESIS_TEST_SELFHOST_ARTIFACT=/workspace/selfhost/toolchain.gc",
                   str(binary), "--test-threads=1", "--nocapture"]
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        started = time.monotonic()
        timed_out = False
        try:
            output, _ = process.communicate(timeout=240)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            output, _ = process.communicate()
            timed_out = True
        args.report.with_suffix(".log").write_bytes(output)
        after = {str(path.relative_to(fixture)): snapshot(path) for path in preserved}
        result = {"authority": "E0 actual-WASI development observation; no qualification",
                  "revision": revision, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                  "runtime": version, "runtime_sha256": hashlib.sha256(runtime.read_bytes()).hexdigest(),
                  "budget_seconds": 240, "elapsed_seconds": time.monotonic() - started,
                  "timed_out": timed_out, "returncode": process.returncode,
                  "host_preservation": before == after, "preserved_before": before,
                  "preserved_after": after,
                  "source_sha256": source_hashes,
                  "source_unchanged": all(hashlib.sha256((repository / name).read_bytes()).hexdigest()
                                          == digest for name, digest in source_hashes.items())}
        args.report.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
        print(output.decode("utf-8", errors="replace"), end="")
        if timed_out or not result["source_unchanged"] or process.returncode != 0 or before != after:
            raise RuntimeError("actual WASI control or host identity/content preservation failed")
        if b"5 passed; 0 failed" not in output:
            raise RuntimeError("actual WASI control inventory did not execute all five tests")
        print("Host-side snapshots: outside and denied fixture identities/content preserved.")


if __name__ == "__main__":
    main()
