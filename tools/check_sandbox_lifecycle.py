#!/usr/bin/env python3
"""Measure cross-session cleanup using actual MCP calls through HTLK.

Run against a prior live-spike directory containing a verified profile/image and
catalogs. Docker queries independently verify resource existence; Docker removal
is test teardown only, never a proposed production recovery mechanism.
"""

import argparse
import json
import os
from pathlib import Path

from live_provider_spike import ROOT, execute, host, write_json


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    directory = args.directory.resolve()
    media_config = json.loads((directory / "media-job.json").read_text())
    environment = os.environ.copy()
    environment["ALLOW_DIRS"] = str(directory / "media")
    environment["SANDBOX_PROFILES_DIR"] = str(directory / "profiles")
    environment["DOCKER_HOST"] = execute(
        ["docker", "context", "inspect", "--format", "{{.Endpoints.docker.Host}}"], capture=True,
    ).stdout.strip()
    label = f"label=dev.teran.sandbox.opencode_session={directory.name}-lifecycle"

    def containers():
        return execute(["docker", "ps", "-aq", "--filter", label], capture=True).stdout.split()

    assert not containers(), "lifecycle test requires a fresh job label"
    create_request = dict(media_config["inputs"]["create_request"])
    create_request["session_id"] = f"{directory.name}-lifecycle"
    create_config = {
        "source": str(ROOT / "examples/integration/sandbox_create_only.htlk"),
        "catalogs": media_config["catalogs"], "inputs": {"request": create_request},
        "endpoints": media_config["endpoints"], "database": "lifecycle.sqlite",
        "idempotency_key": "lifecycle-create",
    }
    write_json(directory / "lifecycle-create.json", create_config)
    try:
        created = host("run", directory / "lifecycle-create.json",
                       directory / "lifecycle-created.json", environment)
        session = created["outputs"]["created"]["id"]
        after_original_session = containers()
        assert len(after_original_session) == 1
        restarted_config = {
            **create_config,
            "source": str(ROOT / "examples/integration/sandbox_restart_probe.htlk"),
            "inputs": {"session": session}, "idempotency_key": "lifecycle-restarted",
        }
        write_json(directory / "lifecycle-restarted.json", restarted_config)
        restarted = host("run", directory / "lifecycle-restarted.json",
                         directory / "lifecycle-restarted-result.json", environment)
        evidence = {
            "original_session": session,
            "container_survives_original_session": bool(after_original_session),
            "new_session_listing": restarted["outputs"]["before"],
            "destroy_receipt": restarted["outputs"]["cleanup"],
            "container_survives_new_session_destroy": bool(containers()),
            "create_fingerprint": created["fingerprint"],
            "restart_fingerprint": restarted["fingerprint"],
        }
        write_json(directory / "lifecycle-evidence.json", evidence)
        print(json.dumps(evidence, indent=2))
    finally:
        # Only remove containers with this fresh test's exact label. Cleanup here
        # does not establish that provider APIs can recover these objects.
        for container in containers():
            execute(["docker", "rm", "--force", container], capture=True)
        assert not containers(), "test teardown failed"


if __name__ == "__main__":
    main()
