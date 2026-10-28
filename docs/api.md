# Application and workflow contracts — draft 0.1

These are target contracts, not a declaration that the service endpoints exist.
Public MCP tools return object-valued `structuredContent`, publish input/output
schemas, and expose redacted typed failures. HTTP/CLI use the same application
services and authorization rules.

Implemented foundation tools: `context_identity`, `context_resource`, and
`context_create_collection`. Their exact descriptors are exported by `context-mcp
catalog`. The remaining contracts below are target capabilities pending their
application milestones. See server-foundation.md for executable setup and evidence.

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
publish revisions. The integration spike must choose an existing HTLK host boundary
(for example its public typed input/reply facility or ordinary application MCP
operations), without assuming a custom adapter. All provider calls remain HTLK-owned.

## Failure contract

Stable classes: invalid input, unauthenticated, forbidden/not found without leaking
existence, conflict, missing capability, overloaded, processing failed, cancelled,
and dependency unavailable. Include retryability only when actually established.
Never expose raw SQL, local file paths, credentials or source content in ordinary
error strings. Distinguish admission rejection from asynchronous processing failure,
and tool delivery from successful domain outcome.
