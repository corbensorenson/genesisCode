#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "domain-starter-registry-bootstrap" \
  ".genesis/build/cargo" \
  "GENESIS_DOMAIN_STARTER_REGISTRY_BOOTSTRAP_CARGO_TARGET_DIR"

GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"
REPORT_OUT="${GENESIS_DOMAIN_STARTER_REGISTRY_BOOTSTRAP_REPORT:-$ROOT_DIR/.genesis/perf/domain_starter_registry_bootstrap_report.json}"

cargo build -p gc_cli --bin genesis >/dev/null

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

publisher="$tmp_dir/publisher"
consumer="$tmp_dir/consumer"
remote_dir="$tmp_dir/remote-registry"
mkdir -p "$publisher" "$consumer" "$remote_dir"

artifact="$tmp_dir/selfhost_toolchain.gc"
if [[ -f "$ROOT_DIR/selfhost/toolchain.gc" ]]; then
  cp "$ROOT_DIR/selfhost/toolchain.gc" "$artifact"
else
  "$GENESIS_BIN" selfhost-artifact --out "$artifact" >/dev/null
fi

remote="file://$remote_dir/"
remote_allow="${remote}v1/"

g() {
  "$GENESIS_BIN" --selfhost-only --selfhost-artifact "$artifact" "$@"
}

store_put() {
  local dir="$1"
  local caps="$2"
  local src="$3"
  local file="$4"
  printf '%s\n' "$src" >"$dir/$file"
  local out
  out="$(g store --caps "$caps" put --input "$dir/$file")"
  printf '%s' "$out" | tr -d '\n'
}

