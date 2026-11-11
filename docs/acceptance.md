# Acceptance and evidence

Status describes the current checkout, not intended capability. Each production
claim needs observed evidence; fixture coverage cannot establish provider behavior.

| ID | Criterion | Current evidence |
| --- | --- | --- |
| A01 | Typed, provider-neutral source/memory/context contracts | Implemented in context-core |
| A02 | Credential scope and inherited/cross-tenant grants both required | Domain/actual PostgreSQL tests and actual HTTP/stdio request isolation, scope and revocation checks |
| A03 | Terminal jobs immutable; stale generation cannot publish | Domain/PostgreSQL generation/ancestor tests plus actual staged-worker failed replacement, cancellation and retry checks |
| A04 | Transitive mixed-source purge; citation-only assertions survive | Domain cyclic-lineage test plus actual physical purge/restart of mixed/history/foreign derivatives and retained citation-only claims |
| A05 | Real unchanged HTLK compilation/admission/execution | Final HTLK1e405c49 public-API/envelope/checkpoint/reopen tests, all shipped graphs compile, and actual sandbox metadata graph pass; upgrade evidence in verification.md, historical provider spike in integration-spike.md |
| A06 | Real initial sandbox execution and model processing | Upgraded-runtime paid Rust and fresh container journeys verify actual FFmpeg extraction/destruction, finite embeddings and blue/ocean visual/spoken evidence; initial 429 and mini-model image error preserved separately |
| A07 | Complete agreed media-format matrix | Reference Rust/sandbox workflows and28 actual bounded format cases rerun on the upgraded runtime; representative image frames/video sampling and processing limits in processing.md |
| A08 | All-or-nothing revision publication and durable restart recovery | Actual backing-service admission/publication/failure/reopen/missing-handle tests and real Docker video pipeline; accepted sandbox destroy-receipt and29-day lost-receipt replay boundaries documented |
| A09 | OIDC validation, service credentials and current resource authorization | Actual Keycloak JWT/introspection/rotation, PostgreSQL credential/snapshot and HTTP/stdio authentication/revocation checks pass |
| A10 | Uploads/URLs, CLI, stdio and Streamable HTTP | Actual binary/tool/resource/CLI tests, allowlisted exact-URL tests, and real Docker/provider HTTP/stdio journeys pass |
| A11 | Hybrid retrieval, budgeted maps and source navigation | Actual Typesense and authoritative hydration/BPE/revocation/entity/history tests;4 actual native selection cases; live provider query/clip journey passes |
| A12 | Revisioned memory, concurrent reconciliation and cited synthesis | Actual application history/equivalence/complement/conflict/version/authority/citation/save tests plus real model contradiction preservation and saved cited synthesis |
| A13 | Revocation and app-managed purge across storage, history and temporary files | Actual cross-owner physical purge/restart, S3/index/staging cleanup, source revocation, paused late PUT fences, versioned-bucket refusal and live both-transport purge |
| A14 | Provider replacement through graphs/catalogs/bindings | Generic fixed-contract host and operator-only provider schemas; alternate pure role configurations and both required actual providers run without domain edits. Future providers require their own semantic verification, not a blanket compatibility claim |
| A15 | CI, dependency/license policy, coverage and reproducible releases | Upgrade campaign:52 Rust tests, strict checks, unchanged per-crate coverage gates, advisory/license/source/bans, notices, package and deterministic source release pass locally; GitHub CI is verified separately after push |

Initial domain tests cover independent authorization decisions, invalid state
transitions, publication fencing and transitive derivation/citation semantics.
They do not prove server security, real media extraction or external compatibility.

The 2026-11-11 upgrade reruns nonpaid backing-service, transport and native-format
checks. After explicit authorization it also passed the paid Rust video/model case
and a freshly rebuilt HTTP/stdio container journey, including actual model conflict
preservation, saved cited synthesis and transitive purge. Original delivery evidence
is retained separately. See verification.md for exact revisions, current coverage
and evidence boundaries.
