#!/usr/bin/env python3
"""Reproduce strict checks, actual-service tests, per-crate coverage and release gates."""
import argparse
import os
from pathlib import Path
import subprocess
from test_services import ROOT,environment

def run(command,env):subprocess.run(command,cwd=ROOT,env=env,check=True,timeout=1200)

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--coverage",action="store_true");parser.add_argument("--baseline",action="store_true");args=parser.parse_args()
    env=environment();env["ALLOW_DIRS"]=str(ROOT/"target/test-services");env["SANDBOX_PROFILES_DIR"]=str(ROOT/"target/test-services/profiles")
    run(["cargo","fmt","--all","--check"],env)
    run(["cargo","clippy","--workspace","--all-targets","--all-features","--locked","--","-D","warnings"],env)
    run(["cargo","deny","--log-level","error","check","advisories","licenses","bans","sources"],env)
    if args.coverage:
        output=ROOT/"target/coverage";output.mkdir(parents=True,exist_ok=True)
        run(["cargo","llvm-cov","--workspace","--all-targets","--all-features","--locked","--json","--output-path",str(output/"coverage.json"),"--","--include-ignored","--skip","actual_video","--test-threads=1"],env)
        run(["python3","tools/coverage.py",str(output/"coverage.json"),*(["--write-baseline"] if args.baseline else [])],env)
    else:run(["cargo","test","--workspace","--all-features","--locked","--","--include-ignored","--skip","actual_video","--test-threads=1"],env)
    docs=env.copy();docs["RUSTDOCFLAGS"]="-D warnings";run(["cargo","doc","--workspace","--all-features","--no-deps","--locked"],docs)
    run(["python3","tools/test_coverage.py"],env)
    run(["cargo","package","-p","context-core","--locked","--allow-dirty"],env)
    print("Strict, actual-service and release checks passed")

if __name__=="__main__":main()
