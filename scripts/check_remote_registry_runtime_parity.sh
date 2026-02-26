#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "check-remote-registry-runtime-parity" \
  ".genesis/build/cargo" \
  "GENESIS_CHECK_REMOTE_REGISTRY_RUNTIME_PARITY_CARGO_TARGET_DIR"

REPORT_OUT="${GENESIS_REMOTE_REGISTRY_RUNTIME_PARITY_REPORT:-$ROOT_DIR/.genesis/perf/remote_registry_runtime_parity_report.json}"
SKIP_CHECK="${GENESIS_REMOTE_REGISTRY_RUNTIME_PARITY_SKIP:-0}"

if [[ "$SKIP_CHECK" == "1" ]]; then
  mkdir -p "$(dirname "$REPORT_OUT")"
  python3 - "$REPORT_OUT" <<'PY'
import json
import pathlib
import sys

report = {
    "kind": "genesis/remote-registry-runtime-parity-report-v0.1",
    "ok": True,
    "skipped": True,
    "reason": "GENESIS_REMOTE_REGISTRY_RUNTIME_PARITY_SKIP=1",
}
path = pathlib.Path(sys.argv[1])
path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"remote-registry-runtime-parity: skipped report={path}")
PY
  exit 0
fi

cargo build -p gc_cli --bin genesis >/dev/null
cargo build -p gc_wasi_cli --bin genesis_wasi >/dev/null

GENESIS_NATIVE_BIN="${GENESIS_REMOTE_REGISTRY_RUNTIME_PARITY_NATIVE_BIN:-$CARGO_TARGET_DIR/debug/genesis}"
GENESIS_WASI_BIN="${GENESIS_REMOTE_REGISTRY_RUNTIME_PARITY_WASI_BIN:-$CARGO_TARGET_DIR/debug/genesis_wasi}"

for bin in "$GENESIS_NATIVE_BIN" "$GENESIS_WASI_BIN"; do
  if [[ ! -x "$bin" ]]; then
    echo "remote-registry-runtime-parity: missing binary: $bin" >&2
    exit 1
  fi
done

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

ARTIFACT="$TMP_DIR/selfhost_toolchain.gc"
if [[ -f "$ROOT_DIR/selfhost/toolchain.gc" ]]; then
  cp "$ROOT_DIR/selfhost/toolchain.gc" "$ARTIFACT"
else
  "$GENESIS_NATIVE_BIN" selfhost-artifact --out "$ARTIFACT" >/dev/null
fi

RESULTS_TSV="$TMP_DIR/lane_results.tsv"
: >"$RESULTS_TSV"

