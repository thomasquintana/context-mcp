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

**These observations establish protocol/discovery and read-only sandbox invocation,
not FFmpeg processing, actual model quality, cancellation, or crash cleanup.**

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

## Remaining gates and observed environment blockers

- No `OPENAI_API_KEY` is configured in this session. An available ChatGPT sign-in
  cannot supply llm-mcp's API-key-only embedding/transcription operations. Live
  processing requires configured provider access; never paste a secret into chat
  or commit it in an endpoint file.
- `docker version` reaches Docker Desktop, but `docker pull debian:bookworm-slim`
  timed out after 120 seconds with `error getting credentials ... signal: terminated`.
  The media image has not been built and no container-processing test is claimed.
- Validate a job-scoped media profile/image and real FFmpeg success/nonzero exits.
- Establish actual abort and restart cleanup behavior using existing sandbox tools.
  Its inspected session map is process-local; transport termination is not proof
  that a Docker process stopped. Any concrete unmet requirement is reported before
  proposing an additional mechanism. External projects remain unchanged.
- Complete the combined model/media/publication/retrieval/purge workflow and both
  relevant transports. Unit/local-fixture tests cannot replace those gates.

The full ingestion spike remains incomplete until those gates have evidence.
