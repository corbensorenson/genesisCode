#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 2 ]]; then
  echo "usage: $0 <android|edge|ios|service-runtime> <github-env-output>" >&2
  exit 2
fi

TARGET="$1"
GITHUB_ENV_OUTPUT="$2"
POLICY="policies/release_target_reference_set_v0.1.json"

case "$TARGET" in
  android|edge|ios|service-runtime) ;;
  *)
    echo "release-target-reference: unknown target: $TARGET" >&2
    exit 2
    ;;
esac

policy_value() {
  local field="$1"
  jq -er --arg target "$TARGET" --arg field "$field" \
    '.shards[] | select(.target == $target) | .[$field]' "$POLICY"
}

emit_env() {
  local name="$1"
  local value="$2"
  if [[ ! "$name" =~ ^[A-Z][A-Z0-9_]*$ || -z "$value" || "$value" == *$'\n'* || "$value" == *$'\r'* ]]; then
    echo "release-target-reference: invalid environment binding: $name" >&2
    exit 2
  fi
  printf '%s=%s\n' "$name" "$value" >>"$GITHUB_ENV_OUTPUT"
}

resolve_executable() {
  local tool="$1"
  shift
  local candidate=""
  for candidate in "$@"; do
    if [[ -n "$candidate" && -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  candidate="$(command -v "$tool" 2>/dev/null || true)"
  if [[ -n "$candidate" && -x "$candidate" ]]; then
    printf '%s\n' "$candidate"
    return 0
  fi
  echo "release-target-reference: missing required executable: $tool" >&2
  return 1
}

resolve_android_emulator_revision() {
  local candidate=""
  local properties=""
  for candidate in "$@"; do
    if [[ -n "$candidate" && -f "$candidate" ]]; then
      properties="$candidate"
      break
    fi
  done
  if [[ -z "$properties" ]]; then
    echo "release-target-reference: missing Android emulator package metadata" >&2
    return 1
  fi

  local revision=""
  revision="$(
    awk -F= '
      $1 ~ /^[[:space:]]*Pkg[.]Revision[[:space:]]*$/ {
        value = $0
        sub(/^[^=]*=[[:space:]]*/, "", value)
        sub(/[[:space:]\r]+$/, "", value)
        print value
      }
    ' "$properties"
  )"
  if [[ -z "$revision" || "$revision" == *$'\n'* || ! "$revision" =~ ^[0-9]+([.][0-9]+)+(-[A-Za-z0-9._-]+)?$ ]]; then
    echo "release-target-reference: invalid Android emulator package revision" >&2
    return 1
  fi
  printf '%s\n' "$revision"
}

COMMAND_ENV="$(policy_value commandEnv)"
IDENTITY_ENV="$(policy_value identityEnv)"
SDK_IDENTITY_ENV="$(policy_value sdkIdentityEnv)"
REFERENCE_COMMAND="$(policy_value referenceCommand)"
RUNTIME_CLASS="$(policy_value runtimeClass)"
RUNTIME_IDENTITY=""
SDK_IDENTITY=""
IOS_BOOTED_BY_SCRIPT=0
IOS_DEVICE_ID=""
HOST_LIFECYCLE_REPORT=""

cleanup() {
  if [[ "$IOS_BOOTED_BY_SCRIPT" == "1" && -n "$IOS_DEVICE_ID" ]]; then
    xcrun simctl shutdown "$IOS_DEVICE_ID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

case "$TARGET" in
  android)
    adb_candidates=()
    sdkmanager_candidates=()
    emulator_properties_candidates=()
    for sdk_root in "${ANDROID_SDK_ROOT:-}" "${ANDROID_HOME:-}"; do
      [[ -n "$sdk_root" ]] || continue
      adb_candidates+=("$sdk_root/platform-tools/adb")
      sdkmanager_candidates+=(
        "$sdk_root/cmdline-tools/latest/bin/sdkmanager"
        "$sdk_root/cmdline-tools/bin/sdkmanager"
        "$sdk_root/tools/bin/sdkmanager"
      )
      emulator_properties_candidates+=("$sdk_root/emulator/source.properties")
    done
    ADB_BIN="$(resolve_executable adb "${adb_candidates[@]}")"
    SDKMANAGER_BIN="$(resolve_executable sdkmanager "${sdkmanager_candidates[@]}")"
    EMULATOR_REVISION="$(resolve_android_emulator_revision "${emulator_properties_candidates[@]}")"
    timeout 120 "$ADB_BIN" wait-for-device
    [[ "$(timeout 30 "$ADB_BIN" shell getprop sys.boot_completed | tr -d '\r')" == "1" ]] || {
      echo "release-target-reference: Android emulator did not complete boot" >&2
      exit 1
    }
    RUNTIME_IDENTITY="$(timeout 30 "$ADB_BIN" shell getprop ro.build.fingerprint | tr -d '\r\n')"
    SDK_IDENTITY="sdkmanager=$("$SDKMANAGER_BIN" --version | sed -n '1p' | tr -d '\r\n');emulator-package=$EMULATOR_REVISION"
    ;;
  edge)
    RUNTIME_IDENTITY="$(wasmtime --version | tr -d '\r\n')"
    SDK_IDENTITY="$RUNTIME_IDENTITY"
    ;;
  ios)
    IOS_DEVICE_ID="$(xcrun simctl list devices available --json | jq -er '[.devices[][] | select(.name | startswith("iPhone"))][0].udid')"
    IOS_INITIAL_STATE="$(xcrun simctl list devices available --json | jq -er --arg udid "$IOS_DEVICE_ID" '[.devices[][] | select(.udid == $udid)][0].state')"
    if [[ "$IOS_INITIAL_STATE" != "Booted" ]]; then
      xcrun simctl boot "$IOS_DEVICE_ID"
      IOS_BOOTED_BY_SCRIPT=1
    fi
    python3 - "$IOS_DEVICE_ID" <<'PY'
