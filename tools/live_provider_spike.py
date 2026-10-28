#!/usr/bin/env python3
"""Opt-in real-provider acceptance harness; all MCP traffic goes through HTLK.

Setup/build operations use the Docker CLI. This script has no MCP client and does
not call OpenAI directly. It prepares test fixtures/configuration, launches the
Rust HTLK host, and independently checks accepted results and physical artifacts.
Credentials are inherited from the environment and never serialized into configs.
"""

import argparse
import base64
import json
import math
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]


def execute(arguments, *, environment=None, capture=False):
    return subprocess.run(
        arguments,
        cwd=ROOT,
        env=environment,
        check=True,
        text=True,
        capture_output=capture,
        timeout=180,
    )


def write_json(path, value):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2)


def host(mode, configuration, output, environment):
    execute(
        ["cargo", "run", "--locked", "-p", "context-workflows", "--example",
         "provider_spike", "--", mode, str(configuration), str(output)],
        environment=environment,
    )
    return json.loads(output.read_text(encoding="utf-8"))


def verify_model_result(model, generation_probe=None):
    """Check semantic fixture values and provider vectors without another API call."""
    initial = model["outputs"]["generated"]
    generated = generation_probe["outputs"]["response"] if generation_probe else initial
    texts = [part.get("text", "") for item in generated["output"] for part in item.get("content", [])]
    assert generated["status"] == "completed"
    assert "blue" in " ".join(texts).lower(), "image description disagreed with independent fixture"
    response = model["outputs"]["embedded"]["response"]
    embedding = response["data"]
    assert [item["index"] for item in embedding] == [0, 1]
    assert all(len(item["embedding"]) == 256 for item in embedding)
    assert embedding[0]["embedding"] != embedding[1]["embedding"]
    assert all(math.isfinite(value) for item in embedding for value in item["embedding"])
    transcript = model["outputs"]["transcribed"]["text"].lower()
    assert "ocean" in transcript and "blue" in transcript
    return {
        "model_fingerprint": model["fingerprint"], "image_color": "blue",
        "embedding_count": 2, "embedding_dimensions": 256,
        "generation_model": generated["model"], "generation_usage": generated["usage"],
        "embedding_model": response["model"], "embedding_usage": response["usage"],
        "transcription": "known phrase recognized",
        "separate_generation_probe": generation_probe is not None,
        "generation_fingerprint": generation_probe["fingerprint"] if generation_probe else model["fingerprint"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--media-image", default="context-mcp-media:spike")
    parser.add_argument("--skip-image-build", action="store_true")
    parser.add_argument("--media-only", action="store_true")
    parser.add_argument("--verify-model-result", type=Path)
    parser.add_argument("--generation-result", type=Path)
    args = parser.parse_args()
    if args.generation_result and not args.verify_model_result:
        parser.error("--generation-result requires --verify-model-result")
    if args.verify_model_result:
        model = json.loads(args.verify_model_result.read_text(encoding="utf-8"))
        generation = json.loads(args.generation_result.read_text()) if args.generation_result else None
        evidence = verify_model_result(model, generation)
        write_json(args.verify_model_result.parent / "verified-model-evidence.json", evidence)
        print(json.dumps(evidence, indent=2))
        return
    if not args.media_only and not os.environ.get("OPENAI_API_KEY"):
        parser.error("load OPENAI_API_KEY into the launch environment first")

    providers = ROOT / "target" / "providers"
    providers.mkdir(parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="live-", dir=providers))
    os.chmod(directory, 0o700)
    profiles = directory / "profiles"
    profiles.mkdir()
    staging = directory / "media"
    staging.mkdir()
    environment = os.environ.copy()
    environment["ALLOW_DIRS"] = str(staging)
    environment["SANDBOX_PROFILES_DIR"] = str(profiles)
    environment["DOCKER_HOST"] = execute(
        ["docker", "context", "inspect", "--format", "{{.Endpoints.docker.Host}}"],
        capture=True,
    ).stdout.strip()
    if not args.skip_image_build:
        execute(["docker", "build", "--tag", args.media_image,
                 "--file", "deploy/media/Dockerfile", "deploy/media"])
    image_id = execute(
        ["docker", "image", "inspect", "--format", "{{.Id}}", args.media_image],
        capture=True,
    ).stdout.strip()
    # This is an operator-authored profile, using the staging owner's uid/gid.
    # Pin the image by its actual local digest, not a mutable tag, for execution.
    (profiles / "media.yml").write_text(
        "apiVersion: docker.profiles.sandbox.teran.dev/v1\n"
        "name: media\ndescription: Fixed FFmpeg integration fixture\nspec:\n"
        f"  image: {image_id}\n"
        "  network:\n    mode: none\n"
        "  cgroups:\n    cpu_cores: 1\n    memory_gb: 1\n    pids_limit: 128\n"
        f"  user:\n    uid: {os.getuid()}\n    gid: {os.getgid()}\n"
        "  sudo: never\n  cwd:\n    mode: rw\n  shell: bash\n",
        encoding="utf-8",
    )

    endpoints = {
        "initial-model-provider": {
            "transport": "stdio",
            "program": os.environ.get("LLM_MCP_BINARY", str(ROOT.parent / "llm-mcp/target/debug/llm-mcp")),
            "arguments": ["serve"],
            "environment": {"LLM_MCP_AUTH": "api_key", "LLM_MCP_TRANSPORT": "stdio"},
            "inherit_environment": ["OPENAI_API_KEY"],
        },
        "initial-sandbox-provider": {
            "transport": "stdio",
            "program": os.environ.get("SANDBOX_MCP_BINARY", str(providers / "mcp-sandbox")),
            "arguments": [],
            "environment": {},
            "inherit_environment": ["ALLOW_DIRS", "SANDBOX_PROFILES_DIR", "DOCKER_HOST"],
        },
    }
    selection = json.loads((ROOT / "examples/integration/discover.json").read_text())
    if args.media_only:
        del selection["servers"]["llm"]
        del endpoints["initial-model-provider"]
    selection["endpoints"] = endpoints
    discover_config = directory / "discovery.json"
    write_json(discover_config, selection)
    catalogs = directory / "catalogs.json"
    host("discover", discover_config, catalogs, environment)

    # Fixed synthetic content; no untrusted filename or shell interpolation.
    command = (
        "ffmpeg -hide_banner -loglevel error -f lavfi -i color=c=blue:s=64x64:r=1 "
        "-f lavfi -i 'flite=text=The ocean is blue.:voice=slt' "
        "-t 3 -c:v mpeg4 -c:a aac -threads 1 fixture.mp4 && "
        "ffmpeg -hide_banner -loglevel error -i fixture.mp4 -frames:v 1 -threads 1 frame.png && "
        "ffmpeg -hide_banner -loglevel error -i fixture.mp4 -vn -ac 1 -ar 16000 speech.wav && "
        "ffprobe -v error -show_entries stream=codec_type,width,height "
        "-of json fixture.mp4"
    )
    media_config = directory / "media-job.json"
    write_json(media_config, {
        "source": str(ROOT / "examples/integration/sandbox_media.htlk"),
        "catalogs": str(catalogs),
        "inputs": {
            "create_request": {"profile": "media", "cwd": str(staging), "session_id": directory.name},
            "command": command,
        },
        "endpoints": {"initial-sandbox-provider": endpoints["initial-sandbox-provider"]},
        "database": "media-runtime.sqlite", "idempotency_key": directory.name,
    })
    media = host("run", media_config, directory / "media-result.json", environment)
    assert media["outputs"]["execution"]["exit_code"] == 0
    probe = json.loads(media["outputs"]["execution"]["stdout"])
    assert probe["streams"] == [
        {"codec_type": "video", "width": 64, "height": 64}, {"codec_type": "audio"},
    ]
    assert (staging / "frame.png").read_bytes().startswith(b"\x89PNG\r\n\x1a\n")
    assert media["outputs"]["cleanup"]["destroyed"] == 1
    # Check physical container disappearance, rather than trusting a tool receipt.
    session = media["outputs"]["execution"]["id"]
    remaining = execute(
        ["docker", "ps", "-aq", "--filter", f"label=dev.teran.sandbox.opencode_session={directory.name}"],
        capture=True,
    ).stdout.strip()
    assert not remaining, "sandbox container survived successful cleanup"
    audio = staging / "speech.wav"
    assert audio.read_bytes().startswith(b"RIFF")
    if args.media_only:
        write_json(directory / "evidence.json", {
            "image_id": image_id, "media_fingerprint": media["fingerprint"],
            "sandbox_session": session, "frame_dimensions": [64, 64],
            "audio": "mono 16 kHz WAV, synthetic known phrase", "cleanup": "container absent",
        })
        print(f"Live FFmpeg checks passed. Evidence: {directory.relative_to(ROOT)}")
        return
    image = base64.b64encode((staging / "frame.png").read_bytes()).decode("ascii")
    model_config = directory / "model-job.json"
    write_json(model_config, {
        "source": str(ROOT / "examples/integration/model_media.htlk"),
        "catalogs": str(catalogs),
        "inputs": {
            "generation": {
                "model": os.environ.get("CONTEXT_SPIKE_GENERATION_MODEL", "gpt-4.1"),
                "input": [{"role": "user", "content": [
                    {"type": "input_text", "text": "What is the dominant color? Reply with only its name."},
                    {"type": "input_image", "image_url": f"data:image/png;base64,{image}"},
                ]}],
                "max_output_tokens": 64, "store": False,
            },
            "embedding": {
                "model": os.environ.get("CONTEXT_SPIKE_EMBEDDING_MODEL", "text-embedding-3-small"),
                "input": ["A blue video frame.", "A red document cover."], "dimensions": 256,
            },
            "transcription": {
                "model": os.environ.get("CONTEXT_SPIKE_TRANSCRIPTION_MODEL", "whisper-1"),
                "file": {"filename": "speech.wav", "mime_type": "audio/wav",
                         "data_base64": base64.b64encode(audio.read_bytes()).decode("ascii")},
                "response_format": "json",
            },
        },
        "endpoints": {"initial-model-provider": endpoints["initial-model-provider"]},
        "database": "model-runtime.sqlite", "idempotency_key": directory.name,
    })
    model = host("run", model_config, directory / "model-result.json", environment)
    write_json(directory / "evidence.json", {
        "image_id": image_id, "media_fingerprint": media["fingerprint"],
        "sandbox_session": session, "cleanup": "container absent",
        **verify_model_result(model),
    })
    print(f"Live model/media checks passed. Evidence: {directory.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
