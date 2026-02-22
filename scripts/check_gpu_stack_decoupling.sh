#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

COMPUTE_BUNDLE="docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md"
GFX_BUNDLE="docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md"
COMBINED_BUNDLE="docs/spec/GPU_GFX_BUNDLE_v0.1.md"
HEALTH_SCRIPT="scripts/check_upgrade_plan_health.sh"
COMPUTE_LANE_SCRIPT="scripts/check_gpu_compute_runtime_profile.sh"
GFX_LANE_SCRIPT="scripts/check_gfx_runtime_profile.sh"
VERIFY_RUNTIME="${GENESIS_GPU_STACK_DECOUPLING_VERIFY_RUNTIME:-0}"

for required in \
  "$COMPUTE_BUNDLE" \
  "$GFX_BUNDLE" \
  "$COMBINED_BUNDLE" \
  "$HEALTH_SCRIPT" \
  "$COMPUTE_LANE_SCRIPT" \
  "$GFX_LANE_SCRIPT"
do
  [[ -f "$required" ]] || {
    echo "gpu-stack-decoupling: missing required file: $required" >&2
    exit 1
  }
done

python3 - "$COMPUTE_BUNDLE" "$GFX_BUNDLE" "$COMBINED_BUNDLE" "$HEALTH_SCRIPT" <<'PY'
import pathlib
import sys

compute_bundle = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
gfx_bundle = pathlib.Path(sys.argv[2]).read_text(encoding="utf-8")
combined_bundle = pathlib.Path(sys.argv[3]).read_text(encoding="utf-8")
health = pathlib.Path(sys.argv[4]).read_text(encoding="utf-8")

for section in ("## Included Specs", "## Cross-Over Points"):
    if section not in compute_bundle:
        raise SystemExit(f"gpu-stack-decoupling: compute bundle missing section: {section}")
    if section not in gfx_bundle:
        raise SystemExit(f"gpu-stack-decoupling: gfx bundle missing section: {section}")

required_compute_refs = (
    "docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md",
    "scripts/check_gpu_compute_runtime_profile.sh",
    "scripts/check_gfx_runtime_profile.sh",
)
for ref in required_compute_refs:
    if ref not in compute_bundle:
        raise SystemExit(f"gpu-stack-decoupling: compute bundle missing cross-over ref: {ref}")

required_gfx_refs = (
    "docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md",
    "scripts/check_gpu_compute_runtime_profile.sh",
    "scripts/check_gfx_runtime_profile.sh",
)
for ref in required_gfx_refs:
    if ref not in gfx_bundle:
        raise SystemExit(f"gpu-stack-decoupling: gfx bundle missing cross-over ref: {ref}")

for ref in ("docs/spec/GPU_COMPUTE_BUNDLE_v0.1.md", "docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md"):
    if ref not in combined_bundle:
        raise SystemExit(f"gpu-stack-decoupling: combined bundle missing split-bundle ref: {ref}")

if "check_gpu_compute_runtime_profile.sh" not in health:
    raise SystemExit(
        "gpu-stack-decoupling: health profile script must include compute-only runtime lane"
    )
if "check_gfx_runtime_profile.sh" not in health:
    raise SystemExit(
        "gpu-stack-decoupling: health profile script must include gfx-only runtime lane"
    )

print("gpu-stack-decoupling: doc/gate topology ok")
PY

if [[ "$VERIFY_RUNTIME" == "1" ]]; then
  bash scripts/check_gpu_compute_runtime_profile.sh
  bash scripts/check_gfx_runtime_profile.sh
fi

echo "gpu-stack-decoupling: ok"
