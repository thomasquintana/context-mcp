#!/usr/bin/env python3
"""Generate raw-text dependency notices with deterministic LF/whitespace formatting."""
import argparse
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--check",action="store_true");args=parser.parse_args()
result=subprocess.run(["cargo","about","generate","--workspace","--all-features","--locked","--fail","tools/notices.hbs"],cwd=ROOT,check=True,capture_output=True,text=True)
text="\n".join(line.rstrip() for line in result.stdout.splitlines()).rstrip()+"\n"
path=ROOT/"THIRD_PARTY_NOTICES.md"
if args.check:
    if path.read_text()!=text:raise SystemExit("Dependency notices are stale")
    print("Locked dependency notices match")
else:path.write_text(text)
