#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREPARE="$ROOT_DIR/scripts/prepare_release_target_reference.sh"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/genesis-reference-prepare.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

JQ_BIN="$(command -v jq)"
COMMON_BIN="$TMP_DIR/common-bin"
mkdir -p "$COMMON_BIN"
ln -s "$JQ_BIN" "$COMMON_BIN/jq"
cat >"$COMMON_BIN/timeout" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
shift
exec "$@"
SH
chmod +x "$COMMON_BIN/timeout"

ANDROID_SDK="$TMP_DIR/android-sdk"
mkdir -p \
  "$ANDROID_SDK/platform-tools" \
  "$ANDROID_SDK/cmdline-tools/latest/bin" \
  "$ANDROID_SDK/emulator"
cat >"$ANDROID_SDK/platform-tools/adb" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "$*" in
  "wait-for-device") ;;
  "shell getprop sys.boot_completed") printf '1\n' ;;
  "shell getprop ro.build.fingerprint") printf 'genesis/android/reference:fingerprint\n' ;;
  *) printf 'unexpected adb invocation: %s\n' "$*" >&2; exit 64 ;;
esac
SH
cat >"$ANDROID_SDK/cmdline-tools/latest/bin/sdkmanager" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == "--version" ]]
printf '19.0\n'
SH
cat >"$ANDROID_SDK/emulator/emulator" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
echo "reference-prepare-test: emulator executable must not be invoked" >&2
exit 70
SH
cat >"$ANDROID_SDK/emulator/source.properties" <<'EOF'
Pkg.Desc=Android Emulator
Pkg.Revision=36.2.3.0
EOF
chmod +x \
  "$ANDROID_SDK/platform-tools/adb" \
  "$ANDROID_SDK/cmdline-tools/latest/bin/sdkmanager" \
  "$ANDROID_SDK/emulator/emulator"

ANDROID_ENV="$TMP_DIR/android.env"
(
  cd "$ROOT_DIR"
  PATH="$COMMON_BIN:/usr/bin:/bin" \
    ANDROID_HOME="$ANDROID_SDK" \
    ANDROID_SDK_ROOT="$ANDROID_SDK" \
    /bin/bash "$PREPARE" android "$ANDROID_ENV"
)
grep -Fqx 'GENESIS_GCPM_ANDROID_RUNTIME_IDENTITY=genesis/android/reference:fingerprint' "$ANDROID_ENV"
grep -Fqx 'GENESIS_GCPM_ANDROID_SDK_IDENTITY=sdkmanager=19.0;emulator-package=36.2.3.0' "$ANDROID_ENV"
grep -Fqx 'GENESIS_GCPM_ANDROID_RUNTIME_CMD=java -jar $BUNDLETOOL_JAR build-apks --bundle=$GENESIS_TARGET_PACKAGE --output=$GENESIS_TARGET_ARTIFACT_DIR/app.apks --mode=universal && java -jar $BUNDLETOOL_JAR install-apks --apks=$GENESIS_TARGET_ARTIFACT_DIR/app.apks --device-id="$(adb get-serialno)"' "$ANDROID_ENV"

MISSING_SDK="$TMP_DIR/android-sdk-missing-emulator-metadata"
mkdir -p "$MISSING_SDK/platform-tools" "$MISSING_SDK/cmdline-tools/latest/bin"
cp "$ANDROID_SDK/platform-tools/adb" "$MISSING_SDK/platform-tools/adb"
cp "$ANDROID_SDK/cmdline-tools/latest/bin/sdkmanager" "$MISSING_SDK/cmdline-tools/latest/bin/sdkmanager"
if (
  cd "$ROOT_DIR"
  PATH="$COMMON_BIN:/usr/bin:/bin" \
    ANDROID_HOME="$MISSING_SDK" \
    ANDROID_SDK_ROOT="$MISSING_SDK" \
    /bin/bash "$PREPARE" android "$TMP_DIR/android-missing.env"
) >"$TMP_DIR/android-missing.out" 2>"$TMP_DIR/android-missing.err"; then
  echo "reference-prepare-test: Android setup accepted missing emulator package metadata" >&2
  exit 1
fi
grep -Fq 'missing Android emulator package metadata' "$TMP_DIR/android-missing.err"

for invalid_revision in 'preview' $'36.2.3\nPkg.Revision=36.2.4'; do
  INVALID_SDK="$TMP_DIR/android-sdk-invalid-$RANDOM"
  cp -R "$ANDROID_SDK" "$INVALID_SDK"
  printf 'Pkg.Revision=%s\n' "$invalid_revision" >"$INVALID_SDK/emulator/source.properties"
  if (
    cd "$ROOT_DIR"
    PATH="$COMMON_BIN:/usr/bin:/bin" \
      ANDROID_HOME="$INVALID_SDK" \
      ANDROID_SDK_ROOT="$INVALID_SDK" \
      /bin/bash "$PREPARE" android "$TMP_DIR/android-invalid.env"
  ) >"$TMP_DIR/android-invalid.out" 2>"$TMP_DIR/android-invalid.err"; then
    echo "reference-prepare-test: Android setup accepted invalid emulator package metadata" >&2
    exit 1
  fi
  grep -Fq 'invalid Android emulator package revision' "$TMP_DIR/android-invalid.err"
done

DOCKER_BIN="$TMP_DIR/docker-bin"
mkdir -p "$DOCKER_BIN"
ln -s "$JQ_BIN" "$DOCKER_BIN/jq"
cp "$COMMON_BIN/timeout" "$DOCKER_BIN/timeout"
cat >"$DOCKER_BIN/docker" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-} ${2:-}" in
  "info ") ;;
  "pull alpine:3.20") printf 'pulled\n' ;;
  "version --format") printf '28.5.1\n' ;;
  "image inspect")
    if [[ "${GENESIS_TEST_DOCKER_MALFORMED:-0}" == "1" ]]; then
      printf '[{"Id":"sha256:image","RepoDigests":{}}]\n'
    else
      printf '[{"Id":"sha256:image","RepoDigests":["alpine@sha256:bbb","alpine@sha256:aaa"]}]\n'
    fi
    ;;
  *) printf 'unexpected docker invocation: %s\n' "$*" >&2; exit 64 ;;
esac
SH
chmod +x "$DOCKER_BIN/docker"

SERVICE_ENV="$TMP_DIR/service.env"
(
  cd "$ROOT_DIR"
  PATH="$DOCKER_BIN:/usr/bin:/bin" /bin/bash "$PREPARE" service-runtime "$SERVICE_ENV"
)
grep -Fqx 'GENESIS_GCPM_SERVICE_RUNTIME_RUNTIME_IDENTITY=docker-server=28.5.1' "$SERVICE_ENV"
grep -Fqx 'GENESIS_GCPM_SERVICE_RUNTIME_SDK_IDENTITY=sha256:image|alpine@sha256:aaa,alpine@sha256:bbb' "$SERVICE_ENV"

if (
  cd "$ROOT_DIR"
  PATH="$DOCKER_BIN:/usr/bin:/bin" \
    GENESIS_TEST_DOCKER_MALFORMED=1 \
    /bin/bash "$PREPARE" service-runtime "$TMP_DIR/service-malformed.env"
) >"$TMP_DIR/service-malformed.out" 2>"$TMP_DIR/service-malformed.err"; then
  echo "reference-prepare-test: Docker setup accepted malformed repository digests" >&2
  exit 1
fi
grep -Fq 'Docker image has no immutable repository digest' "$TMP_DIR/service-malformed.err"

echo "reference-prepare-test: ok"
