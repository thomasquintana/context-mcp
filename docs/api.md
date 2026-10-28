# Application and workflow contracts — draft 0.1

These are target contracts, not a declaration that the service endpoints exist.
Public MCP tools return object-valued `structuredContent`, publish input/output
schemas, and expose redacted typed failures. HTTP/CLI use the same application
services and authorization rules.

Implemented tools: `context_identity`, `context_resource`,
`context_create_collection`, `context_create_upload`, `context_finalize_upload`,
`context_ingest_text`, `context_job_status`, `context_cancel_job`, and
`context_retry_job`, `context_ingest_url`, `context_query`, `context_expand`,
`context_extract_media`, `context_asset_history`, `context_reprocess`, and
`context_set_access`. Their exact descriptors are exported by `context-mcp catalog`.
See context-services.md for retrieval/budget/history/navigation and sharing contracts.
Semantic memory and cited generation tools are `context_write_memory`,
`context_reconcile_memory`, `context_read_memory` and `context_synthesize`; see
memory-and-synthesis.md for version/conflict/authority and transient/save boundaries.
Permanent deletion/repair tools are `context_purge`, `context_purge_status`, and
`context_rebuild_index`; see permanent-purge.md for closure, fencing and physical
cleanup completion, including approved content-free upload fences.
See ingestion.md for implemented job/role contracts and server-foundation.md for
setup. The remaining capabilities below await their application milestones.

## Public operations

| Logical operation | Input contract | Output contract |
| --- | --- | --- |
| Create/finalize upload | Collection, file metadata/size/hash, idempotency key; verified completion | Upload session or durable ingestion receipt |
| Ingest exact URL | Collection, URL, selected operator workflow, idempotency key | Asset/revision/job receipt |
| Inspect/cancel/retry job | Authorized job identity; expected state where relevant | Durable state, bounded stage progress and redacted error |
| Refresh/reprocess | Asset, expected current revision, selected workflow | New immutable revision and durable job |
| Query context | Authorized scope, query, bounded filters/limit, optional token budget | ContextMap |
| Expand/navigate | Typed pointer, bounded extraction parameters | Authorized exact-revision evidence/resource references |
| Write/update memory | Workspace, subject, description, typed/text value, evidence, expected version | New revision and current/conflicted view |
| Reconcile memory | Candidate revisions, authorized rules, expected view | New revision or preserved conflict |
| Synthesize | Question and evidence/scope, bounded generation options | Transient cited answer |
| Save generated artifact | Explicit answer/provenance references and destination | Saved generated record with dependencies |
| Share/revoke | Owned resource, recipient, permissions, expected ACL revision | Updated live grant/ACL revision |
| Purge | Owned target, expected generation, idempotency key | Durable purge receipt; completed only after cleanup |

Principals, memberships, owner labels and effective grants are trusted host data.
An operation can select a target scope but cannot manufacture authority by passing
a tenant ID. Exact implementation tool names and URI templates are pinned in the
server catalog once the server transport milestone exists.

## Internal workflow boundary

Graphs declare typed inputs and outputs for extraction, transcription/description,
enrichment, chunking, embedding, indexing and publication. Operator catalogs supply
complete exact descriptors; bindings supply endpoints/credentials. Domains do not
accept arbitrary tool names, endpoints, shell commands, or graph source from users.

Application operations prepare staged inputs, validate outputs, persist data and
publish revisions. The shared host uses existing public HTLK admission, inspection,
artifact and RuntimePolicy interfaces. All provider calls remain HTLK-owned; see
workflow-host.md and ingestion.md for the implemented role/recovery boundary.

## Failure contract

Stable classes: invalid input, unauthenticated, forbidden/not found without leaking
existence, conflict, missing capability, overloaded, processing failed, cancelled,
and dependency unavailable. Include retryability only when actually established.
Never expose raw SQL, local file paths, credentials or source content in ordinary
error strings. Distinguish admission rejection from asynchronous processing failure,
and tool delivery from successful domain outcome.
