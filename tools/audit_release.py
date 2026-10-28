#!/usr/bin/env python3
"""Reject credential files/key material in tracked release input without printing values."""
from pathlib import Path
import re
import subprocess

ROOT=Path(__file__).resolve().parents[1]
paths=subprocess.run(["git","ls-files","--cached","-z"],cwd=ROOT,check=True,capture_output=True).stdout.decode().split("\0")
patterns=[rb"sk-[A-Za-z0-9_-]{24,}",rb"AKIA[A-Z0-9]{16}",rb"-----BEGIN (?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY-----"]
failed=[]
for name in filter(None,paths):
    path=ROOT/name
    if not path.is_file():continue
    if any(part.startswith(".env") and part!=".env.example" for part in path.parts) or path.name in ("agent-token","credentials.json"):
        failed.append(name);continue
    data=path.read_bytes()
    if any(re.search(pattern,data) for pattern in patterns):failed.append(name)
if failed:raise SystemExit("Review rejected tracked credential/key material: "+", ".join(failed))
print("Tracked release input contains no credential files or detected private key material")
