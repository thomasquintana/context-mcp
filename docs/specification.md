# context-mcp specification — draft 0.1

This document states the agreed target behavior. Implementation and measured
verification are tracked separately in [acceptance.md](acceptance.md) and the
temporary task plan. A specified capability is not a claim of implementation.

## 1. Deployment and execution boundaries

One Rust application instance serves multiple tenants and concurrent agents.
Bounded in-process workers execute durable jobs. PostgreSQL owns application
records; Typesense is a derived index; S3-compatible storage owns payloads.
HTLK uses its existing local durable SQLite execution store.

The application embeds unchanged HTLK compiler/runtime interfaces. Every MCP
operation, including provider discovery and execution, is dispatched by HTLK.
Operator-managed graphs, exact catalogs, and trusted deployment bindings compose
all server capabilities. The domain has no provider-specific MCP tool names.

The initial required targets are llm-mcp and mcp-sandbox. Both and all future MCP
providers remain replaceable. A replacement may require operator graph/catalog/
binding configuration appropriate to its tools; MCP transport compatibility alone
does not establish equivalent behavior. No sibling/external projects are changed.
Any additional integration mechanism or adaptation requires prior user approval.

External HTLK workflows consume the context server. Internal workflows must use
existing public HTLK facilities for application operations; their concrete host
boundary is validated in the integration milestone, not assumed to require a shim.

## 2. Authentication and resource authorization

HTTP uses provider-neutral OAuth/OIDC access tokens issued for the configured
resource/audience. Validate issuer, signature/key rotation, audience, expiry, and
configured algorithms. Identity is keyed by issuer plus subject. ID tokens are not
API credentials. Discovery endpoints support the chosen MCP authorization profile.

Agents use distinct service accounts and high-entropy scoped credentials, stored
only as hashes, with expiry, rotation and revocation. Stdio binds an explicit
service identity. The host authenticates identity before constructing an HTLK
HostContext; ordinary tool arguments cannot supply trusted identity or grants.

PostgreSQL owns memberships and ACLs. Hierarchy: tenant -> workspace/project ->
collection -> asset. A resource's ACL may inherit ancestor grants or stop
inheritance. Effective authority requires both credential scope and a resource
grant. Membership in the owning tenant alone is insufficient. Scope capabilities
include read, ingest, memory-write, synthesize, share, and purge.

Cross-tenant grants target a principal, tenant, or workspace. The asset retains its
owner and identity; sharing is live access rather than copying. Each read, query,
resource expansion, pointer resolution and derivative publication checks current
authority. Derived content retains all source protections; revocation cannot expose
the inaccessible contribution through a saved summary or memory. Unauthorized
content is excluded before it can affect returned evidence or pointer metadata.

Local development/test uses Keycloak. Production can use another compatible issuer.

## 3. Formats and acquisition

| Category | Starting support matrix |
| --- | --- |
| Text/data | TXT, Markdown, HTML, CSV, JSON |
| Documents | PDF, DOCX, PPTX, XLSX, EPUB |
| Images | JPEG, PNG, WebP, GIF, TIFF |
| Audio | MP3, WAV, M4A, FLAC, Ogg |
| Video | MP4, MOV, WebM, MKV |
| Code | Individual source files and explicitly uploaded repository archives |

Explicit uploads and exact-resource URLs are the acquisition mechanisms. A URL
imports the returned resource only: no crawling or implicit remote subresource
downloads. Refresh and reprocessing are explicit. Embedded bytes within uploaded
documents may be extracted as derived content. Verify media types and bound
archive expansion, file counts, extraction duration, payload size and temporary
storage; repository archives do not cause code execution.

Large upload sessions use authenticated HTTP and direct-to-object-store transfer;
MCP creates/finalizes sessions and manages jobs. Small text and memories can be
written directly through MCP. Hashes and sizes are verified before admission.

## 4. Processing and publication

Each logical asset has immutable source revisions. Citations bind exact revisions.
The current pointer changes only after all required extraction, model processing,
chunking, embedding, and indexing stages succeed. A replacement failure preserves
the previous successful current revision. Historical reads require explicit request.

Configured model and sandbox capabilities are required. Reject admission if the
selected workflow lacks its required capabilities. Accepted work is asynchronous;
provider failures after acceptance cause a failed job and no published new revision.
Progress, cancellation, explicit retry, and idempotency are public contracts.

Operator graphs choose the media processor, model, prompt, embedding space and
schema. Native processing uses the sandbox provider and pinned operator images;
ordinary Rust transformations can run locally through existing host interfaces.
No server implementation invokes another MCP server directly.

