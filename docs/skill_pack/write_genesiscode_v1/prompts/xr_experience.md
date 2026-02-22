# Prompt: XR Experience

Use this prompt when implementing XR runtime features, contracts, or workflows.

## Required Inputs

- `docs/spec/XR_HOST_RUNTIME_v0.1.md`
- `docs/spec/GFX_RUNTIME_BUNDLE_v0.1.md`
- relevant XR workflow/example scripts

## Required Outputs

- XR capability contract deltas
- deterministic replay and parity behavior notes
- updated runtime profile checks for XR pathways

## Minimum Verification

- `bash scripts/check_webxr_browser_conformance_lane.sh`
- `bash scripts/check_agent_reference_workflows.sh`
- `bash scripts/check_write_genesiscode_skill_distribution.sh`
