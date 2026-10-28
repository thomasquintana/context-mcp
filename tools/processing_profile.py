#!/usr/bin/env python3
"""Write an operator-owned sandbox profile pinned to an actually built image."""
import argparse
import os
from pathlib import Path
import subprocess

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("output",type=Path)
parser.add_argument("--image",default="context-mcp-media:processing")
parser.add_argument("--uid",type=int,default=os.getuid())
parser.add_argument("--gid",type=int,default=os.getgid())
args=parser.parse_args()
image=subprocess.run(["docker","image","inspect","--format","{{.Id}}",args.image],check=True,capture_output=True,text=True,timeout=30).stdout.strip()
if not image.startswith("sha256:") or len(image)!=71:raise SystemExit("Expected actual immutable image identity")
args.output.parent.mkdir(parents=True,exist_ok=True)
with args.output.open("x",encoding="utf-8") as stream:
    stream.write("apiVersion: docker.profiles.sandbox.teran.dev/v1\nname: context\ndescription: Bounded offline context processing\nspec:\n"
        +f"  image: {image}\n  network:\n    mode: none\n  cgroups:\n    cpu_cores: 1\n    memory_gb: 2\n    pids_limit: 128\n  user:\n    uid: {args.uid}\n    gid: {args.gid}\n  sudo: never\n  cwd:\n    mode: rw\n  shell: bash\n")
print("Created pinned processing profile:",args.output)
