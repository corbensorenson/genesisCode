# Policy Profiles Index

Last updated: 2026-02-21

This folder contains reference capability policy profiles used by specs/tests.

- `gpu_device_runtime_caps_v0.1.toml`
  - Enables first-party runtime GPU device backend policy (`gpu_backend = "device-runtime"`).
- `gfx_desktop_first_party_caps_v0.1.toml`
  - Enables first-party desktop window/input/audio backend policy (`first_party_profile = "desktop"`).
- `gpu_compute_bridge_device_caps_v0.1.toml`
  - Runtime microbench device bridge profile (reports canonical backend `device-runtime`; build lane uses `device-bridge` feature naming).
- `gpu_compute_bridge_fallback_caps_v0.1.toml`
  - Runtime microbench deterministic fallback profile.

Use these as templates; production deployments should pin and version project-specific policies.
