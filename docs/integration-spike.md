# Existing-interface integration spike

## Scope and current result

The `context-workflows` crate uses unchanged HTLK public APIs for explicit live
descriptor discovery, graph compilation/admission, native MCP dispatch, durable
SQLite execution, inspection, and accepted-artifact reads. There is no custom MCP
adapter, descriptor rewriting, text-output shim, or external-project modification.
The fixed spike principal/policy is for operator-run validation, not server auth.

Verified against:

| Component | Inspected/tested identity |
| --- | --- |
| HTLK source | `3448acb5dc7cebef4d08ef37bb79757fea780659` (Cargo Git dependency pinned) |
| llm-mcp | Sibling binary, MCP implementation `llm-mcp` / `0.1.0`; source HEAD `a9b6d2fe3fed686dd2ca2fb67591613f0cf44cb8` |
| mcp-sandbox | Official `v0.2.0` Darwin arm64 release; source tag/HEAD `8c649a415ba7be6aa6e15a4dc51e56dc5990eae0` |
| Sandbox archive SHA-256 | `3c48cd9ff8980b4cd81210e7b9559e4ba0ad0f7e8331c3121ed566ac191efda0` |

Observed successful checks:

- Actual MCP negotiation and complete exact descriptor discovery through HTLK:
  four model tools and five sandbox tools, MCP `2025-11-25`.
- A compiled/admitted graph calls actual `sandbox_list` and `profile_list`; accepted
  outputs are `{"sessions":[]}` and `{"profiles":[]}` for the isolated configuration.
- Successful graph fingerprint:
  `sha256:0aec62b60537ac88ec3be74135eda529e9c62125afb35ef691d054210cb42862`.
- Automated public-interface tests compile and execute a comparison, read its
  accepted boolean output, reopen durable SQLite and obtain the same idempotent
  run/output, and reject wrong-typed inputs at runtime admission.

The additional live model/media and lifecycle checks below extend those observations.
Restart-safe sandbox cleanup and the full application ingestion service remain unproved.

## Reproducing the metadata check

Run from the repository root. Install/build the sibling llm-mcp binary separately
without editing its source. Install the verified official sandbox binary at
`target/providers/mcp-sandbox`. The example endpoints are trusted local operator
configuration and can be changed to other binaries/deployments.

```console
mkdir -p target/providers
gh release download v0.2.0 --repo teran/mcp-sandbox --pattern mcp-sandbox_0.2.0_darwin_arm64.tar.gz --pattern checksums.txt --dir target/providers
shasum -a 256 target/providers/mcp-sandbox_0.2.0_darwin_arm64.tar.gz
tar -xzf target/providers/mcp-sandbox_0.2.0_darwin_arm64.tar.gz -C target/providers mcp-sandbox
ALLOW_DIRS="$PWD/target" SANDBOX_PROFILES_DIR="$PWD/examples/integration/profiles" cargo run -p context-workflows --example provider_spike -- discover examples/integration/discover.json target/providers/catalogs.json
ALLOW_DIRS="$PWD/target" SANDBOX_PROFILES_DIR="$PWD/examples/integration/profiles" cargo run -p context-workflows --example provider_spike -- run examples/integration/sandbox_metadata.json target/providers/sandbox-metadata-result.json
```

Verify the checksum matches the pinned value above before executing the binary.
The output writer refuses to overwrite an existing file. Choose new output paths
for another run; update the run config's catalog path if necessary. Generated
catalogs, runtime databases, binaries and reports remain ignored under `target/`.
No machine-specific paths or credentials are committed.

The discovery-only model endpoint has an explicitly fake API-key string to allow
startup and descriptor enumeration. It never invokes a model tool. **Do not use
that endpoint configuration for processing.** For live processing, use a deployment
with real provider credentials through `inherit_environment`, or an existing
operator-managed HTTP endpoint. OAuth-only llm-mcp enables generation only, so it
does not establish the required embedding/transcription capabilities.

`discover` calls HTLK's existing `McpAdapter::descriptor`. Its provisional selection
binding is not an admitted execution pin and cannot invoke anything in this path.
Exported descriptors are preserved in full; the compiler derives actual schema and
descriptor digests, and the runtime checks those exact pins before execution.

## Actual native-media verification

Docker image access succeeded on resume. Built `deploy/media/Dockerfile` from the
pinned Debian base. The resulting local image identity was
`sha256:121898f939e1cc1d784fb6c09a73159c09f84b8a1379efa1cf7516167edade89`.
The base is pinned; apt repository packages are not claimed reproducible without
an additional release-time package/snapshot policy.

`tools/live_provider_spike.py --skip-image-build --media-only` generates an
operator profile using the actual image ID and staging-owner uid/gid. It uses
HTLK discovery and `sandbox_media.htlk` for create -> execute -> destroy. Observed:

- FFmpeg creates a blue 64x64 video with a synthetic spoken phrase.
- FFmpeg extracts a PNG frame and mono 16 kHz WAV.
- ffprobe reports the expected video dimensions and audio stream.
- Native execution returns exit code zero; output magic bytes match PNG/WAV.
- Explicit destruction removes the actual Docker container, independently checked
  using the test's exact ownership label.

The media graph deliberately checks native exit status after the destroy step so
that a nonzero command result can still reach successful-path cleanup. Nonzero,
transport-error and cancellation paths still require their own observed evidence.
Artifacts: `target/providers/live-6uzuck7r/evidence.json` and `media-result.json`.
Graph fingerprint:
`sha256:5e846faa5f5c41652705a2dcbd7c88a645bb3a73bd69ae974fcf41a0487d8eb0`.

