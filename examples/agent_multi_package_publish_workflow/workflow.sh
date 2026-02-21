#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
EXAMPLE_DIR="$ROOT_DIR/examples/agent_multi_package_publish_workflow"
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

WORK="$TMP_DIR/work"
cp -R "$EXAMPLE_DIR" "$WORK"

PUBLISHER="$WORK/publisher"
CONSUMER="$WORK/consumer"
REMOTE_DIR="$WORK/remote-registry"
mkdir -p "$PUBLISHER" "$CONSUMER" "$REMOTE_DIR"
cp -R "$WORK/lib" "$PUBLISHER/lib"
cp -R "$WORK/app" "$PUBLISHER/app"

REMOTE="file://$REMOTE_DIR/"
REMOTE_ALLOW="${REMOTE}v1/"

cat >"$PUBLISHER/caps.toml" <<EOF
allow = [
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

policy_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" '{
  :type :vcs/policy
  :v 1
  :name "policy:agent-multi-pkg"
  :refs {:frozen-prefixes []}
  :classes {
    :dev  {:patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations []}
    :main {:patterns ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
    :tags {:patterns ["refs/**/tags/*"] :required-obligations [core/obligation::unit-tests] :require-signatures false}
  }
}' policy.gc)"

patch_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" '{:type :vcs/patch :v 1 :ops []}' patch.gc)"
evidence_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" '{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}' evidence.gc)"

lib_snapshot_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" '{:type :vcs/snapshot :v 1 :kind :package :pkg/name "lib-core" :pkg/version "0.1.0" :modules [] :obligations []}' lib_snapshot.gc)"
lib_commit_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" "{
  :type :vcs/commit
  :v 1
  :parents []
  :target {:kind :package :name \"lib-core\"}
  :base nil
  :patch \"$patch_h\"
  :result \"$lib_snapshot_h\"
  :obligations [core/obligation::unit-tests]
  :evidence [\"$evidence_h\"]
  :attestations []
  :message \"lib-core publish\"
}" lib_commit.gc)"
g refs --caps "$PUBLISHER/caps.toml" set refs/pkgs/lib-core/heads/main "$lib_commit_h" --policy "$policy_h" >/dev/null
lib_publish_h="$(g pkg --caps "$PUBLISHER/caps.toml" publish --remote "$REMOTE" --ref refs/pkgs/lib-core/heads/main --policy "$policy_h" | tr -d '\n')"
if [[ "$lib_publish_h" != "$lib_commit_h" ]]; then
  echo "agent-multi-package-publish-workflow: lib publish mismatch expected=$lib_commit_h got=$lib_publish_h" >&2
  exit 1
fi

app_snapshot_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" "{
  :type :vcs/snapshot
  :v 1
  :kind :package
  :pkg/name \"app-main\"
  :pkg/version \"0.1.0\"
  :modules []
  :deps [{:dep/name \"lib-core\" :dep/commit \"$lib_commit_h\" :dep/snapshot \"$lib_snapshot_h\"}]
  :obligations []
}" app_snapshot.gc)"
app_commit_h="$(store_put "$PUBLISHER" "$PUBLISHER/caps.toml" "{
  :type :vcs/commit
  :v 1
  :parents []
  :target {:kind :package :name \"app-main\"}
  :base nil
  :patch \"$patch_h\"
  :result \"$app_snapshot_h\"
  :obligations [core/obligation::unit-tests]
  :evidence [\"$evidence_h\"]
  :attestations []
  :message \"app-main publish\"
}" app_commit.gc)"
g refs --caps "$PUBLISHER/caps.toml" set refs/pkgs/app-main/heads/main "$app_commit_h" --policy "$policy_h" >/dev/null
app_publish_h="$(g pkg --caps "$PUBLISHER/caps.toml" publish --remote "$REMOTE" --ref refs/pkgs/app-main/heads/main --policy "$policy_h" | tr -d '\n')"
if [[ "$app_publish_h" != "$app_commit_h" ]]; then
  echo "agent-multi-package-publish-workflow: app publish mismatch expected=$app_commit_h got=$app_publish_h" >&2
  exit 1
fi

g sync --caps "$CONSUMER/caps.toml" pull --remote "$REMOTE" --root "$app_commit_h" >/dev/null
g sync --caps "$CONSUMER/caps.toml" pull --remote "$REMOTE" --root "$lib_commit_h" >/dev/null

cat >"$CONSUMER/check_multi.gc" <<EOF
(def prog
  ((core/effect::bind
     (core/effect::perform 'core/store::has {:hash "$app_commit_h"} (fn (x) (core/effect::pure x))))
    (fn (has-app)
      ((core/effect::bind
         (core/effect::perform 'core/store::has {:hash "$lib_commit_h"} (fn (x) (core/effect::pure x))))
        (fn (has-lib)
          (core/effect::pure
            {
              :app ((core/map::get has-app) (quote :present))
              :lib ((core/map::get has-lib) (quote :present))
            }))))))
prog
EOF

run_log="$CONSUMER/check_multi.gclog"
run_out="$(g run "$CONSUMER/check_multi.gc" --caps "$CONSUMER/caps.toml" --log "$run_log" | tr -d '\n')"
replay_out="$(g replay "$CONSUMER/check_multi.gc" --log "$run_log" | tr -d '\n')"
if [[ "$run_out" != "$replay_out" ]]; then
  echo "agent-multi-package-publish-workflow: run/replay mismatch run=$run_out replay=$replay_out" >&2
  exit 1
fi
if [[ "$run_out" != *":app true"* || "$run_out" != *":lib true"* ]]; then
  echo "agent-multi-package-publish-workflow: expected app/lib presence true, got=$run_out" >&2
  exit 1
fi

echo "agent-multi-package-publish-workflow: ok lib=$lib_publish_h app=$app_publish_h replay=$run_out"