run_lane() {
  local lane="$1"
  local genesis_bin="$2"
  local lane_root="$TMP_DIR/$lane"
  local publisher_dir="$lane_root/publisher"
  local consumer_dir="$lane_root/consumer"
  local remote_dir="$lane_root/remote-registry"

  mkdir -p "$publisher_dir" "$consumer_dir" "$remote_dir"
  local publisher_real
  local consumer_real
  local remote_real
  publisher_real="$(cd "$publisher_dir" && pwd -P)"
  consumer_real="$(cd "$consumer_dir" && pwd -P)"
  remote_real="$(cd "$remote_dir" && pwd -P)"

  local remote="file://$remote_real/"
  local remote_allow="${remote}v1/"

  g() {
    "$genesis_bin" --selfhost-only --selfhost-artifact "$ARTIFACT" "$@"
  }

  store_put_literal() {
    local dir="$1"
    local caps="$2"
    local file="$3"
    local content="$4"
    printf '%s\n' "$content" >"$dir/$file"
    g store --caps "$caps" put --input "$dir/$file" | tr -d '\n'
  }

  local caps_publisher="$publisher_real/caps.toml"
  local caps_consumer="$consumer_real/caps.toml"

  cat >"$caps_publisher" <<EOF_CAPS
allow = [
  "core/store::put",
  "core/store::get",
  "core/store::has",
  "core/refs::get",
  "core/refs::set",
  "core/sync::push",
  "core/pkg-low::snapshot",
  "core/gpk-low::export"
]

[store]
dir = "$publisher_real/.genesis/store"

[refs]
path = "$publisher_real/.genesis/refs.gc"

[op."core/sync::push"]
remote_allow = ["$remote_allow"]
wasi_network_profile = "local"

[op."core/pkg-low::snapshot"]
base_dir = "$publisher_real"

[op."core/gpk-low::export"]
base_dir = "$publisher_real"
create_dirs = true
EOF_CAPS

  cat >"$caps_consumer" <<EOF_CAPS
allow = [
  "core/store::get",
  "core/store::has",
  "core/refs::get",
  "core/sync::pull",
  "core/gpk-low::import"
]

[store]
dir = "$consumer_real/.genesis/store"
remote = "$remote"
remote_allow = ["$remote_allow"]

[refs]
path = "$consumer_real/.genesis/refs.gc"

[op."core/store::get"]
wasi_network_profile = "local"

[op."core/store::has"]
wasi_network_profile = "local"

[op."core/sync::pull"]
remote_allow = ["$remote_allow"]
wasi_network_profile = "local"

[op."core/gpk-low::import"]
base_dir = "$consumer_real"
EOF_CAPS

  local policy_h
  local patch_h
  local evidence_h
  local snapshot_h
  local commit_h

  policy_h="$(store_put_literal "$publisher_real" "$caps_publisher" "policy.gc" '{
  :type :vcs/policy
  :v 1
  :name "policy:remote-registry-parity-v0.1"
  :refs {:frozen-prefixes []}
  :classes {
    :dev  {:patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests]}
    :main {:patterns ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests]}
    :tags {:patterns ["refs/**/tags/*"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
  }
}')"

  patch_h="$(store_put_literal "$publisher_real" "$caps_publisher" "patch.gc" '{:type :vcs/patch :v 1 :ops []}')"
  evidence_h="$(store_put_literal "$publisher_real" "$caps_publisher" "evidence.gc" '{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}')"
  snapshot_h="$(store_put_literal "$publisher_real" "$caps_publisher" "snapshot.gc" '{
  :type :vcs/snapshot
  :v 1
  :kind :package
  :pkg/name "parity-mini"
  :pkg/version "0.0.1"
  :modules []
  :obligations []
}')"
  commit_h="$(store_put_literal "$publisher_real" "$caps_publisher" "commit.gc" "{
  :type :vcs/commit
  :v 1
  :parents []
  :base nil
  :patch \"$patch_h\"
  :result \"$snapshot_h\"
  :obligations [core/obligation::unit-tests]
  :evidence [\"$evidence_h\"]
  :attestations []
  :message \"remote-registry parity commit\"
}")"

  g refs --caps "$caps_publisher" set "refs/heads/main" "$commit_h" --policy "$policy_h" >/dev/null
  g sync --caps "$caps_publisher" push \
    --remote "$remote" \
    --root "$commit_h" \
    --root "$policy_h" \
    --set-ref "refs/heads/main:$commit_h:$policy_h@nil" >/dev/null

  local has_remote
  has_remote="$(g store --caps "$caps_consumer" has "$commit_h" | tr -d '\n')"
  if [[ "$has_remote" != "true" ]]; then
    echo "remote-registry-runtime-parity: lane=$lane expected remote store has=true for $commit_h got=$has_remote" >&2
    exit 1
  fi

  g store --caps "$caps_consumer" get "$commit_h" >/dev/null
  g sync --caps "$caps_consumer" pull --remote "$remote" --ref "refs/heads/main" --force >/dev/null

  local pulled_ref
  pulled_ref="$(g refs --caps "$caps_consumer" get "refs/heads/main" | tr -d '\n')"
  if [[ "$pulled_ref" != "$commit_h" ]]; then
    echo "remote-registry-runtime-parity: lane=$lane pulled ref mismatch expected=$commit_h got=$pulled_ref" >&2
    exit 1
  fi

  local bundle="$publisher_real/parity-mini.gpk"
  local bundle_h
  local imported_root

  bundle_h="$(
    cd "$publisher_real"
    g pkg --caps "$caps_publisher" export --snapshot "$snapshot_h" --out "parity-mini.gpk" | tr -d '\n'
  )"
  if [[ ! -f "$bundle" ]]; then
    echo "remote-registry-runtime-parity: lane=$lane missing exported bundle $bundle" >&2
    exit 1
  fi

  cp "$bundle" "$consumer_real/parity-mini.gpk"
  imported_root="$(
    cd "$consumer_real"
    g pkg --caps "$caps_consumer" import --input "parity-mini.gpk" | tr -d '\n'
  )"
  if [[ "$imported_root" != "$snapshot_h" ]]; then
    echo "remote-registry-runtime-parity: lane=$lane imported root mismatch expected=$snapshot_h got=$imported_root" >&2
    exit 1
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$lane" "$commit_h" "$snapshot_h" "$bundle_h" "$imported_root" "$pulled_ref" "$remote" \
    >>"$RESULTS_TSV"
}

