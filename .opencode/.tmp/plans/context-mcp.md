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
- User accepted the reference sandbox's reported destroy success despite observed
  surviving containers. Use saved IDs and existing HTLK destroy calls; provider
  container lifetime is the provider's responsibility. Preserve the bug evidence.
  No watchdog, TTL wrapper, Docker cleanup bypass or sibling change is approved or
  needed for this project. Application-managed staging/content still gets purged.

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

### T2 — Existing-HTLK provider integration spike [completed]

- T2a — Existing-interface host, exact discovery and metadata invocation [completed].
- T2b-media — Actual FFmpeg/frame/audio extraction and normal destruction [completed].
- T2b-model — Live model calls and independent value checks [completed after account activation].
- T2c — Actual provider restart/cleanup probe [completed; limitation accepted by user].

- Resolve dependency using unchanged public HTLK APIs and existing graph/catalog/
  deployment configuration. Record the exact compatible versions/descriptors.
- Compile/admit/run a real graph against the initial providers. Exercise input
  staging, sandbox media work and model processing. Full application publication,
  retrieval and purge verification belongs to T4/T5 once the service exists.
- Verify protocol/output compatibility, bounded payloads, session continuity,
  cancellation/recovery, nonzero process exit handling, and cleanup behavior.
- Inspect existing implementations before promising guarantees: sandbox session
  registry is process-local; Docker exec deadlines may not terminate the process;
  cleanup after restart may be unsupported. Report a demonstrated incompatibility
  and ask before any adaptation. HTLK history erasure is not required.
- Unit/mock tests are supplementary, not substitutes for the real-provider spike.
- Update plan and commit successful work, or record an explicit blocker.

### T3 — Service foundation and authoritative persistence [in progress]

- T3a — PostgreSQL schema/repositories and S3/Typesense clients [completed].
  - T3a1 PostgreSQL schema and transactional primitives [completed].
  - T3a2 S3/Typesense clients and live verification [completed].
- T3b — Shared production HTLK host and role contracts [completed].
- T3c — Authentication, authorization, transport and durable jobs [in progress].
  - T3c1 Provider-neutral authentication and transactional ACL decisions [completed].
  - T3c2 MCP/HTTP/stdio interfaces, uploads and worker recovery [in progress].
    - T3c2a Authenticated transports, operator CLI and resource services [completed].
    - T3c2b Upload admission, job state and durable processing [in progress].

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
  Commit: `91a7582` (Validate existing HTLK interfaces against initial MCP providers).
- T2b environment blockers: no OPENAI_API_KEY configured. llm-mcp auth status reports
  ChatGPT sign-in, but OAuth mode lacks embedding/transcription. Do not access or
  copy credential values. Need an existing configured model endpoint or provider
  access for required capabilities. Docker daemon responds, but docker pull of
  debian:bookworm-slim failed after 120s in credential helper; no media image exists.
  Ask user to provision dependencies/direction before using a workaround. The
  integration phase and entire implementation are not marked complete.
- Resume: user supplied llm-mcp/.env and authorized execution. Source the file only
  into the launch environment; never print/copy credential values. Model endpoints
  inherit OPENAI_API_KEY by name and explicitly use api_key mode. Upstream host
  checked as api.openai.com over HTTPS. No external project changed.
- T2b-media: Docker pull now succeeds. Built deploy/media/Dockerfile from pinned
  Debian; local image identity
  sha256:121898f939e1cc1d784fb6c09a73159c09f84b8a1379efa1cf7516167edade89.
  tools/live_provider_spike.py --skip-image-build --media-only passed against real
  mcp-sandbox via compiled HTLK graph: blue 64x64 video, extracted PNG and mono
  16kHz WAV, expected ffprobe streams, native exit0, actual container absent after
  explicit destroy. Evidence target/providers/live-6uzuck7r/evidence.json.
- T2b-model: actual llm-mcp generation (gpt-4.1-mini), embedding
  (text-embedding-3-small), and transcription (whisper-1) each returned HTTP429.
  A later isolated tiny embedding also returned429 with request ID
  req_eb8d6e0b2fd44c45b6f6ed9c9854e770. Four rejected calls, no successful model
  output. Error bodies deliberately unavailable; do not assert quota vs transient
  throttling. User asked about missing usage; explained rejected requests and
  verified actual OpenAI host and live credential inheritance (not fake discovery
  config). Stop repeated calls until account/project access is clarified.
