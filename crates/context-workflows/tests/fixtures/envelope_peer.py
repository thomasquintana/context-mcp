"""Independent stdio fixture: literal results and dispatch facts, no real provider claims."""
import json
import os
import sys
import time

# Startup is independent of operation execution and never dispatches a tool.
time.sleep(int(os.environ.get("FIXTURE_STARTUP_DELAY_MS", "0")) / 1000)

with open(sys.argv[1]) as source:
    descriptors = [entry["descriptor"] for entry in json.load(source)["tools"]["tools"]]

for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request["method"]
    if method == "initialize":
        result = {"protocolVersion": "2025-11-25", "capabilities": {"tools": {}},
                  "serverInfo": {"name": "envelope-fixture", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": descriptors}
    elif method == "tools/call":
        params = request["params"]
        name = params["name"]
        case = params.get("arguments", {}).get("case", "success")
        with open(sys.argv[2], "a") as facts:
            facts.write(json.dumps({"name": name, "case": case}) + "\n")
        if name == "sandbox_create":
            value = {"id": "opaque-fixture-handle"}
        elif name == "sandbox_exec":
            value = {"id": "opaque-fixture-handle", "exit_code": 7, "stdout": "{}"}
        elif name == "sandbox_destroy":
            assert params["arguments"]["id"] == "opaque-fixture-handle"
            value = {"destroyed": 1}
        else:
            value = {"marker": "independent literal evidence"}
        if case == "large":
            value["payload"] = "Z" * 70_000
        if case == "invalid":
            value = {"marker": 123}
        result = {"content": [{"type": "text", "text": json.dumps(value)}]}
        if case != "text_only":
            result["structuredContent"] = value
        if case == "missing":
            result = {"content": [{"type": "text", "text": "No application JSON was supplied."}]}
        if case == "error":
            result["isError"] = True
    else:
        raise AssertionError("Unexpected fixture method: " + method)
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
