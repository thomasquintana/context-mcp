#!/usr/bin/env python3
"""Build locked release binaries and verify deterministic, secret-excluding source archives."""
import argparse
import gzip
import hashlib
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tomllib

ROOT=Path(__file__).resolve().parents[1]

def archive(paths,version):
    output=io.BytesIO()
    with gzip.GzipFile(filename="",mode="wb",fileobj=output,mtime=0) as compressed:
        with tarfile.open(fileobj=compressed,mode="w|",format=tarfile.PAX_FORMAT) as package:
            for relative in sorted(paths):
                path=ROOT/relative
                if not path.is_file() or path.is_symlink():raise RuntimeError("Unexpected release member type")
                if relative.startswith((".opencode/","target/","data/")) or relative=="AGENTS.md" or any(part.startswith(".env") for part in Path(relative).parts):continue
                data=path.read_bytes();member=tarfile.TarInfo(f"context-mcp-{version}/{relative}");member.size=len(data);member.mode=0o755 if path.stat().st_mode&0o111 else 0o644;member.mtime=0;package.addfile(member,io.BytesIO(data))
    return output.getvalue()

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--skip-build",action="store_true");args=parser.parse_args()
    if not args.skip_build:subprocess.run(["cargo","build","--workspace","--release","--locked"],cwd=ROOT,check=True,timeout=1200)
    version=tomllib.loads((ROOT/"Cargo.toml").read_text())["workspace"]["package"]["version"]
    paths=subprocess.run(["git","ls-files","--cached","--others","--exclude-standard","-z"],cwd=ROOT,check=True,capture_output=True).stdout.decode().split("\0");paths=set(filter(None,paths))
    first=archive(paths,version);second=archive(paths,version)
    if first!=second:raise SystemExit("Source archive is not deterministic")
    output=ROOT/"target/releases";output.mkdir(parents=True,exist_ok=True);name=f"context-mcp-{version}-source.tar.gz";(output/name).write_bytes(first)
    checks=[f"{hashlib.sha256(first).hexdigest()}  {name}"]
    for binary in ["context-mcp","context-process"]:
        path=ROOT/"target/release"/binary
        if path.exists():checks.append(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  ../release/{binary}")
    (output/"SHA256SUMS").write_text("\n".join(checks)+"\n")
    print("Locked release and deterministic source archive verified:",output.relative_to(ROOT))

if __name__=="__main__":main()
