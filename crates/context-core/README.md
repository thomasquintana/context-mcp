# context-core

Provider-independent domain contracts for context-mcp. Contains typed identifiers,
authorization decisions, content provenance, revisioned semantic memories, context
maps, job transitions, publication fencing, and derivation-aware purge planning.

No provider SDK, network client, database connection, or HTLK execution is hidden
inside these contracts. Persistence must enforce their transactional invariants.

Apache-2.0. See the repository specification and API documentation.
