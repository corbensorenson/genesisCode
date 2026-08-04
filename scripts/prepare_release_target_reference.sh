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

COMMAND_ENV="$(policy_value commandEnv)"
IDENTITY_ENV="$(policy_value identityEnv)"
SDK_IDENTITY_ENV="$(policy_value sdkIdentityEnv)"
REFERENCE_COMMAND="$(policy_value referenceCommand)"
RUNTIME_CLASS="$(policy_value runtimeClass)"
RUNTIME_IDENTITY=""
SDK_IDENTITY=""
IOS_BOOTED_BY_SCRIPT=0
IOS_DEVICE_ID=""

cleanup() {
  if [[ "$IOS_BOOTED_BY_SCRIPT" == "1" && -n "$IOS_DEVICE_ID" ]]; then
    xcrun simctl shutdown "$IOS_DEVICE_ID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

case "$TARGET" in
  android)
    timeout 120 adb wait-for-device
    [[ "$(timeout 30 adb shell getprop sys.boot_completed | tr -d '\r')" == "1" ]] || {
      echo "release-target-reference: Android emulator did not complete boot" >&2
      exit 1
    }
    RUNTIME_IDENTITY="$(timeout 30 adb shell getprop ro.build.fingerprint | tr -d '\r\n')"
    SDK_IDENTITY="sdkmanager=$(sdkmanager --version | sed -n '1p' | tr -d '\r\n');emulator=$(emulator -version | sed -n '1p' | tr -d '\r\n')"
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
    ;;
  service-runtime)
    docker info >/dev/null
    timeout 180 docker pull alpine:3.20 >/dev/null
    RUNTIME_IDENTITY="docker-server=$(docker version --format '{{.Server.Version}}' | tr -d '\r\n')"
    SDK_IDENTITY="$(docker image inspect alpine:3.20 --format '{{.Id}}|{{join .RepoDigests ","}}' | tr -d '\r\n')"
    ;;
esac

emit_env "$COMMAND_ENV" "$REFERENCE_COMMAND"
emit_env "$IDENTITY_ENV" "$RUNTIME_IDENTITY"
emit_env "$SDK_IDENTITY_ENV" "$SDK_IDENTITY"
case "$TARGET" in
  android) emit_env GENESIS_GCPM_ANDROID_RUNTIME_CLASS "$RUNTIME_CLASS" ;;
  edge) emit_env GENESIS_GCPM_EDGE_RUNTIME_CLASS "$RUNTIME_CLASS" ;;
  ios) emit_env GENESIS_GCPM_IOS_RUNTIME_CLASS "$RUNTIME_CLASS" ;;
  service-runtime) emit_env GENESIS_GCPM_SERVICE_RUNTIME_RUNTIME_CLASS "$RUNTIME_CLASS" ;;
esac

echo "release-target-reference: ready target=$TARGET class=$RUNTIME_CLASS"