- T2c actual recovery evidence: create-only HTLK run ends; container survives.
  Fresh provider session lists []; destroy(saved ID) returns destroyed:1 but
  container remains. tools/check_sandbox_lifecycle.py recorded evidence under
  target/providers/live-apebe2qm/lifecycle-evidence.json. Removed only exact probe-
  owned container during test teardown; no remaining test containers. This is NOT
  a production cleanup workaround. Existing provider APIs cannot establish the
  restart-safe cleanup requirement. Ask user before adding any lifetime/recovery
  mechanism; no sibling modifications permitted.
- Resume verification: fmt, strict all-target Clippy, workspace tests (8 passed),
  strict rustdoc, Python compilation; real media success and actual recovery gap
  recorded in docs/integration-spike.md. Commit these completed investigations.
- User confirmed account activation and explicitly requested one retry. A fresh
  admission key succeeded: text-embedding-3-small, vector256, 4 input/total tokens.
  Evidence target/providers/embedding-approved-retry-result.json. Account blocker
  is resolved; historical HTTP429 evidence is retained rather than overwritten.
- Follow-on actual model graph initially exceeded our host's 100k expression-step
  allowance validating the 65KB audio JSON before run admission (no provider call).
  Raised only existing host-owned policy to4M steps; no HTLK mechanism/change.
  Added public-runtime regression covering 70KB JSON through validation/artifacts.
- Actual model graph then succeeded: embeddings returned two ordered, distinct,
  finite vector256 values (10 input/total tokens); whisper-1 recognized synthetic
  "The ocean is blue.". gpt-4.1-mini returned White for the known blue frame;
  semantic test correctly failed. Isolated gpt-4.1 call correctly identified blue
  (274 input, 2 output, 276 total tokens). No tests weakened. Fixture model default
  now verified gpt-4.1, still operator configurable.
- Evidence target/providers/live-tx3h_arn/model-result.json;
  target/providers/generation-knbb8pin/{result,evidence}.json;
  live-tx3h_arn/verified-model-evidence.json. Verification explicitly combines two
  model runs; not a claim of full server publication/retrieval. No more model calls
  needed for these checks unless a new change/failure justifies them.
- Final checkpoint checks: cargo fmt --all --check; strict workspace/all-target
  Clippy; all workspace tests (9 passed); strict rustdoc; Python syntax compilation
  for all three harnesses. Sibling Git working trees clean; llm-mcp/.env ignored.
  Task commit subject: Verify live media and model workflows through unchanged HTLK.
- User decision: accepts successful destroy(saved ID) receipts, treats surviving
  containers as a replaceable provider bug, and does not want an extra recovery/TTL
  mechanism. Preserve evidence; proceed with existing protocols. T2 completed at
  provider-integration scope; production application E2E remains explicitly T4/T5.
  Prior live-verification commit: 2f85d68.
- T3a1: context-storage SQLx0.9 PostgreSQL schema and repositories added. Resources
  enforce immutable ancestry/ownership, revisions are immutable, generation and
  purging-ancestor checks guard publication. Scope/grant hydration, serialized
  idempotency receipt admission and concurrent job claiming implemented. Fixed
  login-role/application-schema migration search_path collision during actual tests.
- T3a1 verification: 5 actual PostgreSQL tests passed on fresh database
  context_authority using disposable local container context-mcp-test-postgres,
  localhost55432, PG16 image9e2cf628... (full digest in docs/persistence.md).
  fmt, strict workspace all-target Clippy, normal workspace tests (9 passed plus
  5 explicitly ignored backing-service tests), strict rustdoc all passed. No
  sibling changes. Storage primitives do not claim completed request auth/service.
- T3a1 commit: d3592aa. T3a2 implemented S3-compatible bounded reads, create-only
  writes, streaming size/SHA256 verification, short-lived signed URLs and repeated
  deletes; Typesense schema/dimension validation, deterministic upsert, finite
  vectors, authorized-ID hybrid queries and repeated deletion. Actual MinIO and
  Typesense tests passed. Metadata/credentials stay out of ordinary errors.
- T3a2 test infrastructure: local context-mcp-test-minio on59000 uses cached MinIO
  DEVELOPMENT.GOGET image d84beaf... (full provenance in docs/persistence.md).
  Public MinIO registry tags were unavailable; no endpoint/schema adaptation added.
  Typesense29.1 context-mcp-test-typesense on58108 uses tmpfs/data and pinned image.
  Disposable test credentials are public fixtures, not user secrets. Tests create
  random S3 buckets and Typesense collections and delete them after verification.
