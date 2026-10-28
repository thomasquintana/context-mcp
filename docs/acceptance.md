# Acceptance and evidence

Status describes the current checkout, not intended capability. Each production
claim needs observed evidence; fixture coverage cannot establish provider behavior.

| ID | Criterion | Current evidence |
| --- | --- | --- |
| A01 | Typed, provider-neutral source/memory/context contracts | Implemented in context-core |
| A02 | Credential scope and inherited/cross-tenant grants both required | Domain and actual PostgreSQL tests; request authentication/authorization integration pending |
| A03 | Terminal jobs immutable; stale generation cannot publish | Domain tests and actual PostgreSQL generation/ancestor publication tests; full worker integration pending |
| A04 | Transitive mixed-source purge; citation-only assertions survive | Domain unit test including cyclic lineage; physical cleanup pending |
| A05 | Real unchanged HTLK compilation/admission/execution | Verified public-API integration tests and actual sandbox metadata graph; see integration-spike.md |
| A06 | Real initial sandbox execution and model processing | Actual FFmpeg extraction/destruction, finite embeddings, known-phrase transcription and correct image inference verified; initial 429 and mini-model image error preserved separately |
| A07 | Complete agreed media-format matrix | Specified; implementation/format fixtures pending |
| A08 | All-or-nothing revision publication and durable restart recovery | Pending; actual sandbox restart loses resource ownership and does not remove the saved container |
| A09 | OIDC validation, service credentials and current resource authorization | Actual Keycloak JWT/introspection/rotation and PostgreSQL credentials/snapshot tests pass; request transport binding pending |
| A10 | Uploads/URLs, CLI, stdio and Streamable HTTP | Pending |
| A11 | Hybrid retrieval, budgeted maps and source navigation | Pending |
| A12 | Revisioned memory, concurrent reconciliation and cited synthesis | Contracts implemented; service behavior pending |
| A13 | Revocation and app-managed purge across storage, history and temporary files | Pending |
| A14 | Provider replacement through graphs/catalogs/bindings | Architectural boundary; alternative-provider evidence pending |
| A15 | CI, dependency/license policy, coverage and reproducible releases | Local contract checks available; delivery gates pending |

Initial domain tests cover independent authorization decisions, invalid state
transitions, publication fencing and transitive derivation/citation semantics.
They do not prove server security, real media extraction or external compatibility.
