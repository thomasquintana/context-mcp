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
- Add bounded S3-compatible storage with conditional writes, verified streaming
  downloads, expiring transfer URLs and idempotent deletion; add authorized-ID
  Typesense hybrid-search/index clients. Verify against actual MinIO and Typesense.
- Add a shared long-lived HTLK role host with trusted job grants/provenance labels,
  exact role and provider readiness checks, revocation, protected output reads,
  durable reopening and orderly shutdown through unchanged public interfaces.
- Add provider-neutral JWT/JWKS and RFC7662 authentication, hashed scoped service
  credentials and shared-transaction resource authorization. Verify actual Keycloak
  key rotation, introspection, token rejection and PostgreSQL revocation/race cases.
- Add authenticated MCP HTTP/stdio server foundation, protected-resource discovery,
  explicit operator CLI, resource metadata/collection services and actual-binary
  HTLK-driven transport/isolation/idempotency verification.
- Add immutable, streaming-verified S3 upload promotion, frozen workflow role pins,
  declared accepted-node recovery through existing HTLK inspection, and typed
  provider-neutral processing profile/document contracts for durable ingestion.
- Add authenticated upload/text ingestion, immutable finalization, job status,
  cancellation/retry, bounded durable workers and staged indexing/publication.
- Persist normal-dispatch cancellation fences, reconcile missing HTLK admission
  handles through existing public APIs, restrict cleanup authority and retain
  pending cleanup on failure. Scope staged derivatives to their producing jobs.
- Verify application recovery, retry capacity/idempotency and publication using
  actual PostgreSQL/MinIO/Typesense; extend actual HTTP/stdio binary tests with
  ingestion and fenced external-mutation checks. Processing roles are pure fixtures;
  combined live-provider ingestion/full-format coverage remains pending.
