# Permanent purge and derived-index repair

`context_purge` accepts a typed target (`tenant`, `workspace`, `collection`, `asset`,
`revision`, `content`, or `memory`), expected asset generation/memory version where
applicable, and a stable idempotency key. It requires current Purge authority and
membership in the owning tenant. A foreign recipient cannot purge the original even
if a sharing grant accidentally includes Purge. Dependency removal is not restricted
to the initiating principal's readable subset.

The returned purge receipt is asynchronous. `context_purge_status` is creator-owned,
requires a currently valid Purge-scoped credential and remains readable after the
target is gone. It returns pending/completed control state without foreign derivation
counts or source content. Replaying an old receipt cannot regenerate removed data.

## Admission, settlement and physical deletion

Admission holds the single-instance publication fence and authoritative target lock,
checks the expected version/generation, and persists content-free resource/content/
object/job/acquisition worklists. SQL computes the complete derivation closure across
owners and all immutable revisions. Mixed-source records are selected in full.
Visibility, source reuse and new publication close immediately when admission commits.
The operational purging state is inaccessible pending cleanup, not a recoverable
soft-delete view.

All affected durable jobs are cancelled/fenced and retain cleanup authority until
their configured accepted-handle cleanup settles. Transient request grants are
cancelled/fenced when their input IDs intersect the deletion closure. Restart restores
fenced authority before engine startup and resumes the same worklists. Existing HTLK
admission/inspection/cancellation interfaces handle unresolved stage intents; no
private runtime SQL, adapter or toolkit change is used.

Only after writers settle does cleanup delete S3 payloads, retained compatible index
entries, private job directories, acquisition files and authoritative bodies/history.
Index binding metadata retains old managed collection locations so changing an index
binding cannot silently skip deletion from an older collection. Cleanup is idempotent;
failed provider/backing-service work remains pending and is retried fairly. Unpublished
failed memory/synthesis stages have an operational orphan janitor. Completion clears
worklists and retains only generated IDs, state, counts and opaque intent digests.
Caller-chosen idempotency keys are stored as SHA-256 handles, not raw human text.

Citation-only independent assertions survive with unavailable support. A logical
memory with surviving history but no surviving current contribution reports an
`unavailable` current view; history is explicit. No older claim is silently promoted
and no mixed artifact is automatically regenerated. A logical memory with no remaining
revisions is physically removed. Reprocessing copies retain derivation lineage.

## Approved direct-upload fence

URL expiry alone cannot settle a direct PUT begun before expiry. The user approved
standard conditional PUT fencing while retaining direct-to-S3 uploads:

- Every application-issued upload URL signs `If-None-Match: *` and its SHA-256 header.
- Retiring an upload atomically replaces its bytes with a zero-byte object at that
  opaque generated key. Late conditional commits cannot overwrite that fence.
- These permanent zero-byte objects are **content-free operational fences**, not
  retained payloads or a soft-delete representation. Never remove them from an active
  application's bucket or reuse retired upload keys. Disposable test buckets remove
  their own fences only during explicit test teardown.
- A retry PUT can return412 because its earlier attempt already committed; clients
  should finalize, which independently verifies size/hash. Text/URL ingestion handles
  that receipt gap without accepting mismatched bytes.

Only unversioned buckets are accepted. Startup and purge cleanup verify the standard
S3 versioning configuration and reject Enabled/Suspended history. The reference
deployment configures no payload replication/backups; storage/bucket migrations or
operator-added copies must preserve the same deletion policy. This is deletion of
managed records/objects, not a disk-forensic secure-erasure claim.

The accepted reference sandbox limitation remains in integration-spike.md: use its
reported destroy receipt and retain the measured provider bug. Physical provider
container lifetime and HTLK execution history remain outside application purge scope.

## Explicit index repair

`context_rebuild_index` takes workspace and optional compatible space. It requires
owner Share authority and read access to every contributing source. It recreates a
missing compatible collection, clears that workspace's stale index entries, and
upserts existing published authoritative text/vectors. It does not rerun models,
recreate source bytes, or regenerate purged knowledge. Authoritative query hydration
continues to enforce current permissions/publication.

## Observed evidence

Thirteen nonpaid application tests pass against PostgreSQL/MinIO/Typesense. The purge
case restarts after admission, settles an unstarted replacement, removes mixed-source
memory history and saved/foreign derivatives, preserves an unrelated original and
independent citation-only claim, verifies actual payload/index deletion, rejects a
late signed PUT and rebuilds a damaged index. Ordinary upload/recovery tests continue
to pass with conditional PUTs and hashed receipt keys.

The actual S3 test pauses a streaming PUT, installs the fence, resumes the body and
verifies no payload remains and another signed PUT is rejected. It also enables
versioning on an empty disposable bucket and verifies readiness rejection. Nine actual
storage tests,21 regular tests, strict Clippy/fmt/rustdoc and HTTP/stdio binary checks
pass. Pure model roles in these application tests do not substitute for the separate
live model/provider verification.
