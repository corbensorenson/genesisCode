#!/usr/bin/env bash
set -euo pipefail

readonly WASI_SDK_RELEASE="33"
readonly WASI_SDK_VERSION="33.0"
readonly WASI_SDK_BASE_URL="https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-${WASI_SDK_RELEASE}"

usage() {
  cat <<'EOF'
Usage: scripts/install_wasi_sdk.sh [--install-base DIR] [--github-env FILE]
                                    [--print-shell-env]
       scripts/install_wasi_sdk.sh --version

Installs the pinned official WASI SDK archive after SHA-256 verification. The
installer never replaces an existing valid SDK and emits target-specific C
compiler variables when --github-env is supplied.
EOF
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "install-wasi-sdk: sha256sum or shasum is required" >&2
    return 1
  fi
}

sdk_layout_is_complete() {
  local sdk_path="$1"
  [[ -x "$sdk_path/bin/clang" || -x "$sdk_path/bin/clang.exe" ]] \
    && [[ -f "$sdk_path/share/wasi-sysroot/include/wasm32-wasip1/stdio.h" ]] \
    && [[ -f "$sdk_path/share/wasi-sysroot/lib/wasm32-wasip1/libc.a" ]]
}

sdk_version_from_path() {
  local sdk_path="${WASI_SDK_PATH:-}"
  if [[ -z "$sdk_path" ]]; then
    echo "install-wasi-sdk: WASI_SDK_PATH is not set" >&2
    return 1
  fi
  local name
  name="$(basename "$sdk_path")"
  if [[ ! "$name" =~ ^wasi-sdk-([0-9]+\.[0-9]+)(-[A-Za-z0-9_]+-[A-Za-z0-9_]+)?$ ]]; then
    echo "install-wasi-sdk: WASI_SDK_PATH does not identify a versioned SDK" >&2
    return 1
  fi
  local version="${BASH_REMATCH[1]}"
  sdk_layout_is_complete "$sdk_path" || {
    echo "install-wasi-sdk: SDK clang or WASI sysroot is incomplete" >&2
    return 1
  }
  printf 'wasi-sdk %s\n' "$version"
}

platform_identity() {
  local os arch
  case "$(uname -s)" in
    Darwin) os="macos" ;;
    Linux) os="linux" ;;
    MINGW*|MSYS*|CYGWIN*) os="windows" ;;
    *)
      echo "install-wasi-sdk: unsupported operating system: $(uname -s)" >&2
      return 1
      ;;
  esac
  case "$(uname -m)" in
    arm64|aarch64) arch="arm64" ;;
    x86_64|amd64) arch="x86_64" ;;
    *)
      echo "install-wasi-sdk: unsupported architecture: $(uname -m)" >&2
      return 1
      ;;
  esac
  printf '%s %s\n' "$arch" "$os"
}

archive_sha256() {
  case "$1-$2" in
    arm64-linux) printf '%s\n' "4f98ee738c7abb45c81a94d1461fc53cc569d1cd01498951c8184d841a027844" ;;
    arm64-macos) printf '%s\n' "85c997a2665ead91673b5bb88b7d0df3fc8900df3bfa244f720d478187bbdc78" ;;
    arm64-windows) printf '%s\n' "2f457a62da1ce1a55e2ba77c450401b3551f27f04f0a87112b74c5aa8dd9504f" ;;
    x86_64-linux) printf '%s\n' "0ba8b5bfaeb2adf3f29bab5841d76cf5318ab8e1642ea195f88baba1abd47bce" ;;
    x86_64-macos) printf '%s\n' "18f3f201ba9734e6a4455b0b6410690395a55e9ffa9f6f5066f66083a94b93b3" ;;
    x86_64-windows) printf '%s\n' "df14ca2a2127c2d6b6be07e6f5549b3af9c1b3c0112430c200a4749970c59f06" ;;
    *)
      echo "install-wasi-sdk: no checksum for $1-$2" >&2
      return 1
      ;;
  esac
}

validate_archive_paths() {
  local archive="$1" expected_root="$2"
  tar -tzf "$archive" | python3 -c '
from pathlib import PurePosixPath
import sys

expected = sys.argv[1]
seen = False
for raw in sys.stdin:
    path = PurePosixPath(raw.rstrip("\n"))
    if path.is_absolute() or ".." in path.parts:
        raise SystemExit("install-wasi-sdk: archive contains an unsafe path")
    if not path.parts or path.parts[0] != expected:
        raise SystemExit("install-wasi-sdk: archive root does not match the pinned asset")
    seen = True
if not seen:
    raise SystemExit("install-wasi-sdk: archive is empty")
' "$expected_root"
}

