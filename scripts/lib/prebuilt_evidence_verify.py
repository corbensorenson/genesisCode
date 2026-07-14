#!/usr/bin/env python3
"""Run an already-built evidence verifier with hard network denial and a deadline."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import platform
import shutil
import stat
import subprocess
import sys
from typing import List, Optional, Sequence


class VerifyError(ValueError):
    pass


def regular(path: Path, label: str, executable: bool = False) -> Path:
    if path.is_symlink() or not path.is_file():
        raise VerifyError(f"{label} must be a regular non-symlink file: {path}")
    mode = path.stat().st_mode
    if executable and not mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH):
        raise VerifyError(f"{label} is not executable: {path}")
    return path.resolve()


def isolated_argv(command: List[str]) -> List[str]:
    system = platform.system().lower()
    if system == "darwin":
        sandbox = shutil.which("sandbox-exec")
        if sandbox is None:
            raise VerifyError("Darwin network-denial backend is unavailable")
        return [sandbox, "-p", "(version 1)(allow default)(deny network*)", *command]
    if system == "linux":
        unshare = shutil.which("unshare")
        if unshare is None:
            raise VerifyError("Linux network namespace backend is unavailable")
        probe = subprocess.run(
            [unshare, "--user", "--map-root-user", "--net", "--", "true"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if probe.returncode != 0:
            raise VerifyError("Linux network namespace creation is not permitted")
        return [unshare, "--user", "--map-root-user", "--net", "--", *command]
    raise VerifyError(f"prebuilt evidence verification is unsupported on {system or 'unknown'}")


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--verifier", type=Path, required=True)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--policy-sha256", required=True)
    parser.add_argument("--artifact-tree", type=Path, required=True)
    parser.add_argument("--artifact-root", type=Path, required=True)
    parser.add_argument("--timeout-seconds", type=int, default=300)
    args = parser.parse_args(argv)
    try:
        if args.timeout_seconds < 1 or args.timeout_seconds > 300:
            raise VerifyError("timeout must be within 1..300 seconds")
        if len(args.policy_sha256) != 64 or any(ch not in "0123456789abcdef" for ch in args.policy_sha256):
            raise VerifyError("policy sha256 must be 64 lowercase hexadecimal characters")
        verifier = regular(args.verifier, "verifier", executable=True)
        bundle = regular(args.bundle, "bundle")
        policy = regular(args.policy, "policy")
        artifact_tree = regular(args.artifact_tree, "artifact tree")
        artifact_root = args.artifact_root.resolve()
        if args.artifact_root.is_symlink() or not artifact_root.is_dir():
            raise VerifyError("artifact root must be a non-symlink directory")
        command = [
            str(verifier),
            "--bundle", str(bundle),
            "--policy", str(policy),
            "--policy-sha256", args.policy_sha256,
            "--artifact-tree", str(artifact_tree),
            "--artifact-root", str(artifact_root),
        ]
        env = dict(os.environ)
        env["CARGO_NET_OFFLINE"] = "true"
        proc = subprocess.run(
            isolated_argv(command),
            cwd=args.root.resolve(),
            env=env,
            timeout=args.timeout_seconds,
        )
        return proc.returncode
    except subprocess.TimeoutExpired:
        print("prebuilt-evidence-verify: verifier exceeded 300-second budget", file=sys.stderr)
        return 124
    except (OSError, VerifyError) as exc:
        print(f"prebuilt-evidence-verify: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
