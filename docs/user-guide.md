# context-mcp user guide

context-mcp gives an assistant a durable, permission-aware place to work with
documents, images, recordings, source code, project facts, and cited answers.
You add material explicitly, wait for it to be processed, and retrieve evidence
that points back to exact source revisions. You can also write versioned semantic
memories and explicitly save generated answers.

This handbook covers the implemented 0.1 application. It is written for people
using an MCP-enabled assistant, people using the command-line client, and operators
running a server. You do not need to understand Rust or HTLK to use an existing
installation. Operators need familiarity with containers, credentials, and storage.

## Contents

1. [Choose your starting point](#1-choose-your-starting-point)
2. [Understand the essentials](#2-understand-the-essentials)
3. [Connect to an existing server](#3-connect-to-an-existing-server)
4. [Complete your first workflow](#4-complete-your-first-workflow)
5. [Add files, text, and URLs](#5-add-files-text-and-urls)
6. [Monitor, cancel, and retry processing](#6-monitor-cancel-and-retry-processing)
7. [Search and explore evidence](#7-search-and-explore-evidence)
8. [Use revision history and update sources](#8-use-revision-history-and-update-sources)
9. [Work with documents, images, audio, and video](#9-work-with-documents-images-audio-and-video)
10. [Create and reconcile semantic memories](#10-create-and-reconcile-semantic-memories)
11. [Generate and save cited answers](#11-generate-and-save-cited-answers)
12. [Share material and revoke access](#12-share-material-and-revoke-access)
13. [Permanently purge material](#13-permanently-purge-material)
14. [Install your own server](#14-install-your-own-server)
15. [Operate and maintain an installation](#15-operate-and-maintain-an-installation)
16. [Troubleshoot common problems](#16-troubleshoot-common-problems)
17. [MCP tool reference](#17-mcp-tool-reference)
18. [CLI reference](#18-cli-reference)
19. [Resources, pointers, permissions, and limits](#19-resources-pointers-permissions-and-limits)
20. [Glossary and further reading](#20-glossary-and-further-reading)

## 1. Choose your starting point

**Your organization already runs context-mcp:** obtain the connection details and
starting resource IDs described in [section 3](#3-connect-to-an-existing-server),
then follow [the first workflow](#4-complete-your-first-workflow).

**You want to run it locally:** start with [installation](#14-install-your-own-server).
The reference deployment builds the application and launches PostgreSQL, MinIO,
Typesense, and optionally development Keycloak. Model and native-media processing
are performed through replaceable MCP providers.

**You already have content and need a particular task:** use the contents above.
The [tool reference](#17-mcp-tool-reference) lists exact inputs; the walkthroughs
explain how to combine them.

### What you can ask an assistant to do

Once its MCP connection is configured, requests can look like:

> Add these research notes to my collection. Wait for processing to finish, then
> find the evidence about ocean color and show the original source references.

> Record our approved release date as project memory. If it contradicts an existing
> assertion, show the conflict instead of choosing a winner.

> Answer this question using the selected evidence, with citations. Keep the answer
> transient; do not save it as project knowledge.

These are example instructions to your assistant, not built-in prompts or a special
command language. Client behavior depends on the assistant. Check the actual tool
results, particularly job completion, conflict state, and whether an answer was saved.

### What this service provides

- Explicit file/text/exact-URL ingestion and durable asynchronous processing.
- Hybrid retrieval with bounded evidence, source coordinates, and navigation.
- Immutable asset revisions and versioned semantic memory.
- Separate cited synthesis, transient by default and saved only when requested.
- Live sharing and source-dependent authorization.
- Irreversible, transitive purge of application-managed material.
- Authenticated Streamable HTTP, stdio, and an HTTP command-line client.

There is no bundled graphical user interface, web crawler, or automatic monitoring
of local folders or remote URLs. Your assistant supplies the conversational UI;
the CLI supplies a direct way to invoke the same operations.

## 2. Understand the essentials

### How your material is organized

```text
Tenant                         ownership boundary
└── Workspace                  project and semantic-memory scope
    └── Collection             group of assets
        └── Asset              logical document, image, recording, or archive
            ├── Revision A     immutable source bytes and processing results
            └── Revision B     another immutable revision; may become current
```

A tenant can contain multiple workspaces; a workspace can contain multiple
collections. The diagram shows one branch for clarity. Memories and saved synthesis
are workspace-scoped knowledge records, not uploaded assets in a collection.

An **asset** is the continuing identity of a source. Its **revision** identifies
the exact source bytes. A successful processing run publishes evidence for a
revision and updates the current pointer. If a replacement fails, the last
successful revision stays current. Failed and unpublished revisions do not appear
in ordinary published history.

The service also creates **content records**: searchable chunks, semantic-memory
revisions, generated summaries, entity information, and selected binary media.
An evidence ID returned by a query is a content ID.

### Retrieval, memory, and synthesis are separate

| You want to… | Use… | Result |
| --- | --- | --- |
| Store a document or recording | Ingestion | Asset/revision/job receipt, followed by published evidence |
| Find relevant evidence | `context_query` | A structured ContextMap, not a prose answer |
| Inspect an exact piece of evidence | `context_expand` or a resource read | Protected source/content details |
| Record a project assertion | `context_write_memory` | Immutable memory revision and current/conflicted view |
| Propose a reconciled assertion | `context_reconcile_memory` | New memory revision, with authority/conflict rules preserved |
| Get an answer with citations | `context_synthesize` | Transient generated answer unless explicitly saved |

### Keep these identifiers distinct

All IDs below are UUID strings. Their similar appearance does not make them
interchangeable.

| Value | Where you get it | What uses it |
| --- | --- | --- |
| Principal | Operator provisioning or `context_identity` | Identity, membership, sharing, credential ownership |
| Workspace / collection | Operator or collection creation | Scope for ingestion, querying, memory, synthesis |
| Asset | Ingestion receipt | Source metadata, history, replacement, purge |
| Asset revision | Ingestion receipt, history, source reference | Exact citations, expansion, media selection, reprocess precondition |
| Content | Query evidence `id`, memory write `content`, media result | Evidence expansion, synthesis inputs, managed-content dependencies |
| Job | Ingestion/media/retry receipt | Status, cancellation, eligible retry |
| Memory | Memory write/read | Logical subject and its history |
| Memory revision | Memory write `revision` or view | Explicit supersession; not a synthesis evidence ID |
| Purge | Purge admission receipt | Polling deletion completion |
| ACL version | `context_resource` | `expected_acl_version` when replacing access rules |
| Asset generation | Asset `context_resource` | Purge precondition; not the asset revision UUID |
| Memory version | Memory write/read | Expected update version; also memory purge precondition |

### Permissions apply at every step

Your credential has a set of **scopes**, such as `read` and `ingest`. Resource ACLs
grant corresponding permissions. You need both. Tenant/workspace membership is
separate; membership alone does not grant access.

Derived knowledge keeps the protections of all informational sources. Sharing a
summary's destination workspace does not make inaccessible source material public.
Later query, expansion, memory reads, and publication recheck authority.

### How examples are written

- Shell examples use Bash/zsh and start from the repository root unless stated
  otherwise. Use an absolute binary path when working elsewhere.
- `<workspace-id>`, `<job-id>`, and similar strings are **placeholders**. Replace
  them with returned UUIDs before sending JSON. Numeric expected versions are
  examples too; use the actual version you just read.
- Most JSON blocks show a tool's **arguments only**. The heading names the tool.
  Send them through your MCP client or `context-mcp call --tool NAME`.
- A full MCP `tools/call` parameter object wraps those arguments as
  `{"name":"context_query","arguments":{...}}`; it is not a batch operation.
- Example responses are illustrative application objects. MCP puts successful
  tool data in `structuredContent`. HTLK client graphs project application data
  from `McpToolResult.data`; the supplied CLI prints the application object.

## 3. Connect to an existing server

### Obtain your connection details

Ask your operator for:

1. The MCP endpoint, usually `https://context.example/mcp`.
2. Your authentication method: a scoped service credential or an OIDC access token.
3. A workspace ID and an existing collection ID, or permission to create a collection.
4. Available processing profiles, embedding spaces, upload limits, and allowed URL hosts.
5. The permissions you have and who can change them.

There is no public tool that lists every tenant, workspace, collection, or asset.
`context_identity` identifies you, not your complete resource inventory.
`resources/list` is intentionally empty; resource templates and returned IDs support
navigation. Keep useful IDs in your project notes without copying secrets into them.

### Connect an MCP-enabled assistant over HTTP

Configure your client's **Streamable HTTP** connection using the supplied `/mcp`
URL and its secure bearer-token mechanism. Protected requests require:

```text
Authorization: Bearer <access-token-or-service-credential>
```

The implemented MCP protocol version is **2025-11-25**. HTTP is stateless; each
request is authenticated. A prior protocol handshake is not a continuing identity
grant. Browser clients also need an operator-approved origin.

Client configuration syntax is client-specific. Use its documented secret or
environment-reference support; do not assume `${VARIABLE}` interpolation works in
every client's JSON, and do not commit a literal token.

For an HTLK deployment binding, the supported endpoint pattern is:

```json
{
  "transport": "streamable_http",
  "url": "https://context.example/mcp",
  "headers": {},
  "header_environment": {
    "Authorization": "CONTEXT_AUTHORIZATION"
  }
}
```

The launch environment must contain the **complete** header value:

```sh
export CONTEXT_AUTHORIZATION="Bearer $CONTEXT_TOKEN"
```

Public connection metadata is available at
`/.well-known/oauth-protected-resource/mcp`. An unauthenticated MCP request receives
a Bearer challenge pointing to that metadata.

### Human authentication

Human clients obtain an **API access token** from the configured OAuth/OIDC issuer.
An ID token is not an API credential. The token must have the configured issuer,
audience, expiry, and mapped scopes. Valid authentication resolves a principal but
does not automatically grant resources or tenant membership.

context-mcp has no `login` command or browser callback UI. Your MCP client and issuer
handle the login flow. The reference Keycloak realm is for development; production
can use another compatible issuer with explicit operator configuration.

### Use the CLI

The repository supplies `context-mcp call` for any public tool and `upload` for
files. If you do not already have the binary, build it with the pinned Rust toolchain:

```sh
cargo build --workspace --all-features --locked
export CONTEXT_MCP="$PWD/target/debug/context-mcp"
export CONTEXT_ENDPOINT="https://context.example/mcp"
```

Load the credential from your private file, without putting it in a command argument:

```sh
export CONTEXT_TOKEN="$(< "$HOME/.config/context-mcp/agent-token")"
```

That file must already have been provided securely. An OIDC access token can be
supplied in the same environment variable. The binary does not automatically load
`.env` files. Avoid shell tracing or environment dumps while handling credentials.

Check the connection:

```sh
"$CONTEXT_MCP" call \
  --endpoint "$CONTEXT_ENDPOINT" \
  --tool context_identity <<'JSON'
{}
JSON
```

The response contains `principal`, `scopes`, and `credential` (which can be null).
This confirms identity and scope, not access to a particular workspace.

For the local reference server, use `http://localhost:8089/mcp` and add
`--allow-http` to each `call` or `upload` command. Plain HTTP requires that explicit
opt-in; reserve it for deliberately trusted development connections.

To make repeated commands shorter, an optional shell helper is:

```sh
context_call() {
  "$CONTEXT_MCP" call --endpoint "$CONTEXT_ENDPOINT" --tool "$@"
}
```

It accepts the tool name followed by any remaining `call` flags, for example
`context_call context_identity --allow-http`. The walkthrough below uses this
helper. It does not store the token or bypass authentication.

### Connect over stdio

Stdio launches the **entire server**, not a small remote-client proxy. It needs the
backing services, runtime configuration, provider executables, and their launch
environment. If an operator has already prepared these:

```sh
export CONTEXT_STDIO_TOKEN="$(< "$HOME/.config/context-mcp/agent-token")"
"$CONTEXT_MCP" serve \
  --config /absolute/private/context-data/runtime.json \
  --transport stdio
```

Configure your MCP client to launch that command with the required environment.
Use the intended scoped service-credential pattern. Stdout is MCP traffic only;
logs go to stderr. Retain the connection while asynchronous work is processing.

**Only one application instance may own a deployment.** Stop its HTTP instance
before starting stdio against the same state. Do not launch a new application per
tool call. HTLK stdio consumers should keep one outer run alive across ingestion,
polling, retrieval, and purge; see the
[retained-session example](../examples/integration/stdio_context_journey.htlk).

## 4. Complete your first workflow

This walkthrough uses a small research note. You need a readable workspace and
the `share`, `ingest`, `read`, and `synthesize` capabilities plus corresponding
resource grants. If you cannot create collections, use one your operator supplied.
Reference processing and synthesis can call paid models even for short text.

Use the CLI helper from [section 3](#3-connect-to-an-existing-server), or send the
same JSON through your assistant. Local HTTP users add `--allow-http` to helper calls.

### Step 1: Check your workspace

Call `context_resource`:

```sh
context_call context_resource <<'JSON'
{
  "resource": {"kind": "workspace", "id": "<workspace-id>"}
}
JSON
```

Replace the placeholder before executing. A response includes `id`, `kind`,
`owner`, `parent`, `label`, and `acl_version`. If it is unavailable, check both
your credential scopes and the workspace's grants with your operator.

### Step 2: Create a collection

Call `context_create_collection`:

```json
{
  "workspace": "<workspace-id>",
  "label": "Ocean research",
  "idempotency_key": "ocean-guide.collection.1"
}
```

Expected result:

```json
{
  "collection": "<collection-id>",
  "workspace": "<workspace-id>"
}
```

Keep `collection`. Creation requires **Share**, not merely Ingest. A new collection
inherits access from its workspace.

### Step 3: Add a note

Call `context_ingest_text`:

```json
{
  "collection": "<collection-id>",
  "name": "ocean-notes.txt",
  "text": "The ocean is blue. This research note records our observation.",
  "idempotency_key": "ocean-guide.note.1"
}
```

The receipt looks like:

```json
{
  "job": "<job-id>",
  "asset": "<asset-id>",
  "revision": "<asset-revision-id>",
  "state": "accepted"
}
```

Keep all three IDs. **Accepted does not mean published.** It means the durable
processing job was admitted.

### Step 4: Wait for successful processing

Call `context_job_status` repeatedly with a short delay between requests:

```json
{"job": "<job-id>"}
```

Proceed when `state` is `succeeded`. A successful ingestion status includes a chunk
count and normally `result:null`. If it is `failed` or `cancelled`, use
[job recovery](#6-monitor-cancel-and-retry-processing).

### Step 5: Retrieve evidence

Call `context_query`:

```json
{
  "scope": {"kind": "collection", "id": "<collection-id>"},
  "query": "What color is the ocean?",
  "limit": 5,
  "token_budget": 256
}
```

Look in `evidence`. Each item has a content `id`, text, origin, rank, token count,
source references, and pointers. Exact segmentation and generated enrichment depend
on the selected processing profile; do not assume a fixed number of chunks.

Keep the evidence `id` and its source references. The source's `revision` binds the
claim to the source you actually queried, even if the asset changes later.

### Step 6: Inspect the evidence

Copy a returned pointer into `context_expand`:

```json
{
  "pointer": {"kind": "content", "content": "<evidence-content-id>"}
}
```

This returns the protected content body and origin without private object keys or
embedding vectors. For the original document, expand its returned asset/revision
pointer instead; an authorized response includes a short-lived original-byte URL.

### Step 7: Ask for a cited answer

Call `context_synthesize` with the **content ID**, not the asset revision:

```json
{
  "workspace": "<workspace-id>",
  "question": "What color did the research note record for the ocean?",
  "evidence": ["<evidence-content-id>"],
  "token_budget": 1024
}
```

The response has `saved:false`, `origin:"generated"`, answer `text`, and `citations`.
Review those citations. This answer is transient in application knowledge storage.
Saving it is a separate explicit choice described in
[section 11](#11-generate-and-save-cited-answers).

You have now exercised the core loop: **add → wait → retrieve → inspect → synthesize**.

## 5. Add files, text, and URLs

### Choose an ingestion method

| Input | Recommended route | Important distinction |
| --- | --- | --- |
| Local file | CLI `upload` | Hashes and streams the file, then finalizes it |
| Small text | `context_ingest_text` | Creates a new asset; no replacement asset argument |
| Larger/binary content via MCP | Create upload → HTTP PUT → finalize | Byte transfer goes directly to object storage |
| Exact remote resource | `context_ingest_url` | Operator-allowlisted URL only; no crawling or redirects |

Profiles determine extraction, model operations, chunking, and embedding space.
Omit `profile` to use the operator's default. A profile name is deployment-specific,
not a universally supported format preset. Missing capabilities reject admission;
provider failures after admission fail the job without publishing a partial revision.

### Upload a local file with the CLI

```sh
"$CONTEXT_MCP" upload \
  --endpoint "$CONTEXT_ENDPOINT" \
  --file "$HOME/Documents/ocean-report.pdf" \
  --collection "<collection-id>" \
  --media-type application/pdf \
  --idempotency-key ocean-report.v1 \
  --wait
```

Replace the collection placeholder. `--media-type` is optional, but specifying it
avoids an unhelpful extension-based guess. `--profile` optionally selects an
operator-configured profile. For local HTTP, add `--allow-http`.

Without `--wait`, the command prints an admission receipt. With it, it polls every
500 ms and prints the final successful status. The waiting ceiling is one hour;
timing out does **not** cancel the durable job. A failed wait may not print the
original receipt; recover the same operation with the same key and retain its IDs.

The CLI upload command creates a new asset. To replace an existing asset, use the
public create-upload/finalize flow with `asset`, described below.

### Ingest text directly

`context_ingest_text` accepts nonempty UTF-8 text up to **262,144 bytes**. It still
uses the verified-source processing pipeline, so it returns a job, not immediate
searchable content. It is useful for notes, pasted passages, or standalone source
text. Use memories for explicit project assertions rather than treating every fact
as a new text asset.

### Transfer bytes using an upload session

1. Compute the exact byte length and SHA-256 digest of the file.
2. Call `context_create_upload` with that metadata.
3. PUT the exact bytes to its returned URL using **every returned header**.
4. Call `context_finalize_upload` with the upload ID.
5. Poll the returned job.

For example, the five bytes `hello` have this digest:

```json
{
  "collection": "<collection-id>",
  "name": "hello.txt",
  "media_type": "text/plain",
  "size_bytes": 5,
  "sha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
  "idempotency_key": "hello.upload.1"
}
```

Send that object to `context_create_upload`. Its ordinary response contains
`upload`, `asset`, `revision`, `url`, `headers`, and `expires_in_seconds`.

The byte request is an HTTP **PUT**, not a multipart form or base64 JSON upload.
The returned headers include `content-length`, `x-amz-checksum-sha256`, and
`if-none-match`. The checksum header is base64-encoded; use the returned value
rather than substituting the hex digest from the create request.

For a real file, a transfer pattern is:

```sh
curl --fail --request PUT \
  --header 'content-length: <returned-length>' \
  --header 'x-amz-checksum-sha256: <returned-checksum>' \
  --header 'if-none-match: *' \
  --data-binary @/absolute/path/hello.txt \
  '<returned-upload-url>'
```

Replace all placeholders, and include any other returned headers. The file must
contain exactly the declared bytes; `hello` followed by a newline is six bytes.
Treat signed URLs as temporary secrets and avoid retaining them in shell history.

Then call `context_finalize_upload`:

```json
{
  "upload": "<upload-id>",
  "idempotency_key": "hello.finalize.1"
}
```

Finalization independently streams and checks the stored bytes, then freezes them
under a separate immutable source key. MCP creation alone does not upload bytes;
PUT success alone does not admit processing.

**Interrupted transfer:** a repeated conditional PUT can return HTTP **412** because
the previous attempt already committed. Try finalization; it verifies the bytes.
Do not overwrite the storage key or remove its fence. Expired or invalid sessions
are not made valid by a successful old PUT. Replaying creation after finalization
can return `finalized:true` and `job` instead of a fresh URL.

### Import an exact URL

Call `context_ingest_url`:

```json
{
  "collection": "<collection-id>",
  "name": "ocean-report.pdf",
  "url": "https://approved.example/reports/ocean.pdf",
  "media_type": "application/pdf",
  "idempotency_key": "ocean-url.1"
}
```

The hostname must be in the operator's `application.data.url_hosts`. An empty
allowlist disables URL acquisition. HTTPS is required unless development HTTP was
explicitly enabled. URLs with userinfo or fragments are rejected. Requests pin DNS,
do not follow redirects, and read only the exact returned resource. HTML import
does not fetch linked images, stylesheets, or other subresources.

Acquisition is bounded and occurs before the processing receipt is returned. The
source URL provenance omits query strings. Replaying an already admitted key
recovers its receipt instead of downloading a potentially changed URL again.

### Use idempotency deliberately

An idempotency key identifies one principal's intent for one operation. Reuse the
same key and same input when recovering from an uncertain response. Changed input
with the same key conflicts. A genuinely new import or refresh needs a new key.

Use short opaque operation labels, not source text, secrets, or URLs. The application
stores keys as SHA-256 handles, but their placement in your own logs still matters.
General keys are bounded to 256 UTF-8 bytes; text/URL/reprocess keys to 200 bytes.
The CLI adds `:upload` and `:finalize` to its supplied upload key; leave room for them.

Receipts describe the original admission. Replaying an old receipt can still say
`accepted` after completion. Use `context_job_status` for current state.

## 6. Monitor, cancel, and retry processing

### Read job status

Call `context_job_status` with `{"job":"<job-id>"}`. A status object includes:

| Field | Meaning |
| --- | --- |
| `job`, `asset`, `revision` | Identities of the attempt and immutable source |
| `state` | `accepted`, `running`, `succeeded`, `failed`, or `cancelled` |
| `stage` | Bounded progress label, not a percentage or fixed public stage enum |
| `error_code` | Redacted diagnostic when available; can be null |
| `chunks` | Published chunk count when applicable; can be null |
| `result` | Operation-specific result; media success includes `content` |
| `cleanup_pending` | Whether operational cleanup remains unfinished |

Only the submitting principal can inspect a job, even if another person can read
the asset. Current Read or Ingest authority is accepted for status. Raw provider
inputs, filesystem paths, and cleanup handles are not exposed.

### Cancel a job

Call `context_cancel_job`:

```json
{
  "job": "<job-id>",
  "idempotency_key": "ocean-guide.cancel.1"
}
```

Cancellation fences publication and commits the cancelled state. Cleanup may
continue asynchronously; inspect `cleanup_pending`. It is not a way to delete an
already published success. Cancelling a succeeded **or failed** job conflicts.
Ingestion controls require Ingest; cancelling a media-selection job requires Read.

### Retry failed or cancelled processing

Once cleanup settles, call `context_retry_job`:

```json
{
  "job": "<failed-or-cancelled-job-id>",
  "idempotency_key": "ocean-guide.retry.1"
}
```

It returns a **new job ID** for the retained immutable revision. Poll that new job.
Retry requires available capacity, no active replacement for the asset, no prior
publication of that revision, and the exact retained workflow revisions. It does
not silently pick up a different profile or model configuration.

Media-selection jobs cannot use this retry operation. After resolving their failure,
submit a new explicit extraction request with a new key.

### What survives a restart

Accepted durable jobs and their saved workflow handles survive same-version server
restart. They can continue after the submitting token expires, while later client
calls require valid credentials and current authority remains enforced.

Unconfirmed provider cleanup can keep jobs fenced and `cleanup_pending:true`.
Pending cleanup consumes capacity. Operators must repair the underlying dependency;
changing metadata to pretend cleanup succeeded is not a recovery procedure.

There is a documented lost-admission-receipt replay window of less than 29 days.
Older unresolved intents stay fenced/pending rather than being assumed never run.
See [ingestion recovery](ingestion.md#publication-and-recovery) for that boundary.

## 7. Search and explore evidence

### Choose a scope

`context_query` accepts a tenant, workspace, collection, or asset scope:

```json
{
  "scope": {"kind": "workspace", "id": "<workspace-id>"},
  "query": "observations about ocean color",
  "limit": 10,
  "token_budget": 1024,
  "historical": false
}
```

Use a narrower scope when you know the relevant source. A cross-tenant shared
asset may be readable even when its workspace is not; query the shared **asset**
instead of assuming you can query its parent's workspace.

`space` optionally selects a compatible operator-defined embedding space. Omitting
it selects the default. Vectors from different spaces cannot be mixed merely
because they have the same number of dimensions.

### Read a ContextMap

A simplified response is:

```json
{
  "version": "0.1",
  "evidence": [
    {
      "id": "<content-id>",
      "text": "The ocean is blue.",
      "origin": "original_evidence",
      "sources": [
        {
          "asset": "<asset-id>",
          "revision": "<asset-revision-id>",
          "location": {"kind": "asset"},
          "available": true
        }
      ],
      "rank": 0,
      "tokens": 5,
      "pointers": [
        {"kind": "content", "content": "<content-id>"},
        {"kind": "asset", "asset": "<asset-id>", "revision": "<asset-revision-id>"}
      ]
    }
  ],
  "pointers": [],
  "tokens": {"scheme": "cl100k_base", "limit": 1024, "used": 5},
  "truncated": false
}
```

- `evidence` contains selected text, not an automatically composed answer.
- `rank` starts at zero. Hybrid ranking combines lexical and compatible semantic
  retrieval using reciprocal-rank fusion; unrelated provider scores are not exposed
  as comparable confidence values. Exact duplicate text is selected once.
- `origin` distinguishes `original_evidence`, `agent_assertion`, and `generated`.
  Indexed enrichment does not become original evidence.
- `sources` carries exact revision/location references. Preserve them in citations.
- `tokens.used` accounts for selected evidence under the reported tokenizer, not
  the surrounding JSON or external model billing.
- `truncated` reports bounded evidence selection. Increase the budget or expand a
  pointer when you need more, subject to other limits.

The default limit is 20, with an allowed range of 1–100. Token budgets range from
1–65,536. Without a supplied budget, an internal 65,536-token ceiling still applies,
but `tokens.limit` is null. Selected text also has an independent 262,144-byte bound.
Scopes exceeding the 10,000-candidate bound are rejected; narrow the scope.

### Expand exact evidence

Use `context_expand` with a returned typed pointer. Available variants include:

```json
{"pointer": {"kind": "content", "content": "<content-id>"}}
```

```json
{"pointer": {"kind": "asset", "asset": "<asset-id>", "revision": "<asset-revision-id>"}}
```

```json
{"pointer": {"kind": "entity", "entity": "<entity-content-id>"}}
```

Content expansion returns `content`, `workspace`, nullable `asset`/`revision`,
`kind`, `origin`, `body`, and nullable `url`/`expires_in_seconds`. It strips vectors
and private object keys. Asset expansion includes metadata, enrichment, `current`,
media type, byte size, bounded child pointers, and an original-byte URL. Entity
records expose generated names/types and relationships while retaining source lineage.

Page/media pointers return situated evidence plus an extraction request description;
they do not automatically render bytes. Follow
[media extraction](#9-work-with-documents-images-audio-and-video) for that step.

### Download original or selected bytes

Authorized asset and binary-content expansion normally issues a **60-second** signed
GET URL. Download promptly. If it expires, perform another authorized expansion.
Do not remove query parameters from the URL or assume the internal storage endpoint
is client-reachable.

These URLs are short-lived bearer capabilities. Revoking access prevents subsequent
authorized operations but does not necessarily invalidate a URL already issued
within its lease. Avoid publishing URLs or treating them as durable citations.
Use exact resource/pointer identities for durable references.

## 8. Use revision history and update sources

### List published revisions

Call `context_asset_history`:

```json
{
  "asset": "<asset-id>",
  "limit": 20
}
```

The result contains `asset`, `revisions`, and `next_cursor`. Each revision entry
contains `revision`, `media_type`, `size_bytes`, `current`, and an exact pointer.
Only published revisions are listed. For another page, pass `next_cursor` as
`before`; this cursor is a **revision UUID**. Limit is 1–100.

Ordinary query selects current successful revisions and current memory views.
Set `historical:true` to opt into retained published history. Historical results
still require current source authority; history is not an authorization bypass.

### Decide which update operation you need

| Situation | Operation | Source bytes / workflow |
| --- | --- | --- |
| A processing attempt failed or was cancelled | `context_retry_job` | Same immutable source and frozen workflow; cleanup must settle |
| Re-run retained original with a selected profile | `context_reprocess` | Copies retained bytes into a new revision; no remote fetch |
| Your local file changed | Create upload with `asset`, PUT, finalize | New bytes become a replacement revision |
| A remote resource changed | `context_ingest_url` with `asset` and a new key | Explicitly downloads that exact URL again |

### Reprocess a published source

Read the current revision from asset metadata/history, then call
`context_reprocess`:

```json
{
  "asset": "<asset-id>",
  "expected_revision": "<current-asset-revision-id>",
  "idempotency_key": "ocean-report.reprocess.2"
}
```

Optionally include `profile`. A stale expected current revision conflicts. The
operation returns a new revision/job receipt; wait for success. Reprocessing
retains derivation lineage, so purging a source revision can also remove later
copies derived from it.

### Replace or refresh an asset

For a new local source, add `asset` to the create-upload arguments and keep the
correct `collection`. Upload and finalize the new bytes as usual.

For remote refresh, call `context_ingest_url` with the existing asset:

```json
{
  "collection": "<collection-id>",
  "asset": "<asset-id>",
  "name": "ocean-report.pdf",
  "url": "https://approved.example/reports/ocean.pdf",
  "media_type": "application/pdf",
  "idempotency_key": "ocean-url.refresh.2"
}
```

Use a new key for the new intent. The old current revision remains available until
the new processing succeeds. There is no implicit scheduled refresh.

## 9. Work with documents, images, audio, and video

### Reference format behavior

The shipped `multimodal` profile combines a native sandbox processor and model
roles. A different operator profile may have different support. Successful format
tests establish the reference workflow's bounded behavior, not every possible file
or interchangeable provider's semantics.

| Formats | What the reference processing does |
| --- | --- |
| TXT, Markdown, CSV, JSON, individual source files | Bounded text decoding/segmentation; JSON validation; exact byte coordinates for unchanged UTF-8 |
| HTML | Text/entity extraction, omitting script/style; no remote subresources |
| PDF | Text per page; image rendering for pages without text |
| DOCX, PPTX | ZIP/XML text and embedded images with archive-member locations |
| XLSX | Shared/inline strings and cached cell values per worksheet; no formula execution |
| EPUB | Offline XHTML and embedded images |
| JPEG, PNG, WebP, GIF, TIFF | Signature-checked raster; representative first frame for animation/multipage images |
| MP3, WAV, M4A, FLAC, Ogg | Verified containers; mono 16-kHz WAV segments up to 30 seconds |
| MP4, MOV, WebM, MKV | Frame samples every 10 seconds and audio segments with original time coordinates |
| Explicit repository ZIP, TAR, TAR.GZ | Bounded text members retaining paths; uploaded code is not executed |

Reference archives are limited to 1,000 entries and 64 MiB expanded bytes. Traversal,
absolute paths, symlinks, special TAR entries, and external XML document types are
rejected. Media duration is at most three hours; total segment, upload, and timeout
bounds can reject smaller inputs. Individual managed byte outputs are at most
2 MiB. A failed input does not publish a partial replacement.

Image descriptions, audio transcription, enrichment, and embeddings depend on the
configured model roles. Retrieval over a video is based on the descriptions,
transcripts, and sampled evidence produced by that workflow; it is not an exhaustive
claim that every frame was analyzed.

### Select a PDF page, image region, or media interval

Call `context_extract_media` with an exact published source revision. For a
one-second audio/video selection:

```json
{
  "asset": "<media-asset-id>",
  "revision": "<media-asset-revision-id>",
  "location": {"kind": "time", "start_ms": 1000, "end_ms": 2000},
  "idempotency_key": "ocean-clip.1"
}
```

Alternative locations:

```json
{"kind": "pages", "first": 2, "last": 2}
```

```json
{"kind": "region", "x": 0, "y": 0, "width": 128, "height": 128}
```

Page numbers are one-based. Media times are milliseconds in a half-open interval
`[start_ms,end_ms)`, at most 60 seconds. Regions use upper-left pixel coordinates
and must fit the actual source and bounds. Page materialization accepts one page
per request, not a multi-page PDF export.

The selected profile must supply `media_role`. The operation requires Read; it
materializes a protected derivative rather than replacing the source or granting
arbitrary write access.

### Retrieve the rendered result

1. Keep the returned media job ID.
2. Poll `context_job_status` until it succeeds.
3. Read `result.content` from that status.
4. Expand `{"kind":"content","content":"<result-content-id>"}`.
5. Download using its 60-second URL.

Reference output types are bounded PDF-page images, PNG crops, mono WAV selections,
and bounded-resolution H.264/AAC MP4 clips. The output is digest-verified, at most
2 MiB, and retains exact source coordinates and a purge dependency. Failure does
not replace the original revision. This job type does not support `context_retry_job`.

## 10. Create and reconcile semantic memories

A semantic memory is an explicitly authored project assertion identified by a
natural subject. It is useful for decisions, requirements, preferences, or facts
that need a current view and accountable history. It does not require a machine key.

### Write a first assertion

Call `context_write_memory`:

```json
{
  "workspace": "<workspace-id>",
  "subject": "Approved office temperature",
  "description": "Temperature in degrees Celsius",
  "value": {"kind": "typed", "value_type": "number", "value": 20},
  "expected_version": 0,
  "idempotency_key": "office-temperature.1"
}
```

For a natural-language statement, use:

```json
{"kind": "text", "statement": "The approved office temperature is 20 degrees Celsius."}
```

Built-in typed names are `string`, `number`, `integer`, `boolean`, `object`, `array`,
and `null`. Operators can map semantic type names to these JSON types.

The result includes:

```json
{
  "memory": "<memory-id>",
  "revision": "<memory-revision-id>",
  "content": "<memory-content-id>",
  "version": 1,
  "view": {"state": "current", "revision": "<memory-revision-id>"},
  "basis": "equivalent"
}
```

Keep `memory`, `version`, and the view. The new revision and the current winning
revision are not always the same after reconciliation. Use `content` when passing
this exact assertion to synthesis.

Subjects are normalized by whitespace and case. Use a separate subject for an
unrelated assertion. Supplying a memory ID does not permit renaming its subject.
`expected_version:0` means create; updates need the logical memory's known version.

### Read current state and history

Call `context_read_memory` by ID:

```json
{"memory": "<memory-id>"}
```

Or find its current view using a human subject:

```json
{"workspace": "<workspace-id>", "subject": "Approved office temperature"}
```

Do not combine the two selector forms. For history, select the memory ID:

```json
{"memory": "<memory-id>", "history": true, "limit": 20}
```

Responses contain `memory`, `workspace`, `version`, `view`, `revisions`, and
`next_cursor`. For history pagination, pass `next_cursor` as `before`; here the
cursor is a **version integer**, unlike asset history's UUID cursor. Limit is 1–20.
Subject lookup supports the current view only, not history/cursor options.

Revision bodies retain subject, description, value, author, origin, source
dependencies, citations, supersession, basis, and rationale. Write input
`derived_from` is represented as `sources` in returned memory bodies.

### Propose another assertion

`context_write_memory` and `context_reconcile_memory` accept the same proposed
assertion shape and use the same reconciliation behavior. Reconciliation is not a
tool that takes an arbitrary list of candidate IDs and rewrites them silently.

To propose 21 degrees against version 1:

```json
{
  "workspace": "<workspace-id>",
  "memory": "<memory-id>",
  "subject": "Approved office temperature",
  "description": "Temperature in degrees Celsius",
  "value": {"kind": "typed", "value_type": "number", "value": 21},
  "expected_version": 1,
  "idempotency_key": "office-temperature.2"
}
```

Different values of the same scalar type are recognized as contradictions by the
host. For other comparisons, a configured model can classify equivalence,
complementarity, or contradiction. Models cannot invent authority.

Equivalent assertions retain combined provenance. Complementary text/compatible
objects can merge as generated content. Contradictions remain explicit unless
authorized supersession or strict configured author precedence resolves them:

```json
{
  "state": "conflicted",
  "revisions": ["<first-memory-revision-id>", "<second-memory-revision-id>"]
}
```

Operator-defined author ranks can affect the result. An incoming lower-ranked claim
can be retained in history without replacing the higher-ranked current view. Equal
ranks preserve conflict. Always inspect `view` rather than assuming the last write wins.

### Resolve a conflict explicitly

Read the latest view and version. If you are authorized to make the decision,
propose the approved assertion and list the known current revision IDs in
`supersedes`:

```json
{
  "workspace": "<workspace-id>",
  "memory": "<memory-id>",
  "subject": "Approved office temperature",
  "description": "Explicitly approved correction",
  "value": {"kind": "typed", "value_type": "number", "value": 22},
  "expected_version": 2,
  "supersedes": ["<first-memory-revision-id>", "<second-memory-revision-id>"],
  "idempotency_key": "office-temperature.approved.3"
}
```

A stale expected version conflicts. Reread and reconsider the current contributions;
do not blindly increase the number to force a write.

### Declare sources honestly

Use the correct kind of relationship:

| Field | Meaning | Effect of source removal |
| --- | --- | --- |
| `derived_from` | This assertion's information derives from exact source references | Its revisions remain source-protected and enter transitive purge |
| `derived_content` | Information derives from exact managed content IDs | Preserves that content's transitive protections/deletion dependencies |
| `citations` | An independently authored assertion cites supporting evidence | Assertion can survive with unavailable support |
| `supersedes` | Explicitly replaces known current memory contributions | Requires authorized current revision IDs and expected version |

Source-reference shape:

```json
{
  "asset": "<asset-id>",
  "revision": "<asset-revision-id>",
  "location": {"kind": "asset"},
  "available": true
}
```

All newly supplied references need current read authority. `available:false` is not
a bypass. Do not label copied/derived material as citation-only simply to preserve
it through deletion. Independent assertions with removed support return unavailable
citations; source-derived knowledge does not become independently readable.

## 11. Generate and save cited answers

### Generate transiently

Call `context_synthesize` with a workspace and question. If `evidence` is omitted
or empty, the service retrieves bounded current-view context, with a retrieval limit
of 32, before synthesis:

```json
{
  "workspace": "<workspace-id>",
  "question": "What does our evidence say about ocean color?",
  "token_budget": 4096
}
```

For precise control, pass up to 64 unique evidence **content IDs**:

```json
{
  "workspace": "<workspace-id>",
  "question": "Summarize the observation and any uncertainty.",
  "evidence": ["<content-id-one>", "<content-id-two>"],
  "token_budget": 4096,
  "save": false
}
```

The budget limits input evidence, not answer length, total request JSON, or model
billing. Default is 8,192 tokens; allowed range is 1–16,384. Explicit evidence is
not silently shortened to fit: an over-budget selection fails, so choose fewer
inputs or increase the budget within bounds.

The model must return nonempty bounded text and citations to supplied evidence.
Invented IDs, missing citations, or invalid outputs fail before saved publication.
The response includes `saved`, `origin`, `text`, and `citations`.

**Transient** means the answer is not saved as permanent application knowledge.
HTLK execution records and provider histories have separate lifecycles. Repeating
a transient request runs another computation, even if you supplied a key.

### Explicitly save an answer

Add `save:true` and a stable idempotency key:

```json
{
  "workspace": "<workspace-id>",
  "question": "Summarize our ocean color evidence.",
  "evidence": ["<evidence-content-id>"],
  "token_budget": 4096,
  "save": true,
  "name": "Ocean color evidence summary",
  "idempotency_key": "ocean-summary.1"
}
```

The saved response additionally returns `content`. The generated summary becomes
searchable knowledge, retaining `origin:"generated"`. It is **not** a semantic-memory
revision and does not silently replace a project assertion.

Saved synthesis depends on **all evidence passed to the model**, not only the
citations it chose to emit. Later access requires all informational source
protections. Purging a contributing source removes the dependent artifact in full;
regeneration from surviving sources requires a new explicit request.

## 12. Share material and revoke access

### Understand the access layers

1. **Identity:** exact issuer/subject resolves a principal.
2. **Credential scopes:** set the maximum capabilities that token can use.
3. **Membership:** determines membership in tenant/workspace recipient groups.
4. **ACLs:** grant permissions directly or by inheritance.

Ownership does not change when material is shared. Sharing is live access to the
same source, not a copy. A direct principal grant may allow reading without tenant
membership. Owner-sensitive ACL changes and purge additionally require membership
in the owning tenant.

### Read the ACL version

Call `context_resource` for the resource to change:

```json
{"resource": {"kind": "asset", "id": "<asset-id>"}}
```

Keep `acl_version`. The metadata does **not** include the explicit grant list.
Maintain an operator/owner record of the full intended ACL before replacing it.

### Grant read access

Call `context_set_access`:

```json
{
  "resource": {"kind": "asset", "id": "<asset-id>"},
  "expected_acl_version": 1,
  "inherit": true,
  "grants": [
    {
      "recipient": {"kind": "principal", "id": "<recipient-principal-id>"},
      "permissions": ["read"]
    }
  ],
  "idempotency_key": "ocean-share.1"
}
```

Use the actual version. This **replaces the complete explicit grant list and
inheritance flag**. Include every explicit grant you intend to retain. Recipient
kinds are `principal`, `tenant`, and `workspace`; group recipients refer to current
memberships. Recipients must already exist.

An asset can inherit an owner's access while explicitly sharing with another
principal. Alternatively, `inherit:false` stops ancestor grants at that resource.
An empty explicit list with inheritance disabled can lock out the caller; there is
no implicit owner-read override.

### Revoke access

Replace the ACL again using the returned new version, removing the relevant grant.
To remove all explicit grants while preserving inherited access:

```json
{
  "resource": {"kind": "asset", "id": "<asset-id>"},
  "expected_acl_version": 2,
  "inherit": true,
  "grants": [],
  "idempotency_key": "ocean-share.revoke.2"
}
```

Empty grants do not revoke access that still arrives through inheritance or another
grant. Removing group membership does not remove an independent direct-principal
grant. Examine all applicable paths when diagnosing continued access.

Queries, expansions, and derived-content reads recheck current permissions.
Previously issued byte URLs can remain usable within their 60-second lease.

## 13. Permanently purge material

**Purge is irreversible.** It is not archiving, hiding, revoking a share, or moving
content to a recoverable trash folder. It removes the target's managed material
and its complete transitive derivations, including across owners and history.

### Choose the target and precondition

Target kinds are `tenant`, `workspace`, `collection`, `asset`, `revision`, `content`,
and `memory`.

| Target | Required `expected_generation` value |
| --- | --- |
| Asset | Current asset generation from `context_resource` |
| Asset revision | Its **parent asset's current generation**, even though the revision itself is immutable |
| Memory | Logical memory's current **version** from memory read/write |
| Tenant, workspace, collection, content | No generation comparison required |

The field is named `expected_generation` even for a memory. It is not an asset
revision UUID or a memory revision UUID.

You need current Purge scope/grant **and membership in the owning tenant**.
A foreign recipient cannot purge the original merely because a share included
Purge permission. Deletion of dependent derivatives is not restricted to your
readable subset.

### Admit deletion and retain its receipt

For an asset, read fresh metadata and call `context_purge`:

```json
{
  "target": {"kind": "asset", "id": "<asset-id>"},
  "expected_generation": 1,
  "idempotency_key": "ocean-asset.purge.1"
}
```

Use the actual generation. Response:

```json
{"purge": "<purge-id>", "state": "pending", "completed": false}
```

Admission immediately closes affected reads, reuse, and publication. Physical
cleanup may still be pending. Keep the purge ID independently of the deleted asset.

Call `context_purge_status` until complete:

```json
{"purge": "<purge-id>"}
```

Success is `state:"completed"` and `completed:true`, after required writer
settlement and managed physical cleanup. Status is creator-owned and requires a
currently valid Purge-scoped credential, but remains readable after the target is
gone. Pending status does not expose foreign derivative counts or source content.

### What deletion includes

Managed source/revision records, payloads, extracted content, derived media, chunks,
vectors/index documents, saved summaries, source-derived memory revisions including
history, and affected staged/temporary outputs are in scope.

- Mixed-source artifacts are removed in full.
- Independently authored citation-only assertions can survive with unavailable support.
- Surviving older claims are not automatically promoted to current.
- Summaries are not regenerated implicitly from surviving sources.
- In-flight work is fenced so late results cannot recreate the content.

### What completion does not claim

HTLK execution history and external providers' histories follow their own lifecycles.
Deletion does not promise disk-forensic secure erasure or removal of a recipient's
independent downloads. The reference sandbox's accepted destroy-receipt limitation
is documented in [the integration evidence](integration-spike.md).

Approved permanent **zero-byte upload fences** remain at opaque retired upload keys.
They contain no source payload and prevent late conditional uploads from restoring
bytes. Never delete/reuse those keys or apply an active-bucket lifecycle rule that
removes them. Only unversioned payload storage is accepted. Operator-added backups
or replicas must preserve the same deletion policy.

See [the purge contract](permanent-purge.md) for precise storage and recovery boundaries.

## 14. Install your own server

This section prepares a **local development/reference deployment**. The application
image includes the pinned reference model and sandbox providers. The media processor
is a separate image. Production needs operator-selected TLS, issuer, permissions,
secrets, storage policies, and host controls appropriate to your environment.

### 14.1 Choose container or host execution

| Route | Application/providers | Native processing | Best fit |
| --- | --- | --- | --- |
| Application container | Built together by `deploy/Dockerfile` | Separate processing image through host Docker | Self-contained reference installation |
| Host process | Build Rust app; install/configure providers separately | Same processing image through Docker | Development and debugging |

Both routes use one application instance, PostgreSQL, Typesense, unversioned
S3-compatible storage, private staging, and a durable local HTLK SQLite store.
The reference setup uses MinIO and development Keycloak. This is not a horizontally
shared worker deployment.

Recorded verification covers macOS/arm64 host development and Linux/arm64 containers.
Recipes include other platform selections, but see [verification.md](verification.md)
for actual measured evidence. Local image names in these instructions do not imply
that an application image or binary has been published to a public registry.

### 14.2 Prepare prerequisites and a private launch environment

Install Git, Python 3, Docker with Compose v2, and a working Docker daemon. Container
builds require network access and BuildKit-compatible builds. Host builds also need
the pinned Rust **1.98.0** toolchain and a normal native Rust build environment.
Cargo uses locked dependencies, including exact upstream Git revisions.

Obtain the source from this project's repository and run commands at its root.
Choose a new absolute private data directory, for example:

```sh
export CONTEXT_DATA_DIR="$HOME/.local/share/context-mcp"
export CONTEXT_UID="$(id -u)"
export CONTEXT_GID="$(id -g)"
```

Keep runtime state and secrets outside the checkout/build context. Ensure the chosen
parent exists and the directory is private and writable by the application identity.
The configuration helper creates new directories with mode 0700; it does not tighten
permissions of directories that already exist.

Supply these values in a trusted launch environment or a private secret file:

| Variable | Required value |
| --- | --- |
| `CONTEXT_POSTGRES_PASSWORD` | Your local PostgreSQL password |
| `CONTEXT_S3_ACCESS_KEY` | MinIO access key/root user |
| `CONTEXT_S3_SECRET_KEY` | MinIO secret/root password |
| `CONTEXT_TYPESENSE_KEY` | Typesense key |
| `CONTEXT_KEYCLOAK_PASSWORD` | Development Keycloak bootstrap-admin password |
| `CONTEXT_CONTAINER_DATABASE_URL` | PostgreSQL URL using `postgres:5432` and database/user `context`, with the configured password |
| `OPENAI_API_KEY` | Credential for the shipped model-provider configuration |
| `DOCKER_GID` | Group ID that gives the application access to the mounted Docker socket |

Database passwords in URLs must be correctly URL-encoded. Do not paste live
credential-bearing URLs into the handbook's command examples or graph files.

If your operator maintains a private shell-format launch file, load it like this:

```sh
set -a
. "$HOME/.config/context-mcp/launch.env"
set +a
```

The file is operator-supplied; no setup command creates it. Do not enable shell
tracing while loading secrets. Compose has its own `.env` interpolation, whereas
the application reads only its JSON and referenced environment variables.

**Set all required Compose variables even when launching only backing services.**
The complete Compose file contains required substitutions for the profiled
application too, including `CONTEXT_DATA_DIR` and `CONTEXT_CONTAINER_DATABASE_URL`.

By default, published ports bind to loopback:

| Service | Host port | Override variable |
| --- | --- | --- |
| PostgreSQL | 55432 | `CONTEXT_POSTGRES_PORT` |
| MinIO API | 59000 | `CONTEXT_S3_PORT` |
| MinIO console | 59001 | `CONTEXT_S3_CONSOLE_PORT` |
| Typesense | 58108 | `CONTEXT_TYPESENSE_PORT` |
| Development Keycloak | 58080 | `CONTEXT_KEYCLOAK_PORT` |
| Application HTTP | 8089 | `CONTEXT_HTTP_PORT` |

Changing a published port does not rewrite runtime endpoints, public URLs, or OIDC
issuer/audience. Update those deliberately if you depart from the reference values.

### 14.3 Build images

```sh
docker compose --file deploy/compose.yml build application minio
docker build \
  --tag context-mcp-media:processing \
  --file deploy/media/Dockerfile .
```

The application image builds unchanged pinned `llm-mcp` source and installs a
checksum-verified sandbox archive. The separate media image contains the Rust
processing helper, FFmpeg, and Poppler. Builds can take time and download upstream
dependencies. The reference provider performs model operations through its inherited
credential; actual ingestion/query/synthesis can incur provider charges.

### 14.4 Generate operator files and pin the processing profile

```sh
python3 tools/deployment_config.py \
  "$CONTEXT_DATA_DIR" \
  --container \
  --public-url http://localhost:8089/mcp \
  --public-s3-url http://localhost:59000

python3 tools/processing_profile.py \
  "$CONTEXT_DATA_DIR/profiles/context.yml" \
  --uid "$CONTEXT_UID" \
  --gid "$CONTEXT_GID"
```

The first command creates `runtime.json`, `discovery.json`, `staging/`, and
`profiles/`. Configuration files contain environment-reference names, not live
credential values. The profile helper inspects the actually built media image and
pins its immutable `sha256:` identity, not the mutable tag.

The profile named `context` has networking disabled, 1 CPU core, 2 GiB memory, a
128-process limit, a fixed UID/GID, no sudo, and a read/write job working directory.
Make its UID/GID consistent with the application and the host data directory.
Compose otherwise defaults to 1000:1000, while the profile helper defaults to the
invoking host user's IDs.

Both helpers refuse to overwrite existing output files. Configuration creation is
not transactional across both files: review partial results after a failure. For
an existing installation, edit operator configuration deliberately or prepare new
files; do not rerun setup as if it reset or migrated retained state.

### 14.5 Start and check backing services

```sh
docker compose --file deploy/compose.yml \
  up --detach --no-build postgres minio typesense keycloak
```

Wait for initialization. Useful checks include:

```sh
docker compose --file deploy/compose.yml exec -T postgres \
  pg_isready -U context -d context
curl --fail http://127.0.0.1:59000/minio/health/ready
curl --fail http://127.0.0.1:58108/health
curl --fail \
  http://127.0.0.1:58080/realms/context-dev/.well-known/openid-configuration
```

Only PostgreSQL has an explicit Compose health check. A container having started
does not guarantee that MinIO, Typesense, or Keycloak is ready for provisioning.

### 14.6 Provision storage, catalogs, identity, and credential

Define an operator-command helper:

```sh
operator() {
  docker compose --file deploy/compose.yml --profile application \
    run --rm --no-deps -T application "$@"
}
```

Run these in order:

```sh
operator provision-data --config /configuration/runtime.json
operator migrate --config /configuration/runtime.json
operator discover-providers \
  --config /configuration/discovery.json \
  --output /configuration/catalogs.json
operator bootstrap \
  --config /configuration/runtime.json \
  --issuer context-service \
  --subject agent \
  --kind service \
  --label "Ocean research"
```

- `provision-data` explicitly creates/verifies the unversioned bucket.
- `migrate` applies application PostgreSQL migrations.
- Discovery captures exact provider descriptors through HTLK without inference.
- `bootstrap` creates an initial tenant/workspace/collection and a service principal
  grant. Record all returned IDs. It does not issue a credential.

**Bootstrap is not an idempotent setup check.** Running it again creates another
hierarchy even if the same identity already exists. Reuse the returned IDs.

Set `PRINCIPAL` to the returned principal UUID, then issue a demonstration credential:

```sh
operator issue-credential \
  --config /configuration/runtime.json \
  --principal "$PRINCIPAL" \
  --scope read \
  --scope ingest \
  --scope share \
  --scope write_memory \
  --scope synthesize \
  --scope purge \
  --output /configuration/agent-token
```

The secret is written once to `"$CONTEXT_DATA_DIR/agent-token"`, mode 0600 on Unix,
and is not printed. Record the nonsecret credential ID printed by issuance for later
revocation. Default lifetime is 86,400 seconds; use `--lifetime-seconds` to choose
another finite lifetime up to 90 days. Give ordinary clients only needed scopes.

### 14.7 Start the application

```sh
docker compose --file deploy/compose.yml --profile application \
  up --detach --no-build application
curl --fail http://127.0.0.1:8089/health/ready
```

Readiness follows initialization, role compilation, exact provider-descriptor
verification, bucket/versioning checks, and index setup. It does not prove model
quota, billing activation, successful inference, or correct native execution.

To use the host CLI, build it with the pinned toolchain, load the newly issued
token, and verify identity:

```sh
cargo build --workspace --all-features --locked
export CONTEXT_MCP="$PWD/target/debug/context-mcp"
export CONTEXT_ENDPOINT="http://localhost:8089/mcp"
export CONTEXT_TOKEN="$(< "$CONTEXT_DATA_DIR/agent-token")"
"$CONTEXT_MCP" call \
  --endpoint "$CONTEXT_ENDPOINT" \
  --allow-http \
  --tool context_identity <<'JSON'
{}
JSON
```

Continue with [the first workflow](#4-complete-your-first-workflow), using the
returned workspace/collection IDs. Client CLI calls go over HTTP; they do not start
a second server instance.

### 14.8 Preserve the host-path and Docker boundaries

Compose mounts the data directory both at `/configuration` and at its **identical
absolute host path**. Runtime staging uses that identical path. The sandbox asks
the host Docker daemon to mount job directories, so container-only staging paths
that do not exist on the host break processing.

The reference deployment mounts `/var/run/docker.sock`. Configure `DOCKER_GID` for
usable socket permissions. On Docker Desktop, ensure the host data path is shared
with Docker. The socket grants substantial control over host containers; it is an
explicit trust boundary. Native jobs themselves mount only their private job
directory and use the pinned bounded profile.

For a stdio container, stop HTTP first:

```sh
docker compose --file deploy/compose.yml --profile application stop application
export CONTEXT_STDIO_TOKEN="$(< "$CONTEXT_DATA_DIR/agent-token")"
operator serve --config /configuration/runtime.json --transport stdio
```

An MCP client's launcher should invoke the equivalent Compose command, retaining
stdin/stdout and using `-T`, not a TTY. Keep that single process alive across
dependent operations.

### 14.9 Alternative: run the application on the host

Use the same backing-service preparation and separately built media image. Install
the reference providers using their upstream instructions or your operator's pinned
distribution. The shipped host configuration expects:

- Model binary: `../llm-mcp/target/debug/llm-mcp`, invoked with `serve`.
- Sandbox binary: `target/providers/mcp-sandbox`.

Those are paths relative to the process working directory, not automatic downloads.
The application build and configuration helper do not install providers. See
[integration evidence](integration-spike.md) for the pinned-provider background and
[the application Dockerfile](../deploy/Dockerfile) for its exact source/archive pins.
Set absolute provider `program` paths in both runtime/discovery configuration if
you want to launch from elsewhere.

Choose a separate new host data directory rather than reusing the container config:

```sh
cargo build --workspace --all-features --locked
python3 tools/deployment_config.py "$CONTEXT_DATA_DIR"
python3 tools/processing_profile.py "$CONTEXT_DATA_DIR/profiles/context.yml"
export ALLOW_DIRS="$CONTEXT_DATA_DIR/staging"
export SANDBOX_PROFILES_DIR="$CONTEXT_DATA_DIR/profiles"
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
```

Supply `CONTEXT_DATABASE_URL` privately with the host endpoint
`127.0.0.1:55432`, database/user `context`, and the configured password. Keep the S3,
Typesense, and model credential references loaded. The generated host OIDC config
expects development Keycloak; either keep it available or deliberately configure
`application.oidc` as null for service-credential-only use.

Run the same provisioning sequence with host paths:

```sh
target/debug/context-mcp provision-data --config "$CONTEXT_DATA_DIR/runtime.json"
target/debug/context-mcp migrate --config "$CONTEXT_DATA_DIR/runtime.json"
target/debug/context-mcp discover-providers \
  --config "$CONTEXT_DATA_DIR/discovery.json" \
  --output "$CONTEXT_DATA_DIR/catalogs.json"
target/debug/context-mcp bootstrap \
  --config "$CONTEXT_DATA_DIR/runtime.json" \
  --issuer context-service --subject agent --kind service --label "Ocean research"
```

Issue a credential using the returned principal as in the container route, with
`--config "$CONTEXT_DATA_DIR/runtime.json"` and an output under the private host
directory. Then start the one host instance:

```sh
target/debug/context-mcp serve \
  --config "$CONTEXT_DATA_DIR/runtime.json" \
  --transport http
```

Host generation does not apply `--public-s3-url`; if clients need another byte
endpoint, explicitly configure `application.data.s3_public_endpoint`.

### 14.10 Enable human OIDC access

Container-generated configuration has `application.oidc:null`: service credentials
work, but running Keycloak alongside it does not enable human login automatically.
Configure an issuer that both the server and clients can reach with a consistent
issuer identity.

An example JWT configuration is:

```json
{
  "issuer": "https://identity.example/realms/team",
  "audience": "https://context.example/mcp",
  "scope_map": {
    "context:read": "read",
    "context:ingest": "ingest",
    "context:memory:write": "write_memory",
    "context:synthesize": "synthesize",
    "context:share": "share",
    "context:purge": "purge"
  },
  "required_claims": {"typ": "Bearer"},
  "verification": {"mode": "jwks"},
  "allow_http": false
}
```

This object belongs at `application.oidc`. Scope names and `required_claims` must
match your issuer's **access-token** contract; `typ:Bearer` is the reference
Keycloak marker, not a universal OIDC requirement. Configure API audience issuance
and an appropriate client login flow at your identity provider.

Supported JWT algorithms are RS256, ES256, and EdDSA. Alternatively use
`verification:{"mode":"introspection","client_id":"…","client_secret_env":"…"}`
with an actual operator client ID and secret environment-reference name. Match
required claims to the introspection response rather than blindly copying JWT
markers. Container launches must explicitly forward that secret variable; Compose
does not inherit arbitrary host variables automatically.

To enroll a human deliberately, obtain their exact issuer/subject, then run:

```sh
target/debug/context-mcp register-identity \
  --config "$CONTEXT_DATA_DIR/runtime.json" \
  --issuer "$OIDC_ISSUER" \
  --subject "$OIDC_SUBJECT" \
  --kind human

target/debug/context-mcp membership \
  --config "$CONTEXT_DATA_DIR/runtime.json" \
  --principal "$HUMAN_PRINCIPAL" \
  --resource "$TENANT"
```

Set `HUMAN_PRINCIPAL` from registration and `TENANT` from the existing hierarchy.
Container operators use the equivalent `operator` commands with `/configuration`
paths. Registration returns no grant; membership is also separate. An authorized
owner must supply the intended ACL using `context_set_access`. Successful OIDC
authentication can resolve an ungranted human principal automatically, but email or
tenant-like claims never enroll it in your resources.

The imported development realm has public test credentials, a development client,
and short-lived access tokens. It is not a production authentication recipe. Its
Keycloak container has no dedicated persistent data volume in this Compose file;
recreation can change user subject IDs and require deliberate reenrollment.

### 14.11 Configure public deployment endpoints

The server itself listens on plain HTTP. Setting an HTTPS `public_url` does **not**
enable TLS. Terminate TLS in operator infrastructure, preserve the required MCP and
authorization headers, and configure exact allowed hosts/origins and API audience.

`public_url` must identify `/mcp`, without userinfo or a query. Separate the private
`s3_endpoint` from a client-reachable `s3_public_endpoint` when needed. Signed byte
URLs must be reachable by users; private processing transfers use the private
endpoint. This repository does not include a tested production reverse-proxy or
backup/restore runbook; use the actual operation boundaries below when designing one.

## 15. Operate and maintain an installation

### Configuration map

The trusted JSON configuration is loaded at startup. Unknown fields, duplicate
members, malformed URLs, and invalid bounds are rejected. The runtime file is
limited to 1 MiB. There is no hot reload.

| Area | Main settings and responsibility |
| --- | --- |
| HTTP | `bind`, `public_url`, `allowed_hosts`, `allowed_origins`, `max_request_bytes`, `request_timeout_ms`, `max_concurrent_requests` |
| Stdio | `stdio_token_env` names the launch-environment credential |
| Database | `application.database_url_env`, `database_connections` |
| Human identity | `application.oidc` issuer/audience, scope map, verification, claims |
| HTLK execution | `application.workflows.database`, `workers`, `max_runs` |
| Provider bindings | `application.workflows.endpoints`, `required_deployments` |
| Graph selection | `application.workflows.roles` sources, catalogs, approved checkpoints |
| Storage | `application.data.s3_*`, `typesense_endpoint`, `typesense_key_env`, `staging_directory` |
| Retrieval | `spaces`, `default_space`, `token_encoding` |
| Processing | `profiles`, `default_profile`, `max_upload_bytes`, `upload_lifetime_seconds`, `max_pending_jobs`, `workers`, `max_segments` |
| URL acquisition | `url_hosts`, `url_allow_http` |
| Memory/generation | `reconcile_role`, `synthesis_role`, `memory_authority`, `memory_types` |

Role/catalog/staging/workflow-store paths resolve relative to the configuration
directory unless absolute. Provider executable paths need their own usable process
working directory or absolute paths. Start with the
[development configuration](../examples/operator/development.json), the generator,
and the concrete contracts in [ingestion.md](ingestion.md) and
[workflow-host.md](workflow-host.md).

Providers are replaceable through operator-managed graphs, exact catalogs, and
deployment bindings. A future provider needs semantic verification of the selected
roles; a matching transport alone does not establish compatible behavior. Users
cannot inject provider names, arbitrary graphs, commands, or credentials through
ordinary tool arguments.

### Health and logs

Public endpoints are:

```text
GET /health/live
GET /health/ready
GET /.well-known/oauth-protected-resource/mcp
```

Readiness means initialization completed and shutdown has not begun. It does not
continuously probe every dependency. Diagnose a failing operation using its job
status and redacted server/provider diagnostics even if readiness is still positive.
There is no bundled metrics endpoint or general job/purge inventory CLI.

Operational logs are JSON on stderr. Set `RUST_LOG` in the server's launch environment
to select tracing filters. For Compose, use an explicit environment override if you
want a host `RUST_LOG` value forwarded to the application. Inspect container logs:

```sh
docker compose --file deploy/compose.yml --profile application logs application
```

Do not enable logging that records credentials or signed URLs. A successful
readiness check is not authorization to invoke a paid acceptance campaign.

### Graceful shutdown and restart

For a host process, use Ctrl-C or SIGTERM. For the container:

```sh
docker compose --file deploy/compose.yml --profile application stop application
docker compose --file deploy/compose.yml --profile application \
  up --detach --no-build application
```

The reference Compose stop grace period is 180 seconds. Preserve PostgreSQL, object
storage, HTLK SQLite, private staging, and exact operator configuration. Workers
retain durable handles for compatible restart; do not delete staging as a generic
repair. Startup and database-backed operator commands also apply embedded PostgreSQL
migrations, so treat binary changes as controlled maintenance.

The native HTLK client has a lifetime bound of **4,096 owner identities per host
lifetime**, separate from concurrent-request capacity. At that bound it fails closed.
The documented response is a graceful restart with the same compatible store and
configuration, not an invented owner/fence reset.

### Rotate or revoke a service credential

1. Issue another credential to a **new private file**, with appropriate scopes/lifetime.
2. Deliver it securely and verify identity and required access.
3. Update clients. Restart stdio with the new launch token; its token value is
   captured when the process starts.
4. Revoke the old credential ID:

```sh
target/debug/context-mcp revoke-credential \
  --config "$CONTEXT_DATA_DIR/runtime.json" \
  --principal "$PRINCIPAL" \
  --credential "$OLD_CREDENTIAL_ID"
```

Only existing service principals can receive service credentials. A token's secret
is not recoverable from the database; only its hash is stored. Expiry/revocation and
current stored scope are rechecked in protected service-credential transactions.

### Repair a derived search index

Call `context_rebuild_index`:

```json
{"workspace": "<workspace-id>"}
```

Optionally include `space`. This synchronous operation requires owner Share
authority and Read on every contributing source. It recreates a missing compatible
collection and repopulates the workspace from retained published text/vectors,
including retained history. Response includes `workspace`, `space`, and `indexed`.

Repair does not rerun models, recover lost original bytes, or regenerate purged
knowledge. Its count need not match a current-only query's evidence count. The
operation is bounded to 10,000 authoritative records.

### Change graphs, models, or storage deliberately

Processing admissions freeze role fingerprints. Keep the old graphs, catalogs,
bindings, and credential references needed by unfinished work and cleanup. Startup
or retry rejects incompatible retained selections rather than silently redirecting
work to new roles.

Embedding-space identity includes model/space semantics, not just dimension.
Do not reuse an existing retained Typesense collection binding for an incompatible
space. Historical managed index bindings remain part of purge cleanup; keep their
old endpoints and key-reference names usable until retained data is removed.

Bucket/endpoint changes require an explicit data-migration plan preserving managed
objects, source lineage, and deletion policy. Never enable/suspend bucket versioning
and assume it remains compatible. Both enabled and suspended history are rejected.

### Upgrade an incompatible HTLK execution store

The recorded upgrade from HTLK SQLite schema 6 to 13 has no automatic migration or
reset. Follow [the full retained-data cutover](deployment.md#upgrading-the-embedded-htlk-runtime).
In summary:

1. Preserve the old binary/image, graphs, catalogs, profiles, and configuration.
2. Freeze new admissions at the deployment boundary while the old runtime settles
   jobs, cancellation cleanup, purges, uploads, URL acquisitions, and transient work.
3. Stop the old instance and inspect authoritative state. Unresolved work or pending
   cleanup still requiring the old runtime blocks cutover; resume it to settle that work.
4. Preserve its SQLite database and associated WAL/SHM files with the matching
   binary/configuration. Select a **new unused** SQLite path for the upgraded runtime.
5. Keep PostgreSQL, S3, Typesense, and permanent upload fences intact. Install
   upgraded graphs/catalogs, start the one replacement, and verify retained reads
   and explicitly authorized new operations.

Legacy failed jobs cannot silently retry with new role fingerprints. Reprocess a
published source explicitly or resubmit original input as a new operation/key.
Do not copy private runtime tables or change handles to manufacture compatibility.
Rollback after new admissions requires settling that work under its matching runtime
and reviewing retained workflow selections.

### Plan backups and disaster recovery around actual state

The reference deployment has **no configured payload backups/replication and no
tested backup/restore command or runbook**. If your installation adds them, design
a coordinated recovery and deletion policy. Useful state classification is:

| State | Recovery significance |
| --- | --- |
| PostgreSQL | Authoritative application hierarchy, content/history, jobs, authority, receipts and deletion state |
| S3 payloads and fences | Authoritative bytes and permanent write-control records |
| HTLK SQLite and associated files | Durable executions/history; needs compatible binary/configuration |
| Private staging | May be needed by active processing or cleanup; not universally disposable |
| Graphs, catalogs, profiles, configuration | Exact provider/workflow identities and launch references |
| Typesense | Derived; explicitly rebuildable from retained authoritative published text/vectors |
| Identity provider | Separate lifecycle; loss/recreation can affect exact issuer/subject identities |

A stopped, coordinated snapshot is an operator design starting point, not a
verified restore guarantee from this project. Test your recovery plan separately.
An old snapshot must not silently resurrect purged material, and any replicas or
backups must implement the managed deletion policy. HTLK history remains outside
application purge even when retained with backups.

### Preserve accepted cleanup boundaries

The application accepts the sandbox provider's successful destroy receipt. The
reference provider has a measured restart/container-lifetime bug: a container can
survive a reported destroy after provider restart. The accepted boundary does not
claim independently verified process termination.

No watchdog, TTL, direct-Docker cleanup bypass, or forced authority settlement is
part of this deployment contract. Preserve the evidence and fix/replace providers
through supported operator configuration and verified behavior.

## 16. Troubleshoot common problems

First separate **connection/authentication**, **tool argument/domain failure**, and
**asynchronous job failure**. A successful HTTP exchange or MCP call does not mean
an admitted job later succeeded.

### Diagnostic table

| Symptom | Likely checks and next action |
| --- | --- |
| HTTP 401 / authentication failure | Check expiry/revocation, exact issuer/audience, access token rather than ID token, scope mapping, and bearer header. Confirm the client actually loaded its secret reference. |
| Host or browser-origin rejection | Match `public_url`, `allowed_hosts`, and exact `allowed_origins`; account for a proxy's forwarded Host/Origin behavior. |
| Identity works but resource is unavailable | Check the ID, scope, resource grant, inheritance, membership-based recipients, purge state, and transitive source protections. Missing and unauthorized resources are intentionally not distinguished. |
| Workspace query fails for a shared asset | Query the explicitly shared asset scope; its workspace may not be shared. |
| No workspaces/assets appear in the client | There is no complete public inventory; obtain starting IDs from the operator and retain operation results. Empty `resources/list` is expected. |
| Admission receipt still says accepted | It is the original receipt. Poll `context_job_status`. |
| Query is empty after ingestion | Wait for `succeeded`; verify scope/current revision, selected embedding space, source authority, and profile output. A failed job publishes no new evidence. |
| Historical evidence is absent | Use `historical:true` and/or asset history; failed/unpublished revisions are not published history. |
| Query says overloaded | Narrow an overlarge scope or wait for capacity/cleanup to settle. Requests do not silently search only part of an oversized candidate set. |
| URL ingestion is rejected | Check exact allowed hostname, HTTPS/development opt-in, media type, size, and redirects. Allowlisting a site does not enable crawling. |
| Upload PUT returns 412 | The previous conditional PUT may have committed; try finalization. Do not overwrite/delete the opaque key. |
| Upload verification fails | Recheck exact length/digest and all returned headers, URL expiry, and whether the file changed during transfer. |
| Upload CLI rejects a format | Override `--media-type` if extension guessing is wrong; check the selected profile's supported media types/capabilities. |
| Job failed and retry conflicts | Check terminal state, `cleanup_pending`, active replacement, published revision, queue capacity, and frozen role compatibility. Media jobs do not support this retry. |
| Replacement failed but old results remain | Expected: prior successful revision remains current. Inspect the replacement's job error. |
| Native processing cannot find files | Verify the identical host/container staging path, `ALLOW_DIRS`, profile directory/name, fixed IDs, Docker socket access, and built image digest. |
| Model fails despite readiness | Readiness verifies startup/descriptors, not account quota, billing, model access, or response semantics. Inspect the redacted job/provider diagnostics. |
| Memory write conflicts | Reread logical version/view and decide against the actual current contributions. Use memory revision IDs for supersession, not content IDs. |
| Saved summary becomes unavailable | A contributing informational source may have been revoked/purged. Destination access alone is insufficient. |
| Download URL expires or is unreachable | Re-expand for a fresh URL; correct client-reachable `s3_public_endpoint` and network routing. |
| Purge stays pending | Required writer/provider/backing-service cleanup has not settled. Repair dependencies and keep the single instance running; do not force completion or delete active fences. |
| Startup rejects the execution store | Use a compatible binary/store pair or the documented retained-data cutover; never reset retained history automatically. |
| Setup helper refuses output | It intentionally refuses overwrite. Inspect existing/partial operator files before preparing replacements. |

### Read structured tool errors

Domain failures are MCP error results (`isError:true`) with an application object:

```json
{
  "error": {
    "code": "conflict",
    "message": "application storage operation failed",
    "retryable": false
  }
}
```

Mapped codes are `invalid_input`, `unavailable`, `conflict`, `missing_capability`,
`overloaded`, `processing_contract`, `cancelled`, and `operation_failed`. The current
implementation reports `retryable:false` for all of these. Do not interpret a broad
failure as permission to replay a model operation automatically.

Malformed JSON, unknown arguments/tools, invalid selectors, authentication, and
resource-read failures can instead be protocol errors. Resource domain failures
are redacted as `resource is unavailable`. Async job `error_code` is a separate
diagnostic, not necessarily one of the tool-error codes above.

### Recover uncertain operations

- Lost response after admission: repeat the **same intent/key**, obtain its receipt,
  and inspect the current job. Do not invent a new key until choosing a new operation.
- Stale version/generation/ACL: reread state and make a new explicit decision.
- Dependency failure: restore the backing service/provider and inspect cleanup
  before retrying. Some pending cleanup is maintained automatically by the server.
- Client wait timeout: retain/recover the IDs and inspect status; timeout did not
  itself cancel accepted work.

### Information to give your operator

Provide the tool name, nonsecret resource/job/purge IDs, time of failure, redacted
error code, current state, and whether cleanup is pending. Do not send live tokens,
signed URLs, provider secrets, or private source text merely to identify a failure.

### Frequently asked questions

**Does search automatically save an answer?** No. Search returns evidence. Synthesis
is separate, and synthesis is transient unless `save:true` is explicit.

**Does sharing copy the source?** No. It grants live access to the same owned source.
Your own downloaded copies have their own lifecycle.

**Can a model decide which contradictory project fact is authoritative?** It can
classify relations, but only authorized supersession or configured source authority
can select a winner. Otherwise the conflict remains visible.

**Does reprocess refresh a URL?** No. It uses retained original bytes. Import the
exact URL again with `asset` and a new key to refresh.

**Does index rebuild restore deleted documents?** No. It uses existing authoritative
published text/vectors; it does not recover bytes or regenerate purged knowledge.

**Can I run HTTP and stdio simultaneously against the same data?** No. Both are
complete application instances; this deployment has a single-instance boundary.

**Can I remove zero-byte objects to tidy the bucket?** Not the active deployment's
opaque upload fences. They prevent late writes from restoring retired payloads.

**Does supporting a format mean every feature is extracted?** No. Reference
processing is deliberately bounded and includes representative image/video sampling.

## 17. MCP tool reference

The server exports **23 tools**, the same operations over HTTP and stdio. Tables
below list required fields separately from optional fields. Optional arrays default
to empty unless otherwise noted. IDs are UUIDs; `profile` and `space` are configured
names. Do not add extra fields: public tool inputs reject unknown properties.

Export exact current descriptors without starting providers or using credentials:

```sh
target/debug/context-mcp catalog --transport http
target/debug/context-mcp catalog --transport stdio
```

The output is an HTLK catalog bundle with tools/resources/prompts, not just the
result of MCP `tools/list`. Published output schemas are generic objects; the field
descriptions below document implemented responses. Nested runtime rules, such as
pointer shape and conditional purge preconditions, still apply beyond the schema.

### 17.1 Identity and organization

| Tool | Required arguments | Optional arguments | Application result |
| --- | --- | --- | --- |
| `context_identity` | Empty object `{}` | None | `principal`, `scopes`, nullable `credential` |
| `context_resource` | `resource:{kind,id}` | None | `id`, `kind`, `owner`, `parent`, `label`, `acl_version`; assets add `generation`, `current_revision` |
| `context_create_collection` | `workspace`, `label`, `idempotency_key` | None | `collection`, `workspace` |
| `context_set_access` | `resource`, `expected_acl_version`, `inherit`, `grants`, `idempotency_key` | None | `resource`, new `acl_version`, `inherit` |

Resource kinds: `tenant`, `workspace`, `collection`, `asset`. Recipient kinds:
`principal`, `tenant`, `workspace`. Each grant is
`{"recipient":{"kind":"principal","id":"<principal-id>"},"permissions":["read"]}`.
At most 128 grants; each permission list is nonempty. ACL replacement needs owning
tenant membership and Share. Creating a collection also requires Share.

### 17.2 Ingestion and jobs

| Tool | Required arguments | Optional arguments | Application result |
| --- | --- | --- | --- |
| `context_create_upload` | `collection`, `name`, `media_type`, `size_bytes`, `sha256`, `idempotency_key` | `asset`, `profile` | `upload`, `asset`, `revision`, signed `url`, `headers`, `expires_in_seconds`; finalized replay may return `finalized`, `job` instead of transfer fields |
| `context_finalize_upload` | `upload`, `idempotency_key` | None | Admission receipt: `job`, `asset`, `revision`, `state` |
| `context_ingest_text` | `collection`, `name`, `text`, `idempotency_key` | `profile` | Admission receipt |
| `context_ingest_url` | `collection`, `name`, `url`, `media_type`, `idempotency_key` | `asset`, `profile` | Admission receipt |
| `context_job_status` | `job` | None | `job`, `asset`, `revision`, `state`, `stage`, `error_code`, `chunks`, `result`, `cleanup_pending` |
| `context_cancel_job` | `job`, `idempotency_key` | None | `job`, cancelled `state`, `cleanup_pending` |
| `context_retry_job` | `job`, `idempotency_key` | None | Admission receipt with **new job ID** for the same retained revision |

`sha256` is 64 hex digits, not the base64 header used in PUT. Size must be at least
one byte. Names must be nonblank, bounded to 256 characters; declared MIME type is
normalized and must be supported by the profile. Text is `text/plain` and cannot
target an existing asset. URL import and reprocess use shorter key bounds than
the general 256-byte limit.

Job states are `accepted`, `running`, `succeeded`, `failed`, `cancelled`. Status and
controls are submitter-owned. Retry supports only failed/cancelled ingestion or
reprocess jobs after settled cleanup and compatible frozen-workflow checks. Media
jobs have different cancellation authority and do not support retry.

### 17.3 Retrieval, history, and media

| Tool | Required arguments | Optional arguments / defaults | Application result |
| --- | --- | --- | --- |
| `context_query` | `scope:{kind,id}`, `query` | `limit:20`, `token_budget`, `historical:false`, `space` | ContextMap: `version`, `evidence`, `pointers`, `tokens`, `truncated` |
| `context_expand` | `pointer` | None | Variant-specific protected content, asset metadata, entity, or situated evidence/extraction description |
| `context_asset_history` | `asset` | `before` revision UUID, `limit:20` | `asset`, `revisions`, `next_cursor` |
| `context_reprocess` | `asset`, `expected_revision`, `idempotency_key` | `profile` | New revision/job admission receipt |
| `context_extract_media` | `asset`, `revision`, `location`, `idempotency_key` | `profile` | Media job admission receipt; success status adds `result.content` |

Query scope kinds match the resource kinds above. `context_expand` accepts only a
pointer; it has no additional token-budget, pagination, crop, or filter arguments.
See [pointer and location shapes](#192-pointers-and-source-locations) for the exact
variants. Reprocess expects the **current revision UUID**, not generation.

### 17.4 Memory and synthesis

| Tool | Required arguments | Optional arguments / defaults | Application result |
| --- | --- | --- | --- |
| `context_write_memory` | `workspace`, `subject`, `description`, `value`, `expected_version`, `idempotency_key` | `memory`, `derived_from`, `citations`, `derived_content`, `supersedes` | `memory`, new `revision`, `content`, `version`, `view`, `basis` |
| `context_reconcile_memory` | Same as write | Same as write | Same implementation/result as write |
| `context_read_memory` | Either `memory`, **or** `workspace` plus `subject` | ID selection: `history:false`, `before` version integer, `limit:20` | `memory`, `workspace`, `version`, `view`, `revisions`, `next_cursor` |
| `context_synthesize` | `workspace`, `question` | `evidence:[]`, `token_budget:8192`, `save:false`, `name`, `idempotency_key` | `saved`, `origin`, `text`, `citations`; saved result adds `content` |

Memory `description` is required but can be empty. Value shapes are
`{"kind":"text","statement":"…"}` or
`{"kind":"typed","value_type":"number","value":20}`. New memories use version 0.
Source-reference lists use the exact `asset`, `revision`, `location`, `available`
shape. `derived_content` takes content UUIDs; `supersedes` takes memory revision UUIDs.
Each list has at most 64 elements.

View variants are:

```json
{"state": "current", "revision": "<memory-revision-id>"}
```

```json
{"state": "conflicted", "revisions": ["<revision-one>", "<revision-two>"]}
```

```json
{"state": "unavailable"}
```

Basis values are `equivalent`, `complementary`, `explicit_supersession`,
`authority_rule`, and `unresolved_conflict`. Subject lookup supports current view
only; history and `before` require ID selection. Its default current read uses a
20-item bound; a custom subject-lookup `limit` is not used.

Synthesis evidence takes **content IDs**. Empty evidence invokes retrieval;
explicit evidence must fit its budget in full. Saving requires a nonempty stable
key. Transient synthesis recomputes on repeat; a key does not make it a saved answer.

### 17.5 Deletion and repair

| Tool | Required arguments | Optional/conditional arguments | Application result |
| --- | --- | --- | --- |
| `context_purge` | `target:{kind,id}`, `idempotency_key` | `expected_generation` **required for asset, revision, memory** | `purge`, `state:"pending"`, `completed:false` |
| `context_purge_status` | `purge` | None | `purge`, `state` pending/completed, `completed` |
| `context_rebuild_index` | `workspace` | `space` | `workspace`, `space`, `indexed` |

Purge target kinds are `tenant`, `workspace`, `collection`, `asset`, `revision`,
`content`, `memory`. For memory, `expected_generation` is its logical version;
for revision, it is parent asset generation. Purge requires owning-tenant membership
and Purge authority. Rebuild requires owner Share and every contributing source's
Read authority. Rebuild is synchronous, with no job receipt or idempotency argument.

## 18. CLI reference

There are **12 subcommands**. Use `context-mcp --help` or
`context-mcp SUBCOMMAND --help` for the binary's current help. Flags belong to the
subcommand; there are no global config/token flags.

| Command | Required flags | Optional flags / defaults | Purpose |
| --- | --- | --- | --- |
| `serve` | `--config PATH` | `--transport http\|stdio` (http) | Start the complete single application |
| `catalog` | None | `--transport http\|stdio` (http) | Print exact application catalogs offline |
| `discover-providers` | `--config PATH --output PATH` | None | Discover selected live provider descriptors into a new catalog file |
| `provision-data` | `--config PATH` | None | Explicitly provision/verify the configured unversioned bucket |
| `migrate` | `--config PATH` | None | Apply embedded PostgreSQL migrations |
| `bootstrap` | `--config PATH --issuer VALUE --subject VALUE --label VALUE` | `--kind service` | Create an initial hierarchy/grant for a resolved identity |
| `register-identity` | `--config PATH --issuer VALUE --subject VALUE` | `--kind human` | Register/resolve exact identity, without resource grants |
| `membership` | `--config PATH --principal UUID --resource UUID` | `--remove` | Add/remove tenant or workspace membership |
| `issue-credential` | `--config PATH --principal UUID --scope VALUE --output PATH` | Repeat `--scope`; `--lifetime-seconds 86400` | Write a new scoped service credential once |
| `revoke-credential` | `--config PATH --principal UUID --credential UUID` | None | Revoke an owner-bound credential ID |
| `call` | `--tool NAME` | Connection flags below; `--arguments PATH` (`-`, stdin) | Invoke any public tool with one JSON argument object |
| `upload` | `--file PATH --collection UUID --idempotency-key VALUE` | Connection flags below; `--profile NAME`, `--media-type TYPE`, `--wait` | Stream/hash/create/finalize a new asset, optionally wait |

Common `call`/`upload` connection flags:

| Flag | Default / meaning |
| --- | --- |
| `--endpoint URL` | `http://localhost:8089/mcp` |
| `--token-env NAME` | `CONTEXT_TOKEN`; variable contains token value, **not a file path** |
| `--allow-http` | Explicitly permit a plain-HTTP endpoint; not restricted to loopback by code |

`call` accepts a JSON object up to 1 MiB, from stdin or an arguments file. Example:

```sh
"$CONTEXT_MCP" call \
  --endpoint "$CONTEXT_ENDPOINT" \
  --tool context_query \
  --arguments /absolute/path/query.json
```

There is no built-in login/token-refresh command, credential-list command, resource
read CLI subcommand, drain, backup, restore, or upgrade subcommand. Use an MCP client
for `resources/read`, or use the corresponding tool operation through `call`.

Identity kinds are `human` and `service`. Valid issuance scopes are exactly `read`,
`ingest`, `write_memory`, `synthesize`, `share`, `purge`. Credential lifetimes are
1–7,776,000 seconds. Output must be a new file; failed output creation/write revokes
the just-issued credential. No secret is printed.

Offline operator commands have direct trusted configuration/database access; they
are not ordinary remotely callable tools. Run them only in the intended operator
environment. Database-backed commands also run migrations on connect.

## 19. Resources, pointers, permissions, and limits

### 19.1 MCP resources and auxiliary HTTP routes

| Resource template | Meaning |
| --- | --- |
| `context://content/{id}` | Same protected content expansion as a content pointer |
| `context://asset/{asset}/revision/{revision}` | Exact published source-revision expansion |
| `context://memory/{id}` | Current logical memory view, not its full history |

All templates return MIME type `application/json`. MCP `resources/read` wraps the
encoded JSON as text in `contents`, unlike tool `structuredContent`. Responses
are bounded to 1 MiB. There are no listed resources or prompts; discover templates
and use IDs from authorized operations. Resource URIs reject userinfo, ports,
queries, and fragments.

For HTTP integrations, these authenticated routes use the same application services:

```text
POST /uploads                       create-upload argument object
POST /uploads/{upload}/complete      {"idempotency_key":"…"}
GET  /jobs/{job}                     creator-owned job status
```

They do not provide a general REST alternative for every tool. Actual file bytes
are PUT to the returned signed object-store URL.

### 19.2 Pointers and source locations

`context_expand` accepts one of these pointer objects:

| `kind` | Other required fields |
| --- | --- |
| `content` | `content` UUID |
| `asset` | `asset`, exact `revision` |
| `page` | `asset`, exact `revision`, one-based `page` |
| `media` | `asset`, exact `revision`, typed `location` |
| `entity` | `entity` content UUID |

Example page/media argument forms:

```json
{
  "pointer": {
    "kind": "page", "asset": "<asset-id>",
    "revision": "<asset-revision-id>", "page": 1
  }
}
```

```json
{
  "pointer": {
    "kind": "media", "asset": "<asset-id>", "revision": "<asset-revision-id>",
    "location": {"kind": "time", "start_ms": 1000, "end_ms": 2000}
  }
}
```

Source locations are distinct from pointer types:

| `location.kind` | Fields and units |
| --- | --- |
| `asset` | Entire exact source revision |
| `file` | Relative archive-member `path`, not a host filesystem path |
| `pages` | `first`, `last`, inclusive, one-based |
| `time` | `start_ms`, `end_ms`, half-open milliseconds; end greater than start |
| `region` | `x`, `y`, `width`, `height`, upper-left pixels with positive dimensions |
| `bytes` | `start`, `end`, half-open byte offsets; end greater than start |

File paths reject absolute paths, backslashes, empty components, `.` and `..`, and
are bounded to 1,024 bytes. Do not substitute a current revision into a historical
source reference. Page/media expansion returns evidence and an `extract` request
description; add an idempotency key before submitting that explicit extraction.

### 19.3 Permission reference

These are capability requirements, not automatic grants. Live source authority,
creator ownership, and owning-tenant membership can impose additional restrictions.

| Permission | Typical operations |
| --- | --- |
| `read` | Query, expand, resources, history, read memory, media extraction; status can also accept Ingest |
| `ingest` | Create/finalize upload, text/URL ingestion, reprocess, ordinary ingestion cancellation/retry |
| `write_memory` | Write/reconcile workspace assertions, with source authority for dependencies |
| `synthesize` | Generate/save cited answers, with Read on all evidence |
| `share` | Create collections, replace ACLs, rebuild workspace indexes |
| `purge` | Admit owned-target purge and inspect one's purge receipts |

ACL replacement, purge, and owner index repair require owning-tenant membership in
addition to their capabilities. Group recipient grants apply through authoritative
membership. Membership removal does not erase direct principal grants.

### 19.4 User-facing limits

String limits below often count UTF-8 **bytes**, while JSON Schema string lengths
can be expressed as characters. Non-ASCII input may therefore hit a runtime bound
earlier than a schema-only check suggests. Limits are independent: a request can
fit its own operation limit and still exceed a lower operator-configured transport
or processing limit.

| Item | Implemented bound / default |
| --- | --- |
| Inline text | Nonempty, at most 262,144 UTF-8 bytes |
| General idempotency key | Nonempty, at most 256 bytes |
| Text/URL/reprocess key | At most 200 bytes |
| URL input | At most 8,192 bytes; exact allowed resource |
| Query text | Nonblank, at most 4,096 bytes |
| Query evidence count | Default 20; 1–100 |
| Query token budget | 1–65,536; internal 65,536 ceiling without supplied budget |
| Query selected text | At most 262,144 bytes |
| Initial query candidates | At most 10,000; larger scope rejected |
| Asset history page | Default 20; 1–100 |
| Asset expansion child pointers | At most 100 |
| Original/derived byte GET URL | 60-second lease |
| Selected media interval | At most 60,000 ms; positive half-open interval |
| Selected region | `x`,`y` below 16,384; positive width/height up to 8,192, also constrained by source bounds |
| Selected page | One page per extraction request |
| Managed byte output | At most 2 MiB |
| Reference archives | 1,000 entries and 64 MiB expanded bytes |
| Reference media duration | At most 3 hours; segment/time/output bounds still apply |
| Memory subject / description | 512 / 8,192 bytes; subject nonblank, description may be empty |
| Serialized memory value | At most 32,768 bytes including its tagged structure |
| Memory reference/supersession lists | At most 64 per list |
| Memory history page | Default 20; 1–20 |
| Synthesis question | Nonblank, at most 4,096 bytes |
| Explicit synthesis evidence | At most 64 unique content IDs |
| Synthesis evidence budget | Default 8,192; 1–16,384 tokens |
| Synthesis answer | Nonempty, at most 32,768 bytes; 1–64 validated citations |
| Explicit ACL grants | At most 128; each permission list nonempty |
| Resource response | At most 1 MiB encoded JSON |

### 19.5 Reference settings versus configurable ceilings

The middle column comes from the checked-in development configuration/generator.
It is not a promise that every installation uses that value.

| Setting | Reference value | Configuration bound / omission behavior |
| --- | --- | --- |
| Upload size | 1 GiB | At most 5,000,000,000 bytes |
| Upload lease | 900 seconds | At most 900 seconds |
| Unfinished jobs | 1,024 | At most 4,096, including pending cleanup |
| Processing workers | 2 | 1–16 |
| Extraction segments | 1,000 | 1–10,000 |
| Chunk bytes | 4,096 | 128–8,192 |
| Embedding dimensions | 256 | 1–4,096; at most 16 configured spaces |
| Default profile | `multimodal` | Operator-selected name |
| Default embedding space | `text-embedding-3-small:256:v1` | Operator-selected compatible identity |
| Token encoding | `cl100k_base` | Omission defaults to this; alternative `o200k_base` |
| URL hosts | Empty | Omission disables acquisition |
| URL development HTTP | Disabled | Omission false |
| Request body/frame | 1 MiB | 16 KiB–4 MiB |
| HTTP request deadline | 180,000 ms | Omission 180,000; 1,000–600,000 |
| HTTP concurrent requests | 128 when omitted | 1–1,024 |

Environment references used by the generated setup include:

| Reference | Consumer |
| --- | --- |
| `CONTEXT_DATABASE_URL` | Application/database-backed operator commands |
| `CONTEXT_CONTAINER_DATABASE_URL` | Compose interpolation into application `CONTEXT_DATABASE_URL` |
| `CONTEXT_S3_ACCESS_KEY`, `CONTEXT_S3_SECRET_KEY` | Application S3 client and reference MinIO |
| `CONTEXT_TYPESENSE_KEY` | Application index client and reference Typesense |
| `OPENAI_API_KEY` | Reference model subprocess, inherited through its binding |
| `ALLOW_DIRS`, `SANDBOX_PROFILES_DIR`, `DOCKER_HOST` | Reference sandbox subprocess |
| `CONTEXT_STDIO_TOKEN` | Server stdio launch credential |
| `CONTEXT_TOKEN` | Default HTTP CLI credential |
| Configured `client_secret_env` | OIDC introspection secret, when selected |

Environment-variable names in JSON are references, not secret values. Arbitrary
host variables are not automatically forwarded into Compose or provider subprocesses.
Provider bindings explicitly choose inheritance.

## 20. Glossary and further reading

| Term | Meaning |
| --- | --- |
| ACL | Explicit resource grants plus an inheritance choice |
| Admission | Durable acceptance of work; not completion or publication |
| Assertion | Explicit text or typed semantic-memory claim |
| Citation | Exact evidence support, distinct from declaring informational derivation |
| ContextMap | Ranked selected evidence with provenance, pointers, token accounting |
| Derivation | Informational dependency that carries source protections and transitive purge |
| Embedding space | Named vector/model semantics and dimensional contract |
| Fence | State/write barrier preventing stale work from publishing or restoring content |
| HTLK | Embedded compiler/runtime that dispatches MCP interactions through operator workflows |
| Idempotency | Recovering the same operation's receipt without duplicating its intent |
| MCP | Model Context Protocol, used by assistants/clients to discover tools/resources and invoke them |
| Origin | `original_evidence`, `agent_assertion`, or `generated` provenance class |
| Profile | Operator-selected processing roles, format support, chunking and embedding space |
| Publication | Atomic visibility of a complete successful revision/result |
| Reconciliation | Comparing/proposing memory contributions while preserving authority and conflict |
| Revision | Immutable version of a source or assertion |
| Scope | Credential capability ceiling, or an explicitly selected query hierarchy; context determines which |
| Synthesis | Explicit generation of a cited answer from accessible evidence |

For deeper contracts and measured evidence:

- [Application API contracts](api.md)
- [Deployment and retained-data upgrades](deployment.md)
- [Authentication](authentication.md)
- [Uploads, jobs, and recovery](ingestion.md)
- [Processing profiles and format evidence](processing.md)
- [Retrieval, navigation, history, and sharing](context-services.md)
- [Memory and cited synthesis](memory-and-synthesis.md)
- [Permanent purge and index repair](permanent-purge.md)
- [Workflow host and provider replacement](workflow-host.md)
- [Persistence](persistence.md)
- [Acceptance criteria](acceptance.md) and [verification/release evidence](verification.md)
- [Contributing and development checks](../CONTRIBUTING.md)

The specification states agreed target behavior. Acceptance and verification
documents distinguish implemented behavior, fixture coverage, actual backing-service
checks, native-format campaigns, and explicitly authorized paid provider journeys.
Use those records when evaluating a deployment rather than treating one passing
fixture or startup check as proof of arbitrary provider compatibility.
