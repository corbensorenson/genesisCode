#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
export GENESIS_DOCS_QUICKSTART_GIT_DIR="$(git rev-parse --absolute-git-dir)"

# boundary: dynamic-compilation-subject (executable documentation includes Cargo commands)

DOCS=("$@")
if [[ "${#DOCS[@]}" -eq 0 ]]; then
  DOCS=("README.md" "docs/GETTING_STARTED.md")
fi

for doc in "${DOCS[@]}"; do
  [[ -f "$doc" ]] || {
    echo "docs-quickstart: missing doc: $doc" >&2
    exit 1
  }
done

TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/genesis-docs-quickstart.XXXXXX")"
WORKTREE="$TMP_ROOT/repo"
cleanup() {
  rm -rf "$TMP_ROOT"
}
trap cleanup EXIT

RUN_SCRIPT="$TMP_ROOT/run_quickstart.sh"
PLAN_JSON="$TMP_ROOT/quickstart_plan.json"

if command -v rsync >/dev/null 2>&1; then
  mkdir -p "$WORKTREE"
  rsync -a \
    --delete \
    --exclude '.git' \
    --exclude '.genesis' \
    --exclude 'target' \
    --exclude 'node_modules' \
    --exclude '.tmp' \
    "$ROOT_DIR/" "$WORKTREE/"
else
  rm -rf "$WORKTREE"
  mkdir -p "$WORKTREE"
  tar \
    --exclude './.git' \
    --exclude './.genesis' \
    --exclude './target' \
    --exclude './node_modules' \
    --exclude './.tmp' \
    -cf - . | tar -C "$WORKTREE" -xf -
fi

python3 - "$ROOT_DIR" "$RUN_SCRIPT" "$PLAN_JSON" "${DOCS[@]}" <<'PY'
import json
import pathlib
import re
import shlex
import sys

root = pathlib.Path(sys.argv[1])
run_script = pathlib.Path(sys.argv[2])
plan_json = pathlib.Path(sys.argv[3])
docs = [pathlib.Path(p) for p in sys.argv[4:]]

heading_re = re.compile(r"^##\s+(\d+)\)")
fence_start_re = re.compile(r"^```(?:sh|bash)\s*$")

blocks = []
errors = []

for doc in docs:
    text = (root / doc).read_text(encoding="utf-8")
    expected = 1
    for line_no, line in enumerate(text.splitlines(), start=1):
        m = heading_re.match(line)
        if not m:
            continue
        got = int(m.group(1))
        if got != expected:
            errors.append(
                f"{doc}:{line_no}: numbered heading expected {expected}) but found {got})"
            )
            expected = got + 1
        else:
            expected += 1

    in_fence = False
    start_line = 0
    cur = []
    for line_no, line in enumerate(text.splitlines(), start=1):
        if not in_fence and fence_start_re.match(line):
            in_fence = True
            start_line = line_no + 1
            cur = []
            continue
        if in_fence and line.startswith("```"):
            body = "\n".join(cur).strip()
            if body:
                skip = None
                for body_line in cur:
                    marker = body_line.strip()
                    if marker.startswith("# genesis-doc-skip:"):
                        skip = marker.split(":", 1)[1].strip()
                        break
                if skip is None and ("path/to/" in body or "<" in body and ">" in body):
                    errors.append(
                        f"{doc}:{start_line}: executable shell block contains placeholder material; "
                        "add a genesis-doc-skip marker or use a real fixture"
                    )
                blocks.append(
                    {
                        "doc": doc.as_posix(),
                        "line": start_line,
                        "body": body,
                        "skip": skip,
                    }
                )
            in_fence = False
            continue
        if in_fence:
            cur.append(line)

    if in_fence:
        errors.append(f"{doc}:{start_line}: unterminated shell fence")

if errors:
    raise SystemExit("docs-quickstart: " + "\n".join(errors))

