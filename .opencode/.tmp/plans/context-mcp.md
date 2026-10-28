# context-mcp implementation plan

## Session protocol

- Read this plan before starting work, after every compaction, and whenever the
  next action or a requirement is uncertain.
- Update this file after each completed task with its implementation, verification,
  commit, and remaining work. Keep exactly one task active.
- Commit intended changes after every completed task. Inspect status, diff, and
  recent history before committing. Never commit credentials or generated secrets.
- When every task and its verification is complete, remove this temporary plan,
  commit the cleanup, and push all completed commits to `origin`.
- Do not mark partial implementations, mock integrations, or unverified guarantees
  complete. Record blockers and obtain the user's direction when required.

## Authoritative decisions

- Build context-mcp in Rust, Apache-2.0, using the engineering practices of htlk.
- One application instance; bounded concurrent background processing. No distributed
  application, execution partitions, or horizontal worker architecture.
- PostgreSQL is the authoritative application database; Typesense holds rebuildable
  search documents/vectors; MinIO/S3 holds original and derived content.
- Multi-tenant: tenant -> workspace/project -> collection -> asset. Individual asset
  ACLs inherit container permissions. Agents share project knowledge and memories.
- Explicit cross-tenant grants expose live assets; ownership remains with the source
  tenant. Authorize evidence, pointers, reads, derivatives, and sharing operations.
- Provider-neutral OAuth/OIDC for humans; scoped, revocable service-account
  credentials for agents. Development/test issuer: local Keycloak. Production does
  not require Keycloak. Explicit service identity for stdio.
- Streamable HTTP and stdio MCP transports; authenticated uploads including large
  object-storage upload sessions; CLI using the same application contracts.
- Every MCP operation is dispatched by HTLK. Operator-managed HTLK graphs compose
  capabilities. No MCP server directly calls another MCP server.
- All MCP providers are replaceable. llm-mcp and mcp-sandbox are the initial required
  integration targets. Provider tool names/schemas belong in admitted graphs/catalogs
  and deployment bindings, not in domain logic.
- HTLK compiler/runtime are embedded for ingestion/enrichment; external HTLK
  workflows can consume context-mcp. Use existing public HTLK interfaces.
- No changes to HTLK or any external/sibling project. Before adding any adaptation,
  custom adapter, compatibility shim, or additional mechanism, explain the concrete
  requirement, why existing mechanisms are insufficient, options/tradeoffs, and ask
  the user. Do not silently introduce a workaround.
- Accept explicit uploads and exact-resource URLs only. No crawling, implicit remote
  media downloads from HTML, automatic refresh, or automatic regeneration.
- Initial formats: TXT, Markdown, HTML, CSV, JSON; PDF, DOCX, PPTX, XLSX, EPUB;
  JPEG, PNG, WebP, GIF, TIFF; MP3, WAV, M4A, FLAC, Ogg; MP4, MOV, WebM, MKV;
  individual source-code files and explicitly uploaded repository archives.
- Rust processing libraries are preferred; native helpers such as FFmpeg execute
  through the configured sandbox workflow. Existing llm-mcp description/transcript
  based multimodal retrieval is sufficient; native cross-modal vectors not required.
- Ingestion rejects missing required capabilities. Accepted jobs are durable and
  asynchronous. All required stages must succeed before a revision becomes
  searchable. Preserve the previous successful revision during replacement.
- Stable logical assets, immutable source revisions, revision-bound chunks and
  citations; current successful revision searched by default; explicit historical
  reads, refresh, retry, cancellation, and reprocessing.
- Search returns bounded structured context maps: evidence, provenance, source
  locations, entities/relationships, and typed pointers. Optional token budget;
  rank/deduplicate evidence. Expand chunks, document pages, image regions, clips.
- Agents write semantic memories with subject, description, typed value/text,
  authorship, source references, immutable revisions, and a current view. No required
  machine key. Merge equivalent/complementary statements; resolve contradictions
  only using explicit supersession/authority rules; preserve unresolved conflicts.
- Synthesis is an explicit cited operation; transient unless explicitly saved.
  Distinguish original evidence, agent assertions, and generated summaries.
- Permanent purge covers context-mcp-managed assets/revisions, indexes/embeddings,
  extracted and generated content, dependent summaries/source-derived memory
  revisions including history, staging, and temporary processing files.
- Follow transitive derivation dependencies. Purge mixed-source artifacts in full;
  regenerate only explicitly from surviving sources. Independent assertions that
  merely cite a source survive with unavailable supporting evidence.
- HTLK execution history is explicitly outside context-mcp purge scope. Keep minimal
  content-free application purge/audit records. Fence late processing/publication so
  deleted data cannot be recreated.

## Repository prerequisites

