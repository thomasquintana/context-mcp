# Contributing

Use the pinned Rust toolchain. Every public API needs rustdoc; behavior changes
need meaningful tests and an Unreleased changelog entry. Test authorization,
provenance, failure handling, concurrency, and durable outcomes independently of
implementation details. Do not equate fixture-only tests with real interoperability.

## Checks

```console
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
```

Use workspace packaging when local packages depend on unpublished versions. Cargo
must stage all workspace packages together; an isolated package cannot resolve an
unpublished sibling from crates.io. Release dependency/license policy and generated
third-party notices are part of the delivery milestone.

## Dependencies and execution

All MCP calls go through unchanged HTLK public interfaces. Operator-managed graphs,
catalogs, and bindings select replaceable providers. Source credentials are trusted
deployment settings, not operation arguments or graph source.

Never modify sibling projects. Explain and obtain approval before adding adapters,
compatibility shims, or additional mechanisms. Treat actual provider capability
gaps as explicit decisions rather than silently manufacturing compatibility.

## Task delivery

The temporary implementation plan records requirements and verification evidence.
Read it before work and after context compaction; update it and commit after every
completed task. Remove it only when all acceptance criteria are met, then commit
cleanup and push completed commits to origin.