script_lines = [
    "#!/usr/bin/env bash",
    "set -euo pipefail",
    'source scripts/lib/cargo_target_dir.sh',
    f"GENESIS_DOCS_QUICKSTART_ROOT={shlex.quote(str(root))}",
    'export GIT_DIR="$GENESIS_DOCS_QUICKSTART_GIT_DIR"',
    'export GIT_WORK_TREE="$PWD"',
    '# Discard only authenticated inherited resolver state before rotating the build key.',
    'genesis_clear_resolved_cargo_target_dir "docs-quickstart-inherited-cache"',
    '# Executable docs need behavioral coverage, not multi-gigabyte debug symbols.',
    'export CARGO_INCREMENTAL=0',
    'export CARGO_PROFILE_DEV_DEBUG=0',
    'genesis_configure_cargo_target_dir "$GENESIS_DOCS_QUICKSTART_ROOT" docs-quickstart root-host',
    'trap \'genesis_clear_resolved_cargo_target_dir "docs-quickstart-exit"\' EXIT',
    f"GENESIS_DOCS_QUICKSTART_CARGO_MANIFEST={shlex.quote(str(root / 'Cargo.toml'))}",
    'cargo() {',
    '  local cargo_command="${1:?cargo subcommand required}"',
    '  shift',
    '  local -a cargo_args=("$@")',
    '  local manifest_seen=0',
    '  local i',
    '  for ((i = 0; i < ${#cargo_args[@]}; i++)); do',
    '    if [[ "${cargo_args[$i]}" == "--manifest-path" ]]; then',
    '      ((i + 1 < ${#cargo_args[@]})) || { echo "docs-quickstart: --manifest-path requires a value" >&2; return 2; }',
    '      cargo_args[$((i + 1))]="$GENESIS_DOCS_QUICKSTART_CARGO_MANIFEST"',
    '      manifest_seen=1',
    '    fi',
    '  done',
    '  if [[ "$manifest_seen" == "0" ]]; then',
    '    cargo_args=(--manifest-path "$GENESIS_DOCS_QUICKSTART_CARGO_MANIFEST" "${cargo_args[@]}")',
    '  fi',
    '  command cargo "$cargo_command" "${cargo_args[@]}"',
    '}',
    'export GENESIS_DOCS_QUICKSTART=1',
    "mkdir -p .tmp",
]

for idx, block in enumerate(blocks, start=1):
    loc = f"{block['doc']}:{block['line']}"
    if block["skip"]:
        skip_msg = f"docs-quickstart: skip {idx} {loc}: {block['skip']}"
        script_lines.append(f"echo {shlex.quote(skip_msg)}")
        continue
    script_lines.append(f"echo {shlex.quote(f'docs-quickstart: run {idx} {loc}')}")
    script_lines.append(block["body"])

run_script.write_text("\n\n".join(script_lines) + "\n", encoding="utf-8")
run_script.chmod(0o755)
plan_json.write_text(
    json.dumps(
        {
            "kind": "genesis/docs-quickstart-plan-v0.1",
            "docs": [d.as_posix() for d in docs],
            "blocks": [
                {
                    "doc": b["doc"],
                    "line": b["line"],
                    "skip": b["skip"],
                }
                for b in blocks
            ],
        },
        indent=2,
        sort_keys=True,
    )
    + "\n",
    encoding="utf-8",
)
PY

export GENESIS_CARGO_CACHE_ROOT="${GENESIS_CARGO_CACHE_ROOT:-$ROOT_DIR/.genesis/build/cargo-cache/v1}"

(
  cd "$WORKTREE"
  bash "$RUN_SCRIPT"
)

python3 - "$PLAN_JSON" <<'PY'
import json
import pathlib
import sys

plan = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
total = len(plan["blocks"])
skipped = sum(1 for b in plan["blocks"] if b["skip"])
print(f"docs-quickstart: ok (docs={len(plan['docs'])} shell_blocks={total} skipped={skipped})")
PY