- Working directory initially empty and not a Git checkout.
- Existing GitHub repository verified: https://github.com/thomasquintana/context-mcp
  (empty, current user ADMIN). Initialize local main and configure that origin.
- Do not modify Git configuration other than establishing the repository remote.

## Tasks and commit boundaries

### T0 — Save plan and initialize repository [completed]

- Save this file before implementation.
- Initialize main, configure existing origin, and add session guidance.
- Verify remote and commit this planning checkpoint.

### T1 — Specification and provider-neutral domain contracts [pending]

- Write README, full product/architecture specification, supported-format matrix,
  API/workflow contracts, and explicit acceptance criteria.
- Establish Rust workspace, license, lint/toolchain policy, and focused domain
  crates. Keep dependency versions aligned with actual available tooling.
- Model IDs, hierarchy, ACLs/sharing, revisions, jobs, memories/conflicts, provenance,
  context maps/pointers, publication and purge lifecycle without provider-specific
  tool names.
- Verify formatting, domain invariants, compilation, and public documentation.
- Update plan and commit.

### T2 — Existing-HTLK integration spike [pending]

- Resolve dependency using unchanged public HTLK APIs and existing graph/catalog/
  deployment configuration. Record the exact compatible versions/descriptors.
- Compile/admit/run a real graph against the initial providers. Exercise input
  staging, sandbox media work, model processing, publication, and retrieval.
- Verify protocol/output compatibility, bounded payloads, session continuity,
  cancellation/recovery, nonzero process exit handling, and cleanup behavior.
- Inspect existing implementations before promising guarantees: sandbox session
  registry is process-local; Docker exec deadlines may not terminate the process;
  cleanup after restart may be unsupported. Report a demonstrated incompatibility
  and ask before any adaptation. HTLK history erasure is not required.
- Unit/mock tests are supplementary, not substitutes for the real-provider spike.
- Update plan and commit successful work, or record an explicit blocker.

### T3 — Service foundation and authoritative persistence [pending]

- Implement migrations and PostgreSQL repositories, S3/MinIO and Typesense adapters.
- Implement configured token issuer/audience validation, stable issuer/subject
  identities, scoped agent credentials, tenant/resource authorization and grants.
- Add HTTP/stdio MCP surfaces and explicit stdio identity, upload sessions, CLI,
  durable admission, bounded workers, idempotency, revision publication and recovery.
- Verify actual database/storage interactions and tenant boundary cases.
- Update plan and commit independently verified sub-tasks as appropriate.

### T4 — Multimodal processing and context services [pending]

- Implement complete agreed-format workflows through replaceable graph bindings.
- Implement chunking/enrichment/indexing, hybrid retrieval, context budgets,
  navigation/entities and page/region/clip extraction.
- Implement revisioned semantic memories/reconciliation, explicit cited synthesis,
  transient/saved generation distinctions, and source lineage.
- Implement permission-aware transitive purge with publication fencing and
  restart-safe deletion of app-managed payloads/staging. No soft deletion.
- Verify independent values, access boundaries, failure and concurrency cases.
- Update plan and commit independently verified sub-tasks as appropriate.

### T5 — Delivery and full verification [pending]

- Compose (including local Keycloak), pinned processing image/profiles, container
  deployment, CLI, operator/user docs, HTLK catalogs and runnable examples.
- CI/release checks: fmt, strict Clippy, tests, strict rustdoc, package checks,
  dependency/license policy, notices, meaningful coverage enforcement.
- Actual end-to-end verification on both transports, initial providers, isolation,
  sharing/revocation, concurrent memory updates, cancellation/crash recovery,
  interrupted publication, index rebuilding, and purge races.
- Document measured results and genuine remaining blockers; do not claim success
  from tests against only fixtures.
- Update plan and commit.

### T6 — Final cleanup and push [pending]

- Review status, complete diff/history, tests, and tracked files for secrets.
- Ensure every acceptance criterion has evidence and no unresolved blocker remains.
- Remove this temporary plan; change session guidance to permanent docs.
- Commit final cleanup and push all commits to origin/main. Return commit/test/push
  summary and any limitations accurately.

## Progress and verification log

- Planning: requirements discovered interactively; no implementation existed.
- Read-only research: sibling HTLK, llm-mcp, mcp-sandbox plus plug/funnel references.
- Initial repository check: empty local directory; existing empty origin verified.
- T0: saved plan and root AGENTS guidance; initialized main with existing GitHub
  origin. Rust/cargo 1.98.0 available; Docker Desktop daemon reachable. No sibling
  changes. Planning checkpoint commit follows this update.

## Resume checkpoint

Current task: T1. Next: write specification/domain contracts and research existing
HTLK integration examples before adding any integration mechanism. T0 files are
ready for the planning-checkpoint commit.
