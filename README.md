# context-mcp

A Rust context engine for agents: multimodal evidence, navigable context maps,
versioned semantic memory, and cited synthesis. Apache-2.0.

**Implementation status:** the planned service and delivery surfaces are implemented:
authenticated HTTP/stdio MCP and CLI, verified uploads/exact URLs, durable multimodal
processing, hybrid budgeted context maps/navigation, immutable semantic memories,
authority-bounded reconciliation, cited transient/saved synthesis, live sharing,
physical transitive purge and index repair. All MCP operations use unchanged public
HTLK interfaces and replaceable operator graphs/catalogs/bindings.

Actual PostgreSQL/MinIO/Typesense/Keycloak tests, reference-format sandbox checks and
a full container/provider journey over both transports pass. Coverage is measured
and ratcheted per crate; the longer-term 99% goal is not yet reached. The accepted
sandbox provider bug/history boundary and content-free upload fences are explicit.
See [verification and release evidence](docs/verification.md),
[processing](docs/processing.md), and [initial integration evidence](docs/integration-spike.md).
See [the specification](docs/specification.md) and
[acceptance criteria](docs/acceptance.md) for the target behavior and verification.
See [server setup and implemented operations](docs/server-foundation.md).
See [upload/job contracts and recovery evidence](docs/ingestion.md).
See [context/navigation/sharing](docs/context-services.md),
[memory/synthesis](docs/memory-and-synthesis.md),
[permanent purge](docs/permanent-purge.md), and [deployment/CLI](docs/deployment.md).

## Architecture

One application instance uses PostgreSQL for authoritative application state,
Typesense for derived search indexes, and S3-compatible storage for media. An
embedded HTLK compiler/runtime executes operator-managed workflows. Every MCP
interaction is dispatched through HTLK; servers do not call one another directly.

All providers are replaceable through graphs, exact catalogs, and deployment
bindings. The initial required integration targets are sibling `llm-mcp` and
`mcp-sandbox`. No sibling project is modified. Additional integration mechanisms
require explanation and explicit approval before implementation.

Human authentication is provider-neutral OAuth/OIDC; agents have scoped service
credentials. Local development uses Keycloak as the test issuer.

## Development

The pinned Rust toolchain installs rustfmt and Clippy. Current checks:

```console
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
```

See [CONTRIBUTING.md](CONTRIBUTING.md). No provider credentials are required for the
domain-contract tests. Passing those tests is not evidence of provider compatibility.

## Purge boundary

Permanent purge removes context-mcp-managed content and transitive derivations,
including mixed-source artifacts and historical source-derived memory revisions.
Regeneration is explicit. Independent assertions may retain unavailable citations.
HTLK execution history is outside this boundary and follows HTLK's lifecycle.