- T3a2 commit:37ab660. Strict fmt/Clippy/docs and unit suite passed; all seven actual
  storage tests (5PG+2MinIO/Typesense) passed in one explicit backing-service run.
- T3b: context-workflows::host added one long-lived registry/store/runtime/engine,
  retained operator role graphs with required request/json -> result/json object
  contract, exact selected-binding readiness checks, required deployment validation,
  authenticated JobGrant provenance labels, graph/binding-scoped authorization,
  revocation, saved run handles, typed state/accepted outputs and bounded shutdown.
  Uses existing public HTLK policies/interfaces; no adapter or sibling changes.
- T3b verification: five normal production-host tests cover cross-job/forged-handle
  denial, ungranted role aliases, revocation, durable reopen/idempotency, invalid
  ports/providers, scalar result rejection. Actual sandbox metadata/readiness also
  passed under the production policy. Total normal workspace tests14 passed;
  fmt, strict all-target Clippy and strict rustdoc passed. See docs/workflow-host.md.
- T3b commit085f276. T3c1 code now includes Authentication (non-deserializable host
  facts), context-auth JWT/JWKS +RFC7662 discovery/verification, bounded key caching
  and rotation, random256bit scoped agent credentials stored only asSHA256, owner
  revocation and finite expiry. AuthorizedTransaction keeps membership/grants,
  service-token status/scopes and protected reads in one snapshot; write mode is
  serializable and locked. No JWT tenant claim becomes a grant.
- T3c1 actual tests: PostgreSQL suite7 passes including a never-coexistent
  membership/new-grant race and revoked/expired/scoped credentials at read time.
  Actual Keycloak26.8.1 on58080 JWT, ID-token/wrong-audience/tamper rejection,
  introspection and real signing-key rotation passed. Local realm fixture required
  standard subject and separate API+introspector audience mappers (no relaxed
  validation/no external code change). Auth unit2 and credentialDB test pass.
  See docs/authentication.md. Fixture contains only public development values.
- T3c1 final checks: fmt, strict all-target Clippy, normal workspace tests16 passed,
  strict rustdoc and diagnostic Python syntax passed. External tests remain explicit:
  7PG+2S3/Typesense+2auth/Keycloak+1native-host. The two auth tests (including actual
  RSA signing-key rotation and cached refresh) passed; no fixture-only proof claim.
- T3c1 commitd6d8971. T3c2a added context-engine and actual root server binary with
  MCP2025-11-25 HTTP/stdio transports, per-operation authentication, RFC9728
  metadata/challenge, host/origin/CORS/body/deadline checks and bounded shutdown.
  Current tools: context_identity, context_resource, context_create_collection.
  CLI: serve/catalog/migrate/bootstrap/issue-credential (private create-only output,
  revoke on file-write error). Operator-role example references both initial
  providers. No data ingestion/retrieval/memory completion claim yet.
- Migration0002 adds resource labels, memory subject uniqueness and authoritative
  workflow/stage records. Application restores job/principal/scope-owner checked
  grants before starting the shared runtime. Collection mutation retries only pure
  DB serialization/deadlock conflicts before external effects.
- T3c2a actual binary test passed: HTLK-compiled clients over HTTP and stdio verify
  authenticated identity, resource isolation, missing-auth challenge/metadata,
  scope limits, revoked credential rejection, sequential/concurrent idempotency
  with one DB row. Fixture application roles are pure and not provider proof.
  fmt, strict Clippy, ordinary workspace tests16 and strict rustdoc passed. Actual
  PostgreSQL wrong-resource-kind/immutable-parent check passed. See
  docs/server-foundation.md. Source/env credentials remain untracked.

## Resume checkpoint

Current task: T3c2b. Next: commit the verified transport foundation, then implement
S3 upload sessions/finalization, immutable source handling, job admission/status/
cancellation and durable staged workers. Full media roles, retrieval/memory/
synthesis/purge follow in T4. Do not expose unimplemented tools or placeholder
successes. Preserve exact source dependencies and generation fences.
Use the shared production host (not spike fixed policy) and authenticate every
HTTP/stdio operation. Serializable admission retries are DB-only before effects.
Provider orphan limitation is accepted; no adaptation needed. Preserve this plan;
cleanup/final push happen only after full verification.
