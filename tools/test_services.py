#!/usr/bin/env python3
"""Provision disposable actual backing services and the unchanged sandbox test binary."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import time
import urllib.request

ROOT=Path(__file__).resolve().parents[1]

def environment():
    env=os.environ.copy();defaults={
        "CONTEXT_TEST_DATABASE_URL":"postgres://context:context-test-only@127.0.0.1:55432/context",
        "CONTEXT_TEST_S3_ENDPOINT":"http://127.0.0.1:59000","CONTEXT_TEST_S3_ACCESS_KEY":"context-test","CONTEXT_TEST_S3_SECRET_KEY":"context-test-secret",
        "CONTEXT_TEST_TYPESENSE_ENDPOINT":"http://127.0.0.1:58108","CONTEXT_TEST_TYPESENSE_KEY":"context-test-only-key",
        "CONTEXT_TEST_OIDC_ISSUER":"http://127.0.0.1:58080/realms/context-dev","CONTEXT_TEST_OIDC_CLIENT_SECRET":"context-development-client-secret",
    }
    for name,value in defaults.items():env.setdefault(name,value)
    env.setdefault("CONTEXT_TEST_SANDBOX_BINARY",str(ROOT/"target/test-services/mcp-sandbox"))
    env.setdefault("CONTEXT_TEST_MCP_CATALOGS",str(ROOT/"target/test-services/catalogs.json"))
    return env

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--stop",action="store_true");args=parser.parse_args()
    directory=ROOT/"target/test-services";directory.mkdir(parents=True,exist_ok=True)
    env=environment();env.update(CONTEXT_POSTGRES_PASSWORD="context-test-only",CONTEXT_S3_ACCESS_KEY="context-test",CONTEXT_S3_SECRET_KEY="context-test-secret",CONTEXT_TYPESENSE_KEY="context-test-only-key",CONTEXT_KEYCLOAK_ADMIN="context-admin",CONTEXT_KEYCLOAK_PASSWORD="context-development-admin",CONTEXT_CONTAINER_DATABASE_URL="postgres://context:context-test-only@postgres:5432/context",CONTEXT_DATA_DIR=str(directory))
    compose=["docker","compose","--project-name","context-mcp-ci","--file",str(ROOT/"deploy/compose.yml")]
    if args.stop:subprocess.run([*compose,"down","--volumes"],env=env,check=True);return
    subprocess.run([*compose,"up","--detach","--build","postgres","minio","typesense","keycloak"],env=env,check=True)
    for url in [env["CONTEXT_TEST_S3_ENDPOINT"]+"/minio/health/ready",env["CONTEXT_TEST_TYPESENSE_ENDPOINT"]+"/health",env["CONTEXT_TEST_OIDC_ISSUER"]+"/.well-known/openid-configuration"]:
        for _ in range(180):
            try:
                with urllib.request.urlopen(url,timeout=2) as response:
                    if response.status==200:break
            except Exception:time.sleep(1)
        else:raise SystemExit("Disposable backing service did not become ready")
    arch="arm64" if platform.machine() in ("arm64","aarch64") else "amd64";system="darwin" if platform.system()=="Darwin" else "linux"
    checks={"linux_arm64":"9bae9bc7bf3ac571e48e06b3446c9441dad7fe6529640319e67b8a347b65b8eb","linux_amd64":"70dc2eb04dbe8ef8b7288beafd7ea11d5d36d8d9d24e739c13c959d2000cf31c","darwin_arm64":"3c48cd9ff8980b4cd81210e7b9559e4ba0ad0f7e8331c3121ed566ac191efda0","darwin_amd64":"5daa2fec02ca09471aebd3c9ae147d4b59615b0001f0390ce0ab7401087e770d"}
    archive=directory/"sandbox.tar.gz";urllib.request.urlretrieve(f"https://github.com/teran/mcp-sandbox/releases/download/v0.2.0/mcp-sandbox_0.2.0_{system}_{arch}.tar.gz",archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest()!=checks[f"{system}_{arch}"]:raise SystemExit("Provider archive checksum mismatch")
    with tarfile.open(archive) as package:
        source=package.extractfile("mcp-sandbox")
        if source is None:raise SystemExit("Provider executable absent")
        binary=directory/"mcp-sandbox";binary.write_bytes(source.read());binary.chmod(0o755)
    profiles=directory/"profiles";profiles.mkdir(exist_ok=True);env.update(ALLOW_DIRS=str(directory),SANDBOX_PROFILES_DIR=str(profiles))
    selection=json.loads((ROOT/"examples/integration/discover.json").read_text());del selection["servers"]["llm"]
    selection["endpoints"]={"initial-sandbox-provider":{"transport":"stdio","program":str(binary),"arguments":[],"environment":{},"inherit_environment":["ALLOW_DIRS","SANDBOX_PROFILES_DIR"]}}
    config=directory/"discovery.json";config.write_text(json.dumps(selection));catalogs=directory/"catalogs.json";catalogs.unlink(missing_ok=True)
    subprocess.run([str(ROOT/"target/debug/context-mcp"),"discover-providers","--config",str(config),"--output",str(catalogs)],env=env,check=True)
    print("Actual backing services and sandbox fixture ready")

if __name__=="__main__":main()
