# Changelog

## Unreleased

- Record the agreed architecture, API/workflow contracts, and acceptance criteria.
- Add provider-neutral domain types for resource access, immutable source and memory
  revisions, context maps, job transitions, publication fences, and transitive purge.
- Add an explicit operator-run integration host using unchanged public HTLK discovery,
  compiler/runtime and native MCP interfaces, with durable admission tests and actual
  initial-provider protocol evidence. Full media/model verification remains pending.
- Exercise actual sandboxed FFmpeg processing through HTLK, record OpenAI HTTP 429
  failures using metadata-only diagnostics, and reproduce the sandbox's lost
  ownership/ineffective destruction across provider-session restart.
- Verify actual OpenAI embeddings, known-phrase transcription and image inference
  after account activation. Preserve the initial mini-model image semantic failure;
  configure the verified generation model and bounded media-sized evaluator policy.
- Add PostgreSQL application authority/migrations, immutable hierarchy/revisions,
  permission-chain hydration, generation-gated publication, serialized admission
  receipts and concurrent job claims, verified against actual PostgreSQL.