run_lane "native" "$GENESIS_NATIVE_BIN"
run_lane "wasi" "$GENESIS_WASI_BIN"

# Chunked upload path is exercised by in-process registry conformance tests.
cargo test -p gc_effects --test sync_registry sync_push_uses_chunked_upload_when_remote_advertises_small_chunks --quiet

python3 - "$RESULTS_TSV" "$REPORT_OUT" <<'PY'
import json
import pathlib
import sys

rows_path = pathlib.Path(sys.argv[1])
report_path = pathlib.Path(sys.argv[2])

lanes = {}
for raw in rows_path.read_text(encoding="utf-8").splitlines():
    if not raw.strip():
        continue
    lane, commit_h, snapshot_h, bundle_h, imported_root, pulled_ref, remote = raw.split("\t")
    lanes[lane] = {
        "commit_hash": commit_h,
        "snapshot_hash": snapshot_h,
        "bundle_hash": bundle_h,
        "imported_root": imported_root,
        "pulled_ref_hash": pulled_ref,
        "remote": remote,
    }

required_lanes = {"native", "wasi"}
missing = sorted(required_lanes - set(lanes.keys()))
if missing:
    raise SystemExit(
        "remote-registry-runtime-parity: missing lane results: " + ", ".join(missing)
    )

cross_lane_consistent = (
    lanes["native"]["commit_hash"] == lanes["wasi"]["commit_hash"]
    and lanes["native"]["snapshot_hash"] == lanes["wasi"]["snapshot_hash"]
    and lanes["native"]["imported_root"] == lanes["wasi"]["imported_root"]
)

report = {
    "kind": "genesis/remote-registry-runtime-parity-report-v0.1",
    "ok": bool(cross_lane_consistent),
    "lanes": lanes,
    "cross_lane_consistent": cross_lane_consistent,
    "operations": [
        "core/store::has (remote)",
        "core/store::get (remote)",
        "core/sync::push",
        "core/sync::pull",
        "core/refs::set (via sync push set-ref)",
        "core/refs::get (post-pull verification)",
        "core/gpk-low::export",
        "core/gpk-low::import",
        "core/store::upload(chunked) via sync registry test",
    ],
}

if not cross_lane_consistent:
    report["errors"] = [
        "native and wasi lanes produced different commit/snapshot/import roots"
    ]

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if not cross_lane_consistent:
    raise SystemExit(
        "remote-registry-runtime-parity: cross-lane outputs diverged; see report"
    )

print(
    "remote-registry-runtime-parity: ok "
    f"lanes={','.join(sorted(lanes.keys()))} report={report_path}"
)
PY
