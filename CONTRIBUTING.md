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

For actual services and per-crate coverage, use tools/test_services.py and
tools/verify.py as described in docs/verification.md. Paid provider campaigns are
explicitly opt-in. Keep measured coverage floors/uncovered-line budgets honest; the
99% goal is not a claim of current coverage. Dependency/source/license checks and
THIRD_PARTY_NOTICES.md are required delivery gates. The publishable core is rebuilt
from its Cargo archive; the unpublished application is released as locked source,
binaries and containers using tools/release.py, not a misleading crates.io package.

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
