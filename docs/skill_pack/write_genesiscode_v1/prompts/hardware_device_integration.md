# Prompt: Hardware Device Integration

Use this prompt for GPU/XR/device-facing systems that require strict runtime evidence.

## Required Inputs

- target hardware/runtime constraints
- device capability policy requirements
- deterministic replay and safety obligations

## Required Outputs

- capability-safe device contracts
- deterministic runtime + replay evidence references
- fallback posture and fault-injection validation lane

## Minimum Verification

- `bash scripts/check_gpu_compute_device_conformance.sh`
- `bash scripts/check_gpu_xr_productization_kits.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
