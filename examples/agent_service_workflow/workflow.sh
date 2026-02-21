#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXAMPLE_DIR="$ROOT_DIR/examples/agent_service_workflow"
GENESIS_BIN="${GENESIS_BIN:-$ROOT_DIR/target/debug/genesis}"

if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

ART="$TMP_DIR/selfhost_toolchain.gc"
REPO_ART="$ROOT_DIR/selfhost/toolchain.gc"
if [[ -f "$REPO_ART" ]]; then
  cp "$REPO_ART" "$ART"
else
  "$GENESIS_BIN" selfhost-artifact --out "$ART" >/dev/null
fi

PUBLISHER="$TMP_DIR/publisher"
CONSUMER="$TMP_DIR/consumer"
REMOTE_DIR="$TMP_DIR/remote-registry"
mkdir -p "$PUBLISHER" "$CONSUMER" "$REMOTE_DIR"
cp "$EXAMPLE_DIR/package.toml" "$PUBLISHER/package.toml"
cp "$EXAMPLE_DIR/service.gc" "$PUBLISHER/service.gc"

REMOTE="file://$REMOTE_DIR/"
REMOTE_ALLOW="${REMOTE}v1/"

cat >"$PUBLISHER/caps.toml" <<EOF
allow = [
  "core/pkg-low::init",
  "core/pkg-low::add",
  "core/pkg-low::lock",
  "core/pkg-low::install",
  "core/pkg-low::save-lock",
  "core/pkg-low::load-lock",
  "core/pkg-low::publish",
  "core/store::put",
  "core/store::get",
  "core/store::has",
  "core/sync::push",
  "core/refs::get",
  "core/refs::set"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg-low::init"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::add"]
base_dir = "."
create_dirs = true

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

[op."core/pkg-low::publish"]
base_dir = "."
remote_allow = ["$REMOTE_ALLOW"]
wasi_network_profile = "local"

[op."core/sync::push"]
remote_allow = ["$REMOTE_ALLOW"]
wasi_network_profile = "local"
EOF

cat >"$CONSUMER/caps.toml" <<EOF
allow = [
  "core/store::has",
  "core/store::get",
  "core/store::put",
  "core/sync::pull"
]

[store]
dir = "./.genesis/store"

[op."core/sync::pull"]
remote_allow = ["$REMOTE_ALLOW"]
wasi_network_profile = "local"
EOF

g() {
  "$GENESIS_BIN" --selfhost-only --selfhost-artifact "$ART" "$@"
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

g gcpm --caps "$PUBLISHER/caps.toml" init --workspace agent-service --lock "$PUBLISHER/genesis.lock" >/dev/null

dep_snapshot_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" '{:type :vcs/snapshot :v 1 :kind :package :pkg/name "dep" :pkg/version "1.0.0" :modules [] :obligations []}' dep_snapshot.gc)"
g gcpm --caps "$PUBLISHER/caps.toml" add "dep@snapshot:$dep_snapshot_h" --lock "$PUBLISHER/genesis.lock" >/dev/null
g gcpm --caps "$PUBLISHER/caps.toml" lock --lock "$PUBLISHER/genesis.lock" --strict >/dev/null
g gcpm --caps "$PUBLISHER/caps.toml" install --lock "$PUBLISHER/genesis.lock" --frozen --strict >/dev/null

policy_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" '{
  :type :vcs/policy
  :v 1
  :name "policy:agent-service"
  :refs {:frozen-prefixes []}
  :classes {
    :dev  {:patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations []}
    :main {:patterns ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
    :tags {:patterns ["refs/**/tags/*"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
  }
}' policy.gc)"

patch_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" '{:type :vcs/patch :v 1 :ops []}' patch.gc)"
result_snapshot_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" '{:type :vcs/snapshot :v 1 :kind :package :pkg/name "agent-service" :pkg/version "0.1.0" :modules [] :obligations []}' result_snapshot.gc)"
evidence_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" '{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}' evidence.gc)"

commit_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" "{
  :type :vcs/commit
  :v 1
  :parents []
  :target {:kind :package :name \"agent-service\"}
  :base nil
  :patch \"$patch_h\"
  :result \"$result_snapshot_h\"
  :obligations [core/obligation::unit-tests]
  :evidence [\"$evidence_h\"]
  :attestations []
  :message \"agent-service publish\"
}" commit.gc)"

g refs --caps "$PUBLISHER/caps.toml" set refs/heads/main "$commit_h" --policy "$policy_h" >/dev/null

published_h="$(g pkg --caps "$PUBLISHER/caps.toml" publish --remote "$REMOTE" --ref refs/heads/main --policy "$policy_h" | tr -d '\n')"
if [[ "$published_h" != "$commit_h" ]]; then
  echo "agent-service-workflow: publish hash mismatch expected=$commit_h got=$published_h" >&2
  exit 1
fi

g sync --caps "$CONSUMER/caps.toml" pull --remote "$REMOTE" --root "$commit_h" >/dev/null

cat >"$CONSUMER/check_pull.gc" <<EOF
(def service/manifest ((((core/kit/service::manifest-v1 "agent-service") "0.1.0") "check_pull") []))

(def prog
  ((core/effect::bind (core/store::has "$commit_h"))
    (fn (present-resp)
      (core/effect::pure
        (((core/kit/service::status-v1 ((core/map::get service/manifest) (quote :name)))
           ((core/map::get present-resp) (quote :present)))
          {:commit "$commit_h"})))))
prog
EOF

run_log="$CONSUMER/check_pull.gclog"
run_out="$(g run "$CONSUMER/check_pull.gc" --caps "$CONSUMER/caps.toml" --log "$run_log" | tr -d '\n')"
replay_out="$(g replay "$CONSUMER/check_pull.gc" --log "$run_log" | tr -d '\n')"
if [[ "$run_out" != "$replay_out" ]]; then
  echo "agent-service-workflow: run/replay mismatch run=$run_out replay=$replay_out" >&2
  exit 1
fi
if [[ "$run_out" != *"true"* ]]; then
  echo "agent-service-workflow: expected pulled commit presence to be true, got=$run_out" >&2
  exit 1
fi

echo "agent-service-workflow: ok publish=$published_h replay=$run_out"
