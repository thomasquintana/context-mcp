# context-mcp

A Rust context engine for agents: multimodal evidence, navigable context maps,
versioned semantic memory, and cited synthesis. Apache-2.0.

**Implementation status:** the provider-independent domain contracts and executable
invariants are implemented. The server and processing pipeline are under development;
the next milestone validates the unchanged HTLK runtime with real MCP providers.
See [the specification](docs/specification.md) and
[acceptance criteria](docs/acceptance.md) for the target behavior and verification.

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
