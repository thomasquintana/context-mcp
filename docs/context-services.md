# Retrieval, history, navigation and live sharing

## Context maps

`context_query` takes `scope` (`{kind,id}`), `query`, optional `limit` (1–100),
`token_budget` (1–65536), `space`, and explicit `historical` opt-in. It returns a
versioned context map with ranked evidence, origin classes, exact source references,
typed content/asset/entity pointers, token accounting and truncation status.

Each candidate and every transitive derivation dependency must be currently readable.
Only authorized IDs are passed to Typesense, in batches of at most512. A request is
bounded to10000 candidates and rejects larger scopes rather than reporting hidden
hit counts. After external query embedding/index calls, the application reauthorizes
and hydrates authoritative bodies in one new snapshot, rechecks current revision/view,
and ranks only those surviving readable records. Embeddings/index scores are never
returned as evidence. Cross-tenant assets retain their original owner; query an
explicitly shared asset scope when its enclosing project is not shared.

Final ranking uses reciprocal-rank fusion (`k=60`) of lexical term overlap and cosine
similarity over compatible vectors, with UUID tie-breaking. Provider raw scores from
unrelated sources are not compared. Exact duplicate text is selected once. Defaults
select successful current asset revisions and current memory views; retained history
requires explicit opt-in.

`application.data.token_encoding` selects `cl100k_base` (default) or `o200k_base`.
The real Rust BPE tokenizer counts selected evidence. Truncation retains valid UTF-8
and stays within the requested token ceiling. Independent evidence-count, byte and
default token bounds still apply. This accounts for returned evidence, not provider
billing or the entire surrounding MCP response.

Query embedding runs on a bounded Actix-owned request queue using unchanged public
HTLK admission. Synchronous request computations do not masquerade as accepted
asynchronous jobs. Their source authority is checked before execution and delivery;
their grants are revoked/released after completion and never restored after restart.
Durable ingestion/media jobs continue to use authoritative persisted grants/handles.

## Exploration and immutable history

- `context_expand` accepts the typed `pointer` returned by a context map. Content
  expansion strips vectors/private object keys. Asset expansion returns exact
  published revision metadata, bounded pointers and a60-second original-byte URL.
- Entity pointers expose generated entity names/types and relationships, retaining
  source lineage and current authorization checks. Generated enrichment has an
  explicit generated origin; it is not original evidence.
- Page/media pointers return situated evidence and an explicit binary-extraction
  request description; they do not silently relabel another revision as the source.
- `context_asset_history` takes asset, optional `before` revision and `limit` and
  lists only published immutable revisions, current markers and exact pointers.
- `context_reprocess` takes asset, `expected_revision`, optional profile and stable
  idempotency key. It conditionally copies/verifies the retained original into a new
  immutable source revision and submits ordinary processing. Finalization checks the
  expected current revision under the mutation lock. Linked receipts bridge crashes;
  failed replacement retains prior successful history/current evidence. Remote
  refresh remains explicit exact-URL ingestion with a replacement asset.

## Binary page/region/clip materialization

`context_extract_media` takes asset, exact revision, `location`, optional profile and
idempotency key. Locations are one PDF page, a validated image rectangle, or an
audio/video interval of at most60 seconds. The selected profile must declare
`media_role` and its required capabilities; the reference profile uses the extraction
graph with the helper's explicit render action.

The tool returns a durable media-job receipt. Poll `context_job_status`; success
includes `result.content`. Expand that content pointer for protected metadata and
a60-second byte URL. Outputs are independently SHA-256 verified and at most2 MiB.
The original asset/current revision is never replaced. Derived bytes carry their
exact source coordinates and derivation edge, are generation/state fenced, and are
purge-addressable. Pending staging/objects are tracked before physical writes.
Read-authorized cache materialization does not grant arbitrary user knowledge writes.

The reference helper renders bounded PDF images, exact PNG crops, mono WAV selections
and bounded-resolution H.264/AAC MP4 clips. Actual HTLK+sandbox selection checks pass
for a PDF page,16x16 crop, one-second video and one-second audio, including independent
dimensions/duration/digest checks. Evidence:
`target/providers/formats-o4lwuk8x/evidence.json`, image
`sha256:707838fe31b4f8584ddb1f9e91c7fcef84cf7c760269e1262528c1b9c98b5407`.

## Sharing and revocation

`context_set_access` replaces a complete explicit grant list and inheritance flag
under `expected_acl_version`. It requires Share authority plus membership in the
owning tenant, validates recipient identities and advances the ACL version atomically.
Recipients can be principals or tenant/workspace memberships. Empty grants revoke
explicit sharing. Ownership and content identity do not change.

Every later query, expansion, source read and derivative publication checks current
authority; saved derivatives also require all transitive source protections. Issued
byte URLs are short-lived capabilities, valid for at most the declared60-second lease.

## Evidence

Nine nonpaid backing-service ingestion/application tests pass, including real hybrid
search, BPE budgets, entity navigation, revocation, reprocessing/history, live
cross-tenant sharing and durable binary materialization. The pure render fixture
checks storage/lifecycle behavior; the separate actual sandbox selection tests prove
the reference native operations. The actual HTTP/stdio binary test passes query,
budget, expansion and history. Its stdio roundtrip uses one existing HTLK loop to
retain the run-owned provider session across admission, polling and retrieval.
