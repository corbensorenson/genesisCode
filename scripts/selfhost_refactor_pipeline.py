#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, List


MANIFEST_REL = Path("selfhost/toolchain_manifest.gc")


class RefactorError(RuntimeError):
    pass


@dataclass(frozen=True)
class SplitResult:
    source_module: str
    new_module: str
    source_forms_before: int
    source_forms_after: int
    new_forms: int
    semantic_hash: str
    artifact_path: str


def run(cmd: list[str], *, cwd: Path, capture_stdout: bool = False) -> str:
    proc = subprocess.run(
        cmd,
        cwd=str(cwd),
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if proc.returncode != 0:
        rendered = " ".join(cmd)
        details = (proc.stderr or "").strip()
        raise RefactorError(f"command failed ({proc.returncode}): {rendered}\n{details}")
    return (proc.stdout or "").strip() if capture_stdout else ""


def ensure_genesis_bin(repo_root: Path, explicit: str | None) -> Path:
    if explicit:
        p = Path(explicit).expanduser().resolve()
        if not p.is_file():
            raise RefactorError(f"--genesis-bin not found: {p}")
        if not os.access(p, os.X_OK):
            raise RefactorError(f"--genesis-bin is not executable: {p}")
        return p

    p = repo_root / "target" / "debug" / "genesis"
    if p.is_file() and os.access(p, os.X_OK):
        return p

    run(["cargo", "build", "-p", "gc_cli", "--bin", "genesis"], cwd=repo_root)
    if not p.is_file() or not os.access(p, os.X_OK):
        raise RefactorError(f"failed to build genesis binary at {p}")
    return p


def resolve_hash_bin(repo_root: Path, genesis_bin: Path) -> Path:
    parity_bin = genesis_bin.with_name("genesis_parity")
    if parity_bin.is_file() and os.access(parity_bin, os.X_OK):
        return parity_bin
    run(["cargo", "build", "-p", "gc_cli", "--bin", "genesis_parity"], cwd=repo_root)
    if parity_bin.is_file() and os.access(parity_bin, os.X_OK):
        return parity_bin
    return genesis_bin


def parse_manifest_module_paths(manifest_path: Path) -> list[str]:
    text = manifest_path.read_text(encoding="utf-8")
    m = re.search(r":module-paths\s*\[(?P<body>.*?)\]", text, flags=re.S)
    if not m:
        raise RefactorError(f"manifest missing :module-paths vector: {manifest_path}")
    body = m.group("body")
    paths = re.findall(r'"(selfhost/[A-Za-z0-9_./-]+\.gc)"', body)
    if not paths:
        raise RefactorError(f"manifest has empty :module-paths vector: {manifest_path}")
    if len(paths) != len(set(paths)):
        raise RefactorError(f"manifest has duplicate module paths: {manifest_path}")
    return paths


def insert_manifest_module_path(manifest_path: Path, after_path: str, new_path: str) -> None:
    lines = manifest_path.read_text(encoding="utf-8").splitlines(keepends=True)
    if any(f'"{new_path}"' in line for line in lines):
        raise RefactorError(f"manifest already contains module path: {new_path}")

    in_block = False
    insert_idx = None
    indent = None
    for idx, line in enumerate(lines):
        if not in_block and ":module-paths" in line and "[" in line:
            in_block = True
            continue
        if not in_block:
            continue
        if "]" in line:
            break
        if f'"{after_path}"' in line:
            insert_idx = idx + 1
            indent_match = re.match(r"^(\s*)", line)
            indent = indent_match.group(1) if indent_match else "    "
            break

    if insert_idx is None or indent is None:
        raise RefactorError(f"manifest missing source module path: {after_path}")

    lines.insert(insert_idx, f'{indent}"{new_path}"\n')
    manifest_path.write_text("".join(lines), encoding="utf-8")


def parse_balanced_form(src: str, start: int) -> int:
    pairs = {"(": ")", "[": "]", "{": "}"}
    openers = set(pairs.keys())
    closers = {v: k for k, v in pairs.items()}

    stack = [src[start]]
    i = start + 1
    in_string = False
    escaped = False
    in_comment = False

    while i < len(src):
        ch = src[i]
        if in_comment:
            if ch == "\n":
                in_comment = False
            i += 1
            continue
        if in_string:
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == '"':
                in_string = False
            i += 1
            continue
        if ch == ";":
            in_comment = True
            i += 1
            continue
        if ch == '"':
            in_string = True
            i += 1
            continue
        if ch in openers:
            stack.append(ch)
            i += 1
            continue
        if ch in closers:
            if not stack:
                raise RefactorError("unbalanced closing delimiter in module source")
            expected_open = closers[ch]
            got_open = stack.pop()
            if got_open != expected_open:
                raise RefactorError(
                    f"mismatched delimiter in module source: expected {pairs[got_open]}, got {ch}"
                )
            i += 1
            if not stack:
                return i
            continue
        i += 1
    raise RefactorError("unterminated form in module source")


def parse_atom_form(src: str, start: int) -> int:
    i = start
    while i < len(src):
        ch = src[i]
        if ch.isspace() or ch in "()[]{};" or ch == '"':
            break
        i += 1
    if i == start:
        raise RefactorError("failed to parse atom form")
    return i


def split_top_level_forms(src: str) -> list[str]:
    forms: list[str] = []
    i = 0
    n = len(src)
    while i < n:
        while i < n and src[i].isspace():
            i += 1
        if i >= n:
            break
        if src[i] == ";":
            while i < n and src[i] != "\n":
                i += 1
            continue
        start = i
        ch = src[i]
        if ch in "([{":
            end = parse_balanced_form(src, i)
        elif ch == '"':
            end = parse_balanced_form(src, i)
        else:
            end = parse_atom_form(src, i)
        form = src[start:end].strip()
        if form:
            forms.append(form)
        i = end
    if not forms:
        raise RefactorError("module source has no top-level forms")
    return forms


def module_text_from_forms(forms: Iterable[str]) -> str:
    out = "\n\n".join(f.strip() for f in forms if f.strip())
    if not out:
        return "\n"
    return out + "\n"


def assembled_module_text(repo_root: Path, module_paths: list[str]) -> str:
    chunks: list[str] = []
    for rel in module_paths:
        p = repo_root / rel
        if not p.is_file():
            raise RefactorError(f"manifest module missing: {rel}")
        chunks.append(p.read_text(encoding="utf-8"))
    out = "\n".join(chunks)
    if not out.endswith("\n"):
        out += "\n"
    return out


def vcs_hash_for_text(repo_root: Path, hash_bin: Path, text: str) -> str:
    with tempfile.TemporaryDirectory(prefix="selfhost-refactor-hash-") as td:
        module_path = Path(td) / "assembled.gc"
        module_path.write_text(text, encoding="utf-8")
        if hash_bin.name.endswith("genesis_parity"):
            cmd = [
                str(hash_bin),
                "vcs",
                "hash",
                "--engine",
                "rust",
                "--in",
                str(module_path),
            ]
        else:
            cmd = [
                str(hash_bin),
                "--step-limit",
                "2000000000",
                "vcs",
                "hash",
                "--in",
                str(module_path),
            ]
        out = run(cmd, cwd=repo_root, capture_stdout=True)
        h = out.strip().splitlines()
        if not h:
            raise RefactorError("vcs hash returned empty output")
        return h[-1].strip()


def build_selfhost_artifact(repo_root: Path, genesis_bin: Path, out_path: Path) -> None:
    run(
        [str(genesis_bin), "selfhost-artifact", "--out", str(out_path)],
        cwd=repo_root,
    )


def run_replay_smoke(repo_root: Path, genesis_bin: Path, artifact_path: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="selfhost-refactor-replay-") as td:
        td_path = Path(td)
        run_gc = td_path / "run.gc"
        caps = td_path / "caps.toml"
        log = td_path / "run.gclog"
        run_gc.write_text("(def prog (core/effect::pure 42))\nprog\n", encoding="utf-8")
        caps.write_text("allow = []\n", encoding="utf-8")
        run(
            [
                str(genesis_bin),
                "--selfhost-only",
                "--selfhost-artifact",
                str(artifact_path),
                "run",
                str(run_gc),
                "--engine",
                "selfhost",
                "--caps",
                str(caps),
                "--log",
                str(log),
            ],
            cwd=repo_root,
        )
        run(
            [
                str(genesis_bin),
                "--selfhost-only",
                "--selfhost-artifact",
                str(artifact_path),
                "replay",
                str(run_gc),
                "--engine",
                "selfhost",
                "--log",
                str(log),
            ],
            cwd=repo_root,
        )


def verify_pipeline(repo_root: Path, genesis_bin: Path, hash_bin: Path) -> tuple[str, str]:
    manifest_path = repo_root / MANIFEST_REL
    module_paths = parse_manifest_module_paths(manifest_path)
    assembled = assembled_module_text(repo_root, module_paths)
    semantic_hash = vcs_hash_for_text(repo_root, hash_bin, assembled)

    with tempfile.TemporaryDirectory(prefix="selfhost-refactor-verify-") as td:
        artifact_path = Path(td) / "toolchain.gc"
        build_selfhost_artifact(repo_root, genesis_bin, artifact_path)
        run_replay_smoke(repo_root, genesis_bin, artifact_path)
        artifact_hash = hashlib.sha256(artifact_path.read_bytes()).hexdigest()
    return semantic_hash, artifact_hash


def do_split_tail(
    repo_root: Path,
    genesis_bin: Path,
    hash_bin: Path,
    source_module: str,
    new_module: str,
    split_form_index: int,
) -> SplitResult:
    manifest_path = repo_root / MANIFEST_REL
    source_path = repo_root / source_module
    new_path = repo_root / new_module

    if not source_path.is_file():
        raise RefactorError(f"--module does not exist: {source_module}")
    if new_path.exists():
        raise RefactorError(f"--new-module already exists: {new_module}")
    if not source_module.startswith("selfhost/") or not new_module.startswith("selfhost/"):
        raise RefactorError("module paths must be under selfhost/")

    module_paths_before = parse_manifest_module_paths(manifest_path)
    if source_module not in module_paths_before:
        raise RefactorError(f"manifest does not contain source module: {source_module}")
    if new_module in module_paths_before:
        raise RefactorError(f"manifest already contains new module path: {new_module}")

    assembled_before = assembled_module_text(repo_root, module_paths_before)
    hash_before = vcs_hash_for_text(repo_root, hash_bin, assembled_before)

    source_src_before = source_path.read_text(encoding="utf-8")
    manifest_before = manifest_path.read_text(encoding="utf-8")
    forms = split_top_level_forms(source_src_before)
    if split_form_index <= 0 or split_form_index >= len(forms):
        raise RefactorError(
            f"--split-form-index must be in [1, {len(forms) - 1}] for module {source_module}"
        )

    keep_forms = forms[:split_form_index]
    moved_forms = forms[split_form_index:]

    source_path.write_text(module_text_from_forms(keep_forms), encoding="utf-8")
    new_path.parent.mkdir(parents=True, exist_ok=True)
    new_path.write_text(module_text_from_forms(moved_forms), encoding="utf-8")
    insert_manifest_module_path(manifest_path, source_module, new_module)

    try:
        module_paths_after = parse_manifest_module_paths(manifest_path)
        assembled_after = assembled_module_text(repo_root, module_paths_after)
        hash_after = vcs_hash_for_text(repo_root, hash_bin, assembled_after)
        if hash_after != hash_before:
            raise RefactorError(
                "semantic equivalence check failed: assembled hash changed "
                f"(before={hash_before}, after={hash_after})"
            )

        with tempfile.TemporaryDirectory(prefix="selfhost-refactor-split-") as td:
            artifact_path = Path(td) / "toolchain.gc"
            build_selfhost_artifact(repo_root, genesis_bin, artifact_path)
            run_replay_smoke(repo_root, genesis_bin, artifact_path)
            result = SplitResult(
                source_module=source_module,
                new_module=new_module,
                source_forms_before=len(forms),
                source_forms_after=len(keep_forms),
                new_forms=len(moved_forms),
                semantic_hash=hash_after,
                artifact_path=str(artifact_path),
            )
        return result
    except Exception:
        source_path.write_text(source_src_before, encoding="utf-8")
        manifest_path.write_text(manifest_before, encoding="utf-8")
        if new_path.exists():
            new_path.unlink()
        raise


def cmd_verify(args: argparse.Namespace) -> int:
    repo_root = Path(args.repo_root).resolve()
    genesis_bin = ensure_genesis_bin(repo_root, args.genesis_bin)
    hash_bin = resolve_hash_bin(repo_root, genesis_bin)
    semantic_hash, artifact_hash = verify_pipeline(repo_root, genesis_bin, hash_bin)
    print("selfhost-refactor: verify ok")
    print(f"  semantic_hash={semantic_hash}")
    print(f"  artifact_hash={artifact_hash}")
    return 0


def cmd_split_tail(args: argparse.Namespace) -> int:
    repo_root = Path(args.repo_root).resolve()
    genesis_bin = ensure_genesis_bin(repo_root, args.genesis_bin)
    hash_bin = resolve_hash_bin(repo_root, genesis_bin)
    result = do_split_tail(
        repo_root=repo_root,
        genesis_bin=genesis_bin,
        hash_bin=hash_bin,
        source_module=args.module,
        new_module=args.new_module,
        split_form_index=args.split_form_index,
    )
    print("selfhost-refactor: split-tail ok")
    print(f"  source_module={result.source_module}")
    print(f"  new_module={result.new_module}")
    print(f"  source_forms_before={result.source_forms_before}")
    print(f"  source_forms_after={result.source_forms_after}")
    print(f"  new_forms={result.new_forms}")
    print(f"  semantic_hash={result.semantic_hash}")
    print(f"  replay_verified_artifact={result.artifact_path}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    default_root = Path(__file__).resolve().parents[1]
    p = argparse.ArgumentParser(
        prog="selfhost_refactor_pipeline.py",
        description=(
            "Deterministic selfhost refactor pipeline: split modular sources while enforcing "
            "semantic-equivalence and replay safety."
        ),
    )
    p.add_argument(
        "--repo-root",
        default=str(default_root),
        help="GenesisCode repository root (defaults to script parent repo).",
    )
    p.add_argument(
        "--genesis-bin",
        default=None,
        help="Optional path to genesis CLI binary (defaults to target/debug/genesis).",
    )

    sub = p.add_subparsers(dest="subcmd", required=True)

    verify = sub.add_parser(
        "verify",
        help="Verify manifest-module semantic hash + selfhost run/replay invariants.",
    )
    verify.set_defaults(func=cmd_verify)

    split = sub.add_parser(
        "split-tail",
        help=(
            "Split a selfhost source module at top-level form index N; "
            "tail forms move to --new-module, manifest is updated after --module."
        ),
    )
    split.add_argument("--module", required=True, help="Existing selfhost module path.")
    split.add_argument(
        "--new-module",
        required=True,
        help="New selfhost module path to create for the moved tail forms.",
    )
    split.add_argument(
        "--split-form-index",
        type=int,
        required=True,
        help="Top-level form index where the source module is split.",
    )
    split.set_defaults(func=cmd_split_tail)
    return p


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return int(args.func(args))
    except RefactorError as e:
        print(f"selfhost-refactor: error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
