#!/usr/bin/env bash
set -euo pipefail

export LC_ALL=C

write_gcpm_low_caps_fixture() {
  local out_path="$1"
  cat > "$out_path" <<'EOF'
allow = [
  "core/pkg-low::init",
  "core/pkg-low::lock",
  "core/pkg-low::install",
  "core/pkg-low::update",
  "core/pkg-low::load-lock",
  "core/pkg-low::save-lock",
  "core/pkg-low::env"
]

[op."core/pkg-low::init"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::lock"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::install"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::update"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::load-lock"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::save-lock"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::env"]
base_dir = "."
create_dirs = true
EOF
}