Current llm-mcp description/transcription-based multimodal retrieval is sufficient.
Embeddings must carry their model/space identity; incompatible vectors must not be
mixed. Generated enrichment never becomes original evidence by being indexed.

The PostgreSQL job/publication transaction and HTLK run-admission transaction are
distinct. Stable idempotency and reconciliation must bridge interruption without
duplicate published revisions. Publication checks an asset generation fence and job
state under a lock. Physical output writes require equivalent fencing and cleanup;
checking a generation only at final publication does not prevent orphan data.

Sandbox profiles expose only the job's staged files and bounded resources. Validate
actual command exit status as well as MCP success. Cancellation and cleanup must
be verified against the real provider; transport teardown alone does not prove
native process termination or deletion of external objects.

## 5. Retrieval and navigation

Hybrid search combines authorized lexical and compatible semantic retrieval.
Return an object-valued structured context map, not a synthesized answer:

- Contract version and ranked/deduplicated evidence.
- Origin classes and exact source revision/location references.
- Typed pointers for evidence expansion, asset metadata, pages, image regions,
  media intervals, entities and relationships.
- Optional caller token budget, operator-selected accounting scheme, selected token
  count, and truncation indicator.

The budget governs selected evidence, not external model billing. Limit evidence
count and response bytes independently. Result ranking uses a documented common
ordering rather than treating unrelated provider raw scores as comparable.

Pointer expansion reauthorizes against current source permissions. Binary media
is normally returned using short-lived authorized object URLs or bounded resource
contents, not enormous inline MCP responses. Exact revision and source coordinates
are retained across navigation and synthesis.

## 6. Semantic memory and synthesis

Agents explicitly write project-scoped semantic memories with a subject,
description and typed value or natural-language statement. No machine key is
required. Immutable revisions retain authorship, source dependencies, citation-only
support, origin class and supersession relationships. Updates compare an expected
current revision/version to avoid silently overwriting concurrent writes.

Reconciliation can merge equivalent or complementary statements. Contradictions
select a winner only through explicit authorized supersession or configured source
authority. Otherwise a conflicted current view retains the competing revisions.
Model judgment does not invent authority; preserve the reconciliation rationale.

Synthesis is a separate explicitly requested operation with citations to accessible
evidence. Results are transient unless an agent explicitly saves them. Transient
means absent from permanent application knowledge/memory storage; HTLK execution
records and external-provider retention remain outside that promise.

Saved generated artifacts preserve their informational dependencies and original
source protections. Distinguish original evidence, agent assertions and generated
summaries in every relevant result.

## 7. Permanent purge

No recoverable soft-delete state. A durable purge operation immediately closes
admission/read/publication for the target and permanently deletes managed content.
The pending physical-cleanup worklist is operational state, not a hidden retained
content view. Success is reported only after required application cleanup completes.

Scope: assets and revisions, payloads, extracted content, derived media, chunks,
vectors/index documents, dependent saved summaries, source-derived memory revisions
(including history), staged outputs and temporary processing files.

Follow transitive derivation edges across source revisions and sharing boundaries.
A mixed-source artifact is purged in full. Regeneration is an explicit operation
using surviving accessible sources. Independently authored assertions that merely
cite the source survive with unavailable evidence. Ownership controls permission to
purge an original source; dependency removal must not be limited to the initiating
principal's readable subset.

Fence and settle in-flight work so late model/process/index results cannot recreate
content. The cleanup worklist is durable and idempotent across restart. Persist only
minimal content-free operational audit records. Object versioning, temporary caches
and any configured replicas must have an explicit application deletion policy.

HTLK execution history is explicitly out of scope. External MCP servers' histories
follow their own lifecycles. Purging a reference does not claim to erase those copies.

## 8. Quality and delivery

Document public APIs; enforce strict formatting, linting and rustdoc; use meaningful
domain, integration, failure, concurrency and crash-recovery tests. Coverage budgets
must be measured rather than fabricated. Generate dependency notices and enforce
license policy; lock dependencies and validate release packages reproducibly.

Deliver a CLI, stdio and Streamable HTTP support, Compose/deployment instructions,
local Keycloak realm, operator graphs/catalogs/bindings and media profiles/images.
Do not describe fixture-only MCP peers or mocked upstream responses as proof of
actual model/media-provider compatibility. Credentials never belong in fixtures,
catalogs, commits, command strings or ordinary operation arguments.