install_base="${GENESIS_WASI_SDK_INSTALL_BASE:-${RUNNER_TOOL_CACHE:-${HOME}/.cache}/genesis/wasi-sdk}"
github_env=""
print_shell_env=0
if [[ "${1:-}" == "--version" ]]; then
  [[ $# -eq 1 ]] || { usage >&2; exit 2; }
  if [[ -z "${WASI_SDK_PATH:-}" ]]; then
    read -r version_arch version_os < <(platform_identity)
    version_base="${GENESIS_WASI_SDK_INSTALL_BASE:-${RUNNER_TOOL_CACHE:-${HOME}/.cache}/genesis/wasi-sdk}"
    export WASI_SDK_PATH="${version_base}/wasi-sdk-${WASI_SDK_VERSION}-${version_arch}-${version_os}"
  fi
  sdk_version_from_path
  exit 0
fi
while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-base)
      [[ $# -ge 2 ]] || { usage >&2; exit 2; }
      install_base="$2"
      shift 2
      ;;
    --github-env)
      [[ $# -ge 2 ]] || { usage >&2; exit 2; }
      github_env="$2"
      shift 2
      ;;
    --print-shell-env)
      print_shell_env=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
done

read -r host_arch host_os < <(platform_identity)
archive_name="wasi-sdk-${WASI_SDK_VERSION}-${host_arch}-${host_os}.tar.gz"
sdk_name="${archive_name%.tar.gz}"
expected_sha256="$(archive_sha256 "$host_arch" "$host_os")"
destination="${install_base}/${sdk_name}"

if ! sdk_layout_is_complete "$destination"; then
  if [[ -e "$destination" ]] && ! sdk_layout_is_complete "$destination"; then
    echo "install-wasi-sdk: refusing to replace incomplete SDK: $destination" >&2
    exit 1
  fi
  mkdir -p "$install_base"
  install_lock="${destination}.install-lock"
  owns_install_lock=0
  for ((attempt = 0; attempt < 1000; attempt++)); do
    if mkdir "$install_lock" 2>/dev/null; then
      owns_install_lock=1
      break
    fi
    if sdk_layout_is_complete "$destination"; then
      break
    fi
    sleep 0.01
  done
  if ! sdk_layout_is_complete "$destination" && [[ "$owns_install_lock" -ne 1 ]]; then
    echo "install-wasi-sdk: concurrent installation did not complete: $destination" >&2
    exit 1
  fi
fi

if ! sdk_layout_is_complete "$destination"; then
  temp_dir="$(mktemp -d "${install_base}/.install-${sdk_name}.XXXXXX")"
  cleanup_install() {
    rm -rf "$temp_dir"
    if [[ "$owns_install_lock" -eq 1 ]]; then
      rmdir "$install_lock" 2>/dev/null || true
    fi
  }
  trap cleanup_install EXIT
  archive="$temp_dir/$archive_name"
  curl --proto '=https' --tlsv1.2 --fail --location --retry 3 --silent --show-error \
    --output "$archive" "$WASI_SDK_BASE_URL/$archive_name"
  observed_sha256="$(sha256_file "$archive")"
  if [[ "$observed_sha256" != "$expected_sha256" ]]; then
    echo "install-wasi-sdk: checksum mismatch for $archive_name" >&2
    exit 1
  fi
  validate_archive_paths "$archive" "$sdk_name"
  tar -xzf "$archive" -C "$temp_dir"
  sdk_layout_is_complete "$temp_dir/$sdk_name" || {
    echo "install-wasi-sdk: extracted SDK clang or sysroot is incomplete" >&2
    exit 1
  }
  mv "$temp_dir/$sdk_name" "$destination"
fi

if [[ "${owns_install_lock:-0}" -eq 1 ]]; then
  rmdir "$install_lock"
  owns_install_lock=0
fi

export WASI_SDK_PATH="$destination"
export WASI_SYSROOT="$destination/share/wasi-sysroot"
sdk_version_from_path >/dev/null

if [[ -n "$github_env" ]]; then
  {
    printf 'WASI_SDK_PATH=%s\n' "$WASI_SDK_PATH"
    printf 'WASI_SYSROOT=%s\n' "$WASI_SYSROOT"
    printf 'CC_wasm32_wasip1=%s/bin/clang\n' "$WASI_SDK_PATH"
    printf 'AR_wasm32_wasip1=%s/bin/llvm-ar\n' "$WASI_SDK_PATH"
    printf 'CFLAGS_wasm32_wasip1=--sysroot=%s\n' "$WASI_SYSROOT"
  } >>"$github_env"
fi

if [[ "$print_shell_env" -eq 1 ]]; then
  printf 'export WASI_SDK_PATH=%q\n' "$WASI_SDK_PATH"
  printf 'export WASI_SYSROOT=%q\n' "$WASI_SYSROOT"
  printf 'export CC_wasm32_wasip1=%q\n' "$WASI_SDK_PATH/bin/clang"
  printf 'export AR_wasm32_wasip1=%q\n' "$WASI_SDK_PATH/bin/llvm-ar"
  printf 'export CFLAGS_wasm32_wasip1=%q\n' "--sysroot=$WASI_SYSROOT"
  printf 'install-wasi-sdk: ready version=%s platform=%s-%s sha256=%s\n' \
    "$WASI_SDK_VERSION" "$host_arch" "$host_os" "$expected_sha256" >&2
else
  printf 'install-wasi-sdk: ready version=%s platform=%s-%s sha256=%s\n' \
    "$WASI_SDK_VERSION" "$host_arch" "$host_os" "$expected_sha256"
fi
