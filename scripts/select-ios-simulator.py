#!/usr/bin/env python3
"""Print an available iPhone UUID for the iOS 26 CI test matrix, without booting it."""
import json
import re
import subprocess
import sys
import uuid

snapshot = json.loads(subprocess.check_output(["xcrun", "simctl", "list", "devices", "available", "--json"], text=True))
candidates = []
for runtime, devices in snapshot.get("devices", {}).items():
    version = re.search(r"\.iOS-(\d+)(?:-(\d+))?", runtime)
    if not version or int(version[1]) != 26:
        continue
    for device in devices:
        kind = device.get("deviceTypeIdentifier", "")
        if device.get("isAvailable") and (".iPhone-" in kind or device.get("name", "").startswith("iPhone")):
            # Prefer the reference device shape, then the newest supported runtime.
            score = (kind.endswith(".iPhone-17-Pro") or device["name"] == "iPhone 17 Pro", int(version[2] or 0), device["name"])
            candidates.append((score, device["udid"]))
if not candidates:
    sys.exit("No available iPhone on an iOS 26 runtime; install the CI simulator runtime before testing.")
selected = max(candidates)[1]
uuid.UUID(selected)
print(selected)