This harness constructs test inputs and launches HTLK hosts; it contains no MCP
client or direct model API call. It is not the production ingestion service or a
proof of end-to-end application publication/retrieval.

## Live model access: initial rejection and successful account activation

The user supplied `../llm-mcp/.env`. Load it into the launch environment and pass
`OPENAI_API_KEY` by name through `inherit_environment`. The key is never serialized
into run configuration or reports. Live calls explicitly select API-key mode.

The live host was verified as `api.openai.com` over HTTPS. Its normal API base is
`https://api.openai.com/v1/`. The actual sibling llm-mcp binary received the key;
the discovery-only fake-key configuration was not used for live inference.

Observed requests through HTLK and llm-mcp:

| Operation | Requested model | Result |
| --- | --- | --- |
| Image generation/description | `gpt-4.1-mini` | HTTP 429 / `rate_limited` |
| Embeddings | `text-embedding-3-small` | HTTP 429 / `rate_limited` |
| Transcription | `whisper-1` | HTTP 429 / `rate_limited` |
| Later isolated minimal embedding | `text-embedding-3-small` | HTTP 429 / `rate_limited` |

The isolated rejected request ID was `req_eb8d6e0b2fd44c45b6f6ed9c9854e770`.
llm-mcp intentionally omits provider response bodies, so its `rate_limited` category
could not distinguish account/project quota from a transient rate limit.

The user then activated the OpenAI account and requested another attempt. A fresh
HTLK admission key dispatched one actual embedding request successfully: model
`text-embedding-3-small`, one 256-dimensional vector, 4 prompt/total tokens. Evidence:
`target/providers/embedding-approved-retry-result.json`.

Subsequent actual calls succeeded through HTLK and llm-mcp:

| Check | Observed result |
| --- | --- |
| Two text embeddings | Ordered indices 0/1, distinct finite 256-dimensional vectors; 10 input/total tokens |
| Speech transcription | `whisper-1` returned the known phrase "The ocean is blue." |
| Image description, initial `gpt-4.1-mini` | Successful API response, but answered "White" for the blue frame; semantic assertion correctly failed |
| Isolated image description, `gpt-4.1` | Completed response correctly identified blue; 274 input + 2 output = 276 tokens |

The failed image assertion was preserved; it was not relaxed into a shape-only
test. The operator fixture now defaults to the verified `gpt-4.1` model, selectable
through `CONTEXT_SPIKE_GENERATION_MODEL`. Image quality is a separate concern from
successful protocol delivery. No provider or HTLK code was changed.

The media-sized audio request initially exhausted the host's tiny 100,000-step
evaluation policy before run admission. Increased that bounded, host-owned policy
to 4,000,000 using existing HTLK configuration. A 70KB JSON public-runtime test
checks admission/typed forwarding/artifact reads against this regression.

Successful model evidence combines the original embedding/transcription graph and
an explicitly identified separate image-inference probe; it is not claimed as one
successful full application pipeline. Files:

- `target/providers/live-tx3h_arn/model-result.json`.
- `target/providers/generation-knbb8pin/result.json` and `evidence.json`.
- `target/providers/live-tx3h_arn/verified-model-evidence.json`.

Validate saved accepted results without issuing new API calls:

```console
python3 tools/live_provider_spike.py --verify-model-result target/providers/live-tx3h_arn/model-result.json --generation-result target/providers/generation-knbb8pin/result.json
```

For a fresh opt-in full provider check, load the environment and run
`python3 tools/live_provider_spike.py --skip-image-build`. This performs paid API
requests. Output files use fresh directories and refuse silent overwrites.

An explicit operator diagnostic uses the public HTLK store API to print only
failure codes, HTTP status, retryability and request IDs; no raw response bodies,
credential values or provider messages are printed:

```console
cargo run --locked -p context-workflows --example inspect_provider_failures -- target/providers/embedding-probe.sqlite run:953d4928dea0ffe0b241ad0c181e04fe:1
```

## Measured sandbox ownership/recovery gap

`tools/check_sandbox_lifecycle.py target/providers/live-apebe2qm` runs two actual
HTLK graphs. The first creates a sandbox and ends; the second starts a fresh
provider process, lists sessions, and invokes destroy with the saved ID. Observed:

```json
{
  "container_survives_original_session": true,
  "new_session_listing": {"sessions": []},
  "destroy_receipt": {"destroyed": 1, "ids": ["docker:f99285f08455"]},
  "container_survives_new_session_destroy": true
}
```

This is an actual failed recovery guarantee, not a fixture limitation. A successful
destroy receipt alone is insufficient evidence of removal across provider restarts.
The probe removed its exact test-owned orphan with the Docker CLI during test
teardown. That teardown is **not** a proposed production bypass or adaptation.
No containers from these tests remain running.

Evidence: `target/providers/live-apebe2qm/lifecycle-evidence.json`. Create graph:
`sha256:66079cbd64b43e0c86cbbfa412d3a414999e3796c6bf82f2b726044db2aa4bd4`.
Recovery graph:
`sha256:98fd57d18d1463871a85b2c3bf54a87688363e883925b3868a97fa55f579baf8`.

## Remaining gates and user decisions

- Live model access is verified after activation. Preserve failed-request and failed
  image-semantic evidence separately from successful inference.
- Obtain direction on sandbox lifetime/recovery guarantees. The reference provider
  cannot recover ownership with its existing session tools after process restart.
  No extra mechanism, alternative cleanup channel, custom adapter, provider change
  or external-project edit is approved or implemented.
- Validate nonzero exits, actual cancellation/process termination, interrupted
  staging, combined application publication/retrieval/purge, and both relevant
  transports. Unit/local-fixture tests cannot replace those gates.

The full ingestion spike remains incomplete until those gates have evidence.
