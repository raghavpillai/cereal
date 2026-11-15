#!/bin/bash
# Rebuilds every shipped icon bitmap from the committed vector masters.
# To change the artwork itself, edit and run scripts/logo/cereal_logo.py first.
set -euo pipefail
project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
swift "$project_root/scripts/generate-app-icons.swift" "$project_root"
