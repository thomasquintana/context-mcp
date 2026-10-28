#!/usr/bin/env python3
"""Opt-in paid actual-provider, Docker deployment and both-transport acceptance.

All MCP calls use the application's HTLK CLI or compiled HTLK stdio workflow.
Docker CLI is setup/teardown/fixture generation only. No API credential is printed
or serialized; public disposable service credentials are fixture configuration.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import urllib.request
import uuid
from live_provider_spike import ROOT,host,write_json

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--skip-build",action="store_true");args=parser.parse_args()
    if not os.environ.get("OPENAI_API_KEY"):parser.error("inherit the configured model credential before this paid integration test")
    base=ROOT/"target/deployment";base.mkdir(parents=True,exist_ok=True);directory=Path(tempfile.mkdtemp(prefix="verified-",dir=base));os.chmod(directory,0o700)
    project="context-delivery-"+uuid.uuid4().hex[:10];env=os.environ.copy();fixture={
        "CONTEXT_DATA_DIR":str(directory),"CONTEXT_UID":str(os.getuid()),"CONTEXT_GID":str(os.getgid()),
        "CONTEXT_POSTGRES_PASSWORD":"delivery-fixture-password","CONTEXT_S3_ACCESS_KEY":"delivery-fixture","CONTEXT_S3_SECRET_KEY":"delivery-fixture-secret",
        "CONTEXT_TYPESENSE_KEY":"delivery-fixture-index-key","CONTEXT_KEYCLOAK_ADMIN":"context-admin","CONTEXT_KEYCLOAK_PASSWORD":"context-development-admin",
        "CONTEXT_CONTAINER_DATABASE_URL":"postgres://context:delivery-fixture-password@postgres:5432/context",
        "CONTEXT_POSTGRES_PORT":"56432","CONTEXT_S3_PORT":"60000","CONTEXT_S3_CONSOLE_PORT":"60001","CONTEXT_TYPESENSE_PORT":"59108","CONTEXT_KEYCLOAK_PORT":"59080","CONTEXT_HTTP_PORT":"8189",
    };env.update(fixture)
    compose=["docker","compose","--project-name",project,"--file",str(ROOT/"deploy/compose.yml"),"--profile","application"]
    def execute(command,capture=True,timeout=600):
        result=subprocess.run(command,cwd=ROOT,env=env,check=True,text=True,capture_output=capture,timeout=timeout)
        return result.stdout if capture else ""
    def operator(*arguments):return execute([*compose,"run","--rm","--no-deps","-T","application",*arguments])
    counter=0
    def call(tool,arguments):
        nonlocal counter
        counter+=1;path=directory/f"arguments-{counter}.json";write_json(path,arguments)
        output=execute([str(ROOT/"target/debug/context-mcp"),"call","--endpoint","http://localhost:8189/mcp","--token-env","CONTEXT_DELIVERY_TOKEN","--allow-http","--tool",tool,"--arguments",str(path)])
        return json.loads(output)
    def wait_job(receipt):
        for _ in range(300):
            value=call("context_job_status",{"job":receipt["job"]})
            if value["state"]=="succeeded":return value
            if value["state"] in ("failed","cancelled"):raise AssertionError("actual processing failed: "+str(value))
            time.sleep(0.2)
        raise AssertionError("job deadline")
    def wait_purge(receipt):
        for _ in range(100):
            value=call("context_purge_status",{"purge":receipt["purge"]})
            if value["completed"]:return value
            time.sleep(0.2)
        raise AssertionError("purge deadline")
    try:
        if not args.skip_build:execute([*compose,"build","application","minio"],capture=False,timeout=1200)
        execute(["python3","tools/deployment_config.py",str(directory),"--container","--public-url","http://localhost:8189/mcp","--public-s3-url","http://127.0.0.1:60000"])
        execute(["python3","tools/processing_profile.py",str(directory/"profiles/context.yml")])
        execute([*compose,"up","--detach","--no-build","postgres","minio","typesense","keycloak"],capture=False)
        for url in ["http://127.0.0.1:60000/minio/health/ready","http://127.0.0.1:59108/health","http://127.0.0.1:59080/realms/context-dev/.well-known/openid-configuration"]:
            for _ in range(120):
                try:
                    with urllib.request.urlopen(url,timeout=2) as response:
                        if response.status==200:break
                except Exception:time.sleep(1)
            else:raise AssertionError("deployment dependency readiness")
        operator("provision-data","--config","/configuration/runtime.json")
        operator("migrate","--config","/configuration/runtime.json")
        operator("discover-providers","--config","/configuration/discovery.json","--output","/configuration/catalogs.json")
        identity=json.loads(operator("bootstrap","--config","/configuration/runtime.json","--issuer","deployment-acceptance","--subject",project,"--label","Synthetic provider fixture"))
        operator("issue-credential","--config","/configuration/runtime.json","--principal",identity["principal"],"--scope","read","--scope","ingest","--scope","share","--scope","write_memory","--scope","synthesize","--scope","purge","--output","/configuration/agent-token")
        token=(directory/"agent-token").read_text();env["CONTEXT_DELIVERY_TOKEN"]=token;env["CONTEXT_STDIO_TOKEN"]=token
        execute([*compose,"up","--detach","--no-build","application"],capture=False)
        for _ in range(120):
            try:
                with urllib.request.urlopen("http://127.0.0.1:8189/health/ready",timeout=2) as response:
                    if response.status==200:break
            except Exception:time.sleep(1)
        else:raise AssertionError("actual application startup")
        assert call("context_identity",{})["principal"]==identity["principal"]
        image=execute(["docker","image","inspect","--format","{{.Id}}","context-mcp-media:processing"]).strip()
        execute(["docker","run","--rm","--network","none","--user",f"{os.getuid()}:{os.getgid()}","--mount",f"type=bind,src={directory},dst=/fixture","--workdir","/fixture",image,"ffmpeg","-y","-v","error","-f","lavfi","-i","color=c=blue:s=64x64:r=1","-f","lavfi","-i","flite=text=The ocean is blue.:voice=slt","-t","3","-c:v","mpeg4","-c:a","aac","-threads","1","fixture.mp4"])
        video=json.loads(execute([str(ROOT/"target/debug/context-mcp"),"upload","--endpoint","http://localhost:8189/mcp","--token-env","CONTEXT_DELIVERY_TOKEN","--allow-http","--file",str(directory/"fixture.mp4"),"--collection",identity["collection"],"--media-type","video/mp4","--idempotency-key","actual-video","--wait"]))
        context=call("context_query",{"scope":{"kind":"workspace","id":identity["workspace"]},"query":"blue ocean","token_budget":512})
        combined=" ".join(item["text"] for item in context["evidence"]).lower();assert "blue" in combined and "ocean" in combined
        extracted=wait_job(call("context_extract_media",{"asset":video["asset"],"revision":video["revision"],"location":{"kind":"time","start_ms":1000,"end_ms":2000},"idempotency_key":"one-second-clip"}))
        selected=call("context_expand",{"pointer":{"kind":"content","content":extracted["result"]["content"]}})
        with urllib.request.urlopen(selected["url"],timeout=10) as response:data=response.read(2*1024*1024+1)
        assert len(data)<=2*1024*1024 and b"ftyp" in data[:32]
        reference={"asset":video["asset"],"revision":video["revision"],"location":{"kind":"asset"},"available":True}
        memory={"workspace":identity["workspace"],"subject":"Known fixture color","description":"Synthetic evidence assertions","value":{"kind":"text","statement":"The ocean is blue."},"expected_version":0,"derived_from":[reference],"idempotency_key":"blue-assertion"}
        first=call("context_write_memory",memory);memory.update(value={"kind":"text","statement":"The ocean is red."},expected_version=1,idempotency_key="red-assertion")
        conflict=call("context_reconcile_memory",memory);assert conflict["view"]["state"]=="conflicted" and conflict["basis"]=="unresolved_conflict"
        answer=call("context_synthesize",{"workspace":identity["workspace"],"question":"What color is shown and what words were spoken?","evidence":[e["id"] for e in context["evidence"]],"save":True,"idempotency_key":"saved-real-answer"})
        assert "blue" in answer["text"].lower() and answer["citations"]
        generation=call("context_resource",{"resource":{"kind":"asset","id":video["asset"]}})["generation"]
        purged=wait_purge(call("context_purge",{"target":{"kind":"asset","id":video["asset"]},"expected_generation":generation,"idempotency_key":"real-video-purge"}))
        assert not call("context_query",{"scope":{"kind":"workspace","id":identity["workspace"]},"query":"blue ocean"})["evidence"]
        execute([*compose,"stop","application"],capture=False)
        # One outer HTLK run retains the stdio server session through async work.
        catalogs=json.loads(execute([str(ROOT/"target/debug/context-mcp"),"catalog","--transport","stdio"]));write_json(directory/"context-stdio.json",catalogs)
        public_env=dict(fixture)
        endpoint={"transport":"stdio","program":"docker","arguments":[*compose[1:],"run","--rm","--no-deps","-T","application","serve","--config","/configuration/runtime.json","--transport","stdio"],"environment":public_env,"inherit_environment":["OPENAI_API_KEY","CONTEXT_STDIO_TOKEN"]}
        write_json(directory/"stdio-job.json",{"source":str(ROOT/"examples/integration/stdio_context_journey.htlk"),"catalogs":str(directory/"context-stdio.json"),"inputs":{"request":{"workspace":identity["workspace"],"collection":identity["collection"]}},"endpoints":{"context-mcp":endpoint},"database":"client-runtime.sqlite","idempotency_key":"stdio-real-journey"})
        result=host("run",directory/"stdio-job.json",directory/"stdio-result.json",env)["outputs"]["result"]
        assert result["job"]["state"]=="succeeded" and result["purge"]["completed"]
        assert any("ocean" in e["text"].lower() for e in result["context"]["evidence"])
        app_image=execute(["docker","image","inspect","--format","{{.Id}}","context-mcp:0.1.0"]).strip()
        write_json(directory/"evidence.json",{"application_image":app_image,"processing_image":image,"providers":"actual llm-mcp/mcp-sandbox through unchanged HTLK","http_video_retrieval":True,"native_clip":True,"model_conflict_preserved":True,"saved_cited_synthesis":True,"http_transitive_purge":purged["completed"],"stdio_ingest_query_purge":True,"models":["gpt-4.1","whisper-1","text-embedding-3-small"]})
        print("Actual Docker/both-transport/provider journey passed:",directory.relative_to(ROOT))
    finally:
        execute([*compose,"down","--volumes","--remove-orphans"],capture=False)

if __name__=="__main__":main()
