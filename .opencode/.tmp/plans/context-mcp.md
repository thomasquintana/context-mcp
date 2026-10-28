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

### T1 — Specification and provider-neutral domain contracts [completed]

- Write README, full product/architecture specification, supported-format matrix,
  API/workflow contracts, and explicit acceptance criteria.
- Establish Rust workspace, license, lint/toolchain policy, and focused domain
  crates. Keep dependency versions aligned with actual available tooling.
- Model IDs, hierarchy, ACLs/sharing, revisions, jobs, memories/conflicts, provenance,
  context maps/pointers, publication and purge lifecycle without provider-specific
  tool names.
- Verify formatting, domain invariants, compilation, and public documentation.
- Update plan and commit.

### T2 — Existing-HTLK integration spike [in progress]

- T2a — Existing-interface host, exact discovery and metadata invocation [completed].
- T2b — Real media/model pipeline and lifecycle validation [blocked on environment].

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
  changes. Commit: `4ec3963` (Record implementation plan and architecture boundaries).
- T1: added Apache-2.0 license, pinned Rust workspace, provider-neutral context-core
  contracts and facade, README/contribution guidance, full specification, logical
  API contracts and acceptance/evidence matrix. Domain tests exercise scope/ACL
  intersection, cross-tenant inheritance, wrong-target/cycle denial, terminal-job
  immutability, publication fences and mixed-source transitive purge with retained
  independent citations. No provider or service is claimed implemented.
- T1 verification: cargo fmt --all --check; cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings; cargo test --workspace --all-features
  --locked (5 passed); RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features
  --no-deps --locked; cargo package --workspace --allow-dirty --locked --target-dir
  target/package-t1 (both packages built from their archives). Commit `2534c25`.
- T2 research: existing HTLK NativeMcpAdapter supports the necessary stdio/HTTP
  calls without a custom adapter. Pin inspected HTLK revision
  3448acb5dc7cebef4d08ef37bb79757fea780659. Actual providers inspected at llm-mcp
  a9b6d2fe3fed686dd2ca2fb67591613f0cf44cb8 and mcp-sandbox
  8c649a415ba7be6aa6e15a4dc51e56dc5990eae0. These are research observations,
  not executed-provider verification.
- T2a: added context-workflows operator-run host using existing NativeMcpAdapter,
  Compiler, Runtime and SqliteStore APIs. Explicit discovery preserves full exact
  descriptors. No custom adapter, shim or external-project edit. Downloaded official
  sandbox v0.2.0 Darwin arm64 to ignored target/providers and verified archive SHA256
  3c48cd9ff8980b4cd81210e7b9559e4ba0ad0f7e8331c3121ed566ac191efda0.
- T2a real-provider evidence: HTLK discovery succeeded for llm-mcp's four model
  tools and sandbox's five selected tools. Compiled/admitted sandbox_metadata.htlk
  returned profiles=[] and sessions=[] against actual mcp-sandbox; fingerprint
  sha256:0aec62b60537ac88ec3be74135eda529e9c62125afb35ef691d054210cb42862.
  Artifacts: target/providers/catalogs.json and sandbox-metadata-result.json. No
  upstream model call or container was invoked; those capabilities remain unproved.
- T2a checks: fmt, strict workspace/all-target Clippy, all workspace tests (8 passed,
  including public-runtime replay/admission and typed-input rejection), strict
  rustdoc. Added docs/integration-spike.md with exact reproduction and scope.
- T2b environment blockers: no OPENAI_API_KEY configured. llm-mcp auth status reports
  ChatGPT sign-in, but OAuth mode lacks embedding/transcription. Do not access or
  copy credential values. Need an existing configured model endpoint or provider
  access for required capabilities. Docker daemon responds, but docker pull of
  debian:bookworm-slim failed after 120s in credential helper; no media image exists.
  Ask user to provision dependencies/direction before using a workaround. The
  integration phase and entire implementation are not marked complete.

## Resume checkpoint

Current task: T2b. Next: commit completed T2a work, then ask for configured live
model-provider access and a functioning Docker image pull (or existing usable media
image). Resume real sandbox/media/model validation using existing interfaces. Do
not add adaptations or alter siblings. Preserve this plan while tasks remain;
cleanup/final push happen only after full verification.