import subprocess
import sys

subprocess.run(["xcrun", "simctl", "bootstatus", sys.argv[1], "-b"], check=True, timeout=180)
PY
    RUNTIME_IDENTITY="$(xcrun simctl list devices available --json | jq -er --arg udid "$IOS_DEVICE_ID" '[.devices[][] | select(.udid == $udid)][0] | [.name,.udid,.state] | join("|")')"
    SDK_IDENTITY="xcode=$(xcodebuild -version | tr '\n' ';' | tr -d '\r');simulator=$(xcrun --sdk iphonesimulator --show-sdk-version | tr -d '\r\n')"
    if [[ "$IOS_BOOTED_BY_SCRIPT" == "1" ]]; then
      xcrun simctl shutdown "$IOS_DEVICE_ID"
      IOS_BOOTED_BY_SCRIPT=0
    fi
    source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
    genesis_configure_cargo_target_dir \
      "$ROOT_DIR" \
      "release-target-reference-host-lifecycle" \
      root-host
    cargo build -p gc_cli --bin genesis --quiet
    # Keep the lifecycle proof inside the iOS replay tree so the target report
    # inventories its exact bytes before the cross-host aggregate consumes it.
    HOST_LIFECYCLE_REPORT=".genesis/perf/reference-target-ios/ios/host_bridge_daemon_lifecycle_report.json"
    python3 scripts/lib/host_bridge_daemon_lifecycle.py \
      --genesis "$CARGO_TARGET_DIR/debug/genesis" \
      --selfhost-artifact "$ROOT_DIR/selfhost/toolchain.gc" \
      --output "$HOST_LIFECYCLE_REPORT"
    ;;
  service-runtime)
    docker info >/dev/null
    timeout 180 docker pull alpine:3.20 >/dev/null
    RUNTIME_IDENTITY="docker-server=$(docker version --format '{{.Server.Version}}' | tr -d '\r\n')"
    SDK_IDENTITY="$(
      docker image inspect alpine:3.20 |
        jq -er '
          (if type != "array" or length != 1 then
             error("expected one Docker image record")
           else .[0] end) as $image
          | if ($image.Id | type) != "string" or ($image.Id | length) == 0 then
              error("Docker image identity is missing")
            elif ($image.RepoDigests | type) != "array" or ($image.RepoDigests | length) == 0 then
              error("Docker image has no immutable repository digest")
            elif any($image.RepoDigests[]; type != "string" or length == 0) then
              error("Docker repository digests must be non-empty strings")
            else
              [$image.Id, ($image.RepoDigests | sort | join(","))] | join("|")
            end
        '
    )"
    ;;
esac

emit_env "$COMMAND_ENV" "$REFERENCE_COMMAND"
emit_env "$IDENTITY_ENV" "$RUNTIME_IDENTITY"
emit_env "$SDK_IDENTITY_ENV" "$SDK_IDENTITY"
if [[ -n "$HOST_LIFECYCLE_REPORT" ]]; then
  emit_env GENESIS_HOST_BRIDGE_DAEMON_LIFECYCLE_REPORT "$HOST_LIFECYCLE_REPORT"
fi
case "$TARGET" in
  android) emit_env GENESIS_GCPM_ANDROID_RUNTIME_CLASS "$RUNTIME_CLASS" ;;
  edge) emit_env GENESIS_GCPM_EDGE_RUNTIME_CLASS "$RUNTIME_CLASS" ;;
  ios) emit_env GENESIS_GCPM_IOS_RUNTIME_CLASS "$RUNTIME_CLASS" ;;
  service-runtime) emit_env GENESIS_GCPM_SERVICE_RUNTIME_RUNTIME_CLASS "$RUNTIME_CLASS" ;;
esac

echo "release-target-reference: ready target=$TARGET class=$RUNTIME_CLASS"
