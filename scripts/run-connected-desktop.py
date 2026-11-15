#!/usr/bin/env python3
"""Launch the packaged desktop against this checkout's self-hosted instance.

Read only the owner token from the ignored .env. Provider/database credentials
are never passed to the native application. No credential appears in argv/logs.
"""
import os
from pathlib import Path
import re
import sys
from urllib.parse import urlsplit

root = Path(__file__).resolve().parent.parent
binary = root / "dist/Cereal.app/Contents/MacOS/cereal"
if not binary.is_file():
    raise SystemExit("Build the signed application first: ./scripts/package.sh")
token = os.environ.get("CEREAL_SERVER_TOKEN", "")
url = os.environ.get("CEREAL_SERVER_URL", "http://127.0.0.1:3000")
if not token and urlsplit(url).hostname not in {"127.0.0.1", "localhost", "::1"}:
    raise SystemExit("Supply CEREAL_SERVER_TOKEN explicitly for a remote instance")
if not token:
    config = root / ".env"
    if config.is_file():
        match = re.search(r"^ADMIN_API_TOKEN=(.*)$", config.read_text(), re.MULTILINE)
        if match:
            token = match[1].strip().strip("\"'")
if len(token) < 32 or "\n" in token or "\r" in token:
    raise SystemExit("Configure the private instance owner token with ./scripts/setup.sh")
env = {key: value for key, value in os.environ.items() if key not in {
    "ELEVENLABS_API_KEY", "OPENROUTER_API_KEY", "ADMIN_API_TOKEN",
    "DATABASE_URL", "POSTGRES_PASSWORD",
}}
env["CEREAL_SERVER_URL"] = url
env["CEREAL_SERVER_TOKEN"] = token
os.execve(str(binary), [str(binary), *sys.argv[1:]], env)
