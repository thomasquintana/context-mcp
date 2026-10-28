#!/usr/bin/env python3
"""Prepare an operator-owned runtime/discovery configuration without secret values."""
import argparse
import json
import os
from pathlib import Path

root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("directory",type=Path)
parser.add_argument("--container",action="store_true")
parser.add_argument("--public-url",default="http://localhost:8089/mcp")
parser.add_argument("--public-s3-url",default="http://localhost:59000")
args=parser.parse_args()
directory=args.directory.resolve();directory.mkdir(parents=True,exist_ok=True,mode=0o700)
(directory/"staging").mkdir(exist_ok=True,mode=0o700)
(directory/"profiles").mkdir(exist_ok=True,mode=0o700)
configuration=json.loads((root/"examples/operator/development.json").read_text())
app=configuration["application"];app["oidc"]=None if args.container else app["oidc"]
configuration["bind"]="0.0.0.0:8089" if args.container else "127.0.0.1:8089"
configuration["public_url"]=args.public_url
from urllib.parse import urlparse
configuration["allowed_hosts"]=[urlparse(args.public_url).hostname,"localhost","127.0.0.1"]
configuration["allowed_origins"]=[urlparse(args.public_url).scheme+"://"+urlparse(args.public_url).netloc]
app["workflows"]["database"]=str(directory/"runtime.sqlite")
app["data"]["staging_directory"]=str(directory/"staging")
catalogs="/configuration/catalogs.json" if args.container else str(directory/"catalogs.json")
for role in app["workflows"]["roles"].values():
    role["source"]=("/etc/context/operator/"+role["source"]) if args.container else str(root/"examples/operator"/role["source"])
    role["catalogs"]=catalogs
if args.container:
    app["workflows"]["endpoints"]["initial-model-provider"]["program"]="/usr/local/bin/llm-mcp"
    app["workflows"]["endpoints"]["initial-sandbox-provider"]["program"]="/usr/local/bin/mcp-sandbox"
    app["data"]["s3_endpoint"]="http://minio:9000"
    app["data"]["s3_public_endpoint"]=args.public_s3_url
    app["data"]["typesense_endpoint"]="http://typesense:8108"
selection=json.loads((root/"examples/integration/discover.json").read_text())
selection["endpoints"]=app["workflows"]["endpoints"]
for name,value in [("runtime.json",configuration),("discovery.json",selection)]:
    path=directory/name
    with path.open("x",encoding="utf-8") as stream:json.dump(value,stream,indent=2)
    os.chmod(path,0o600)
print("Prepared operator configurations in",directory)