cat >"$publisher/caps.toml" <<EOF_CAPS
allow = [
  "core/store::put",
  "core/store::get",
  "core/store::has",
  "core/refs::get",
  "core/refs::set",
  "core/sync::push",
  "core/pkg-low::snapshot",
  "core/gpk-low::export",
  "core/pkg-low::publish"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg-low::snapshot"]
base_dir = "."

[op."core/gpk-low::export"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::publish"]
base_dir = "."
remote_allow = ["$remote_allow"]
wasi_network_profile = "local"

[op."core/sync::push"]
remote_allow = ["$remote_allow"]
wasi_network_profile = "local"
EOF_CAPS

cat >"$consumer/caps.toml" <<EOF_CAPS
allow = [
  "core/store::put",
  "core/store::get",
  "core/store::has",
  "core/sync::pull",
  "core/pkg-low::init",
  "core/pkg-low::add",
  "core/pkg-low::lock",
  "core/pkg-low::install",
  "core/pkg-low::save-lock",
  "core/pkg-low::load-lock",
  "core/pkg-low::verify",
  "core/pkg-low::info"
]

[store]
dir = "./.genesis/store"
remote = "$remote"
remote_allow = ["$remote_allow"]

[op."core/sync::pull"]
remote_allow = ["$remote_allow"]
wasi_network_profile = "local"

[op."core/pkg-low::init"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::add"]
base_dir = "."

[op."core/pkg-low::lock"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::install"]
base_dir = "."

[op."core/pkg-low::save-lock"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::load-lock"]
base_dir = "."

[op."core/pkg-low::verify"]
base_dir = "."

[op."core/pkg-low::info"]
base_dir = "."
EOF_CAPS

g keygen --out "$publisher/signing_key.toml" >/dev/null

policy_h="$(store_put "$publisher" "$publisher/caps.toml" '{
  :type :vcs/policy
  :v 1
  :name "policy:starter-bundles-v0.1"
  :refs {:frozen-prefixes []}
  :classes {
    :dev  {:patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
    :main {:patterns ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
    :tags {:patterns ["refs/**/tags/*"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
  }
}' policy.gc)"

patch_h="$(store_put "$publisher" "$publisher/caps.toml" '{:type :vcs/patch :v 1 :ops []}' patch.gc)"

mkdir -p "$publisher/starters" "$publisher/bundles"
report_rows="$tmp_dir/starter_rows.tsv"
: >"$report_rows"

starters=(
  "starter-service|service|core/kit/service::status-v1"
  "starter-game-loop|game-loop|core/kit/game::run-fixed-loop"
  "starter-gpu-compute|gpu-compute|core/kit/pipeline::run-spec"
  "starter-data-pipeline|data-pipeline|core/kit/pipeline::run-spec"
  "starter-plugin-ffi|plugin-ffi|core/kit/plugin::invoke"
  "starter-xr|xr|gfx/xr::session-open"
)

for spec in "${starters[@]}"; do
  IFS='|' read -r name domain entry <<<"$spec"

  pkg_dir="$publisher/starters/$name"
  mkdir -p "$pkg_dir"

  cat >"$pkg_dir/lib.gc" <<EOF_MOD
(def starter/meta::record {
  :package "$name"
  :domain "$domain"
  :entry "$entry"
  :ai-first true
})
starter/meta::record
EOF_MOD

  cat >"$pkg_dir/package.toml" <<EOF_PKG
name = "$name"
version = "0.1.0"
obligations = []
dependencies = []

[[modules]]
path = "lib.gc"
EOF_PKG

  acceptance_h="$(g pack --pkg "$pkg_dir/package.toml" | tr -d '\n')"
  signature_h="$(g sign --pkg "$pkg_dir/package.toml" --key "$publisher/signing_key.toml" --acceptance "$acceptance_h" --signatures "$publisher/signatures.gc" | tr -d '\n')"

  snapshot_h="$(g pkg --caps "$publisher/caps.toml" snapshot --pkg "$pkg_dir/package.toml" | tr -d '\n')"
  evidence_h="$(store_put "$publisher" "$publisher/caps.toml" "{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}" "$name.evidence.gc")"
  commit_h="$(store_put "$publisher" "$publisher/caps.toml" "{
    :type :vcs/commit
    :v 1
    :parents []
    :target {:kind :package :name \"$name\"}
    :base nil
    :patch \"$patch_h\"
    :result \"$snapshot_h\"
    :obligations [core/obligation::unit-tests]
    :evidence [\"$evidence_h\"]
    :attestations []
    :message \"publish $name v0.1.0\"
  }" "$name.commit.gc")"

  ref_name="refs/pkgs/$name/tags/v0.1.0"
  g refs --caps "$publisher/caps.toml" set "$ref_name" "$commit_h" --policy "$policy_h" >/dev/null
  published_h="$(g pkg --caps "$publisher/caps.toml" publish --remote "$remote" --ref "$ref_name" --policy "$policy_h" --expected-old nil --commit "$commit_h" | tr -d '\n')"
  if [[ "$published_h" != "$commit_h" ]]; then
    echo "domain-starter-registry-bootstrap: publish mismatch name=$name expected=$commit_h got=$published_h" >&2
    exit 1
  fi

  bundle_out="$publisher/bundles/$name-v0.1.0.gpk"
  (
    cd "$publisher"
    g pkg --caps "$publisher/caps.toml" export --snapshot "$snapshot_h" --out "bundles/$name-v0.1.0.gpk" >/dev/null
  )
  if [[ ! -f "$bundle_out" ]]; then
    echo "domain-starter-registry-bootstrap: missing exported bundle for $name" >&2
    exit 1
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$name" "$domain" "$entry" "$acceptance_h" "$signature_h" "$snapshot_h" "$commit_h" "$bundle_out" \
    >>"$report_rows"
done

(
  cd "$consumer"
  g gcpm --caps "$consumer/caps.toml" new \
    --workspace starter-bootstrap \
    --policy policy:default-v0.1 \
    --registry-default "$remote" >/dev/null
)

for spec in "${starters[@]}"; do
  IFS='|' read -r name _domain _entry <<<"$spec"
  commit_h="$(awk -F'\t' -v n="$name" '$1 == n {print $7}' "$report_rows" | tail -n1)"
  if [[ -z "$commit_h" ]]; then
    echo "domain-starter-registry-bootstrap: missing commit hash for $name" >&2
    exit 1
  fi
  (
    cd "$consumer"
    g gcpm --caps "$consumer/caps.toml" add "$name@commit:$commit_h" --registry default >/dev/null
    g sync --caps "$consumer/caps.toml" pull --remote "$remote" --root "$commit_h" >/dev/null
  )
done

(
  cd "$consumer"
  g gcpm --caps "$consumer/caps.toml" lock --strict >/dev/null
  g gcpm --caps "$consumer/caps.toml" install --strict --frozen >/dev/null
)

for spec in "${starters[@]}"; do
  IFS='|' read -r name _domain _entry <<<"$spec"
  commit_h="$(awk -F'\t' -v n="$name" '$1 == n {print $7}' "$report_rows" | tail -n1)"
  if [[ -z "$commit_h" ]]; then
    echo "domain-starter-registry-bootstrap: missing commit hash for $name" >&2
    exit 1
  fi
  info="$(
    cd "$consumer"
    g gcpm --caps "$consumer/caps.toml" info "$name" | tr -d '\n'
  )"
  if [[ "$info" != *"$commit_h"* ]]; then
    echo "domain-starter-registry-bootstrap: gcpm info missing locked commit for $name" >&2
    exit 1
  fi
done

python3 - "$report_rows" "$REPORT_OUT" "$remote" <<'PY'
import json
import pathlib
import sys

rows_path = pathlib.Path(sys.argv[1])
report_path = pathlib.Path(sys.argv[2])
remote = sys.argv[3]

starters = []
for raw in rows_path.read_text(encoding="utf-8").splitlines():
    if not raw.strip():
        continue
    name, domain, entry, acceptance_h, signature_h, snapshot_h, commit_h, bundle = raw.split("\t")
    starters.append(
        {
            "name": name,
            "domain": domain,
            "entry": entry,
            "acceptance_hash": acceptance_h,
            "signature_hash": signature_h,
            "snapshot_hash": snapshot_h,
            "commit_hash": commit_h,
            "bundle_path": bundle,
        }
    )

report = {
    "kind": "genesis/domain-starter-registry-bootstrap-v0.1",
    "ok": True,
    "remote": remote,
    "starter_count": len(starters),
    "starters": starters,
    "consumer_lock_path": "genesis.lock",
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(
    "domain-starter-registry-bootstrap: ok "
    f"starters={len(starters)} remote={remote} report={report_path}"
)
PY
