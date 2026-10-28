#!/usr/bin/env python3
"""Repeat only image inference with an explicit operator-selected model via HTLK."""

import argparse
import json
import os
from pathlib import Path
import tempfile

from live_provider_spike import ROOT, host, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--model", required=True)
    args = parser.parse_args()
    if not os.environ.get("OPENAI_API_KEY"):
        parser.error("load OPENAI_API_KEY into the launch environment first")
    old = json.loads((args.directory / "model-job.json").read_text())
    directory = Path(tempfile.mkdtemp(prefix="generation-", dir=ROOT / "target/providers"))
    os.chmod(directory, 0o700)
    request = old["inputs"]["generation"]
    request["model"] = args.model
    write_json(directory / "request.json", {
        "source": str(ROOT / "examples/integration/generation.htlk"),
        "catalogs": old["catalogs"], "inputs": {"request": request},
        "endpoints": old["endpoints"], "database": "runtime.sqlite",
        "idempotency_key": directory.name,
    })
    result = host("run", directory / "request.json", directory / "result.json", os.environ.copy())
    response = result["outputs"]["response"]
    texts = [part.get("text", "") for item in response["output"] for part in item.get("content", [])]
    evidence = {
        "requested_model": args.model, "returned_model": response["model"],
        "status": response["status"], "matches_known_color": "blue" in " ".join(texts).lower(),
        "usage": response["usage"], "fingerprint": result["fingerprint"],
    }
    write_json(directory / "evidence.json", evidence)
    print(json.dumps(evidence, indent=2))
    print(f"Evidence: {directory.relative_to(ROOT)}")
    assert evidence["matches_known_color"], "image response disagrees with the known blue fixture"


if __name__ == "__main__":
    main()
