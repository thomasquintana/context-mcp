# Working on context-mcp

Read the permanent requirements and measured evidence before changing behavior:
`docs/specification.md`, `docs/api.md`, `docs/acceptance.md`, and
`docs/verification.md`. Consult `docs/processing.md`, `docs/context-services.md`,
`docs/memory-and-synthesis.md`, and `docs/permanent-purge.md` for concrete contracts.
`docs/deployment.md` records CLI/container operation. Use Git history for completed
delivery checkpoints; the temporary implementation plan has been retired.

## Architecture boundaries

- One Rust application instance. PostgreSQL, Typesense, and S3-compatible storage.
- Every MCP operation is dispatched through unchanged public HTLK interfaces.
- All MCP providers are replaceable through operator-managed graphs, catalogs,
  and deployment bindings. Provider-specific tools do not belong in domain logic.
- Do not modify sibling/external projects. Explain and ask the user before adding
  any adaptation, custom adapter, compatibility shim, or extra HTLK mechanism.
- Permanent purge covers context-mcp-managed content; HTLK execution history is
  outside its scope. Preserve explicit provenance and authorization boundaries.
- Preserve the single-instance publication/generation fences and transitive source
  protections across ownership/history. Never regenerate purged content implicitly.
- The user approved standard conditional PUTs with permanent zero-byte opaque upload
  fences. These are content-free control records; never delete/reuse active deployment
  fence keys or silently accept versioned payload storage.
- The reference sandbox's reported destroy receipt is accepted despite its measured
  restart/container bug. Preserve the evidence and boundary; no watchdog/TTL/Docker
  cleanup bypass is approved. Lost admission-receipt replay has the documented window.

## Delivery

Follow the sibling htlk project's Rust quality standards: documented APIs, strict
formatting/lint/documentation checks, meaningful tests, dependency/license policy,
and reproducible release validation. Never describe fixture-only tests as proof of
real provider compatibility.

Use `tools/verify.py` and the measured per-crate coverage floors/uncovered-line budgets.
The 99% coverage goal is not a claim of current coverage. Actual format/provider
campaigns are separate from pure fixtures; paid tests require explicit invocation.
Credentials are environment/private-file references, never graph inputs or commits.

The project workflow requests a commit after every completed verified task. Inspect
status, diff and recent history first; stage only intended changes. Run the tracked
release audit, locked notice/dependency gates and reproducible release validation.
Push only when requested, and report actual local/remote verification distinctly.
