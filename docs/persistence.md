# Application persistence

## PostgreSQL authority

`context-storage` embeds checked migrations for identities, the immutable resource
hierarchy, memberships and grants, hashed service credentials, assets/revisions,
managed content and derivation edges, memory revisions, uploads, jobs, idempotency
receipts and content-free purge worklists. HTLK records stay in its own SQLite store.

The pool explicitly fixes `search_path=public` so SQLx's migration ledger does not
move when the login role and application schema have the same name. Application
tables are explicitly qualified under `context`. Fresh and reopened databases were
verified, including concurrent initial migration callers in the test suite.

Resource identity, owner and ancestry are immutable. Parent types/owners and
purging-ancestor admission are checked by PostgreSQL. Source/memory revisions reject
updates. Sharing modifies explicit grants, not ownership.

Repositories are trusted infrastructure. `authorized_read`/`authorized_write`
resolve credential state, membership and grants in the same protected-data snapshot.
Standalone resource-chain/membership hydration is useful for diagnostics, not a
replacement for this transactional boundary. The request service still authenticates
each operation and applies its action-specific invariants. See authentication.md.

`existing_receipt` locks the principal/operation/key before effects. The transaction
then performs the authoritative mutation and stores its receipt. Reuse with another
digest conflicts. `claim_job` uses row locking for bounded workers inside the one
application. `publish_revision` requires the matching generation, actual revision
parent and live ancestor chain; success makes staged revision bodies visible.
Application services still coordinate physical/index writes and purge worklists.

Errors redact backend details in Display/Debug. Typed causes exist for deliberate
trusted diagnostics. Credentials and connection strings do not belong in logs.

## Real database verification

The actual PostgreSQL tests require an isolated disposable database. They are
explicitly ignored in ordinary unit-only runs, rather than silently succeeding
when no database is available:

```console
CONTEXT_TEST_DATABASE_URL=postgres://context:context-test-only@127.0.0.1:55432/context_authority cargo test -p context-storage --test postgres -- --ignored
```

The password above is a public disposable-test fixture, not a production credential.
The tested image was PostgreSQL 16, digest
`sha256:9e2cf6287f25c3151f4c9421ccce2b17f3e60061c111a29165e0d2cdd9ff0a97`.

Five actual-database scenarios passed:

- Inherited cross-tenant grants intersect credential scopes; stopping inheritance
  denies access and preserves the source tenant's ownership.
- Owner/ancestor changes fail at the database and ordinary error formatting is redacted.
- Stale generation, purging target and purging ancestor cannot replace a successful
  current revision or expose the staged replacement.
- Concurrent first admissions return one receipt; changed requests conflict.
- Concurrent worker claims return distinct pending jobs exactly once.

## S3-compatible payloads

`Blobs` uses an existing operator-provisioned bucket, explicit credentials,
path-style addressing and bounded requests. The application supplies generated
keys. Create-only writes use the S3 conditional-write contract and cannot silently
overwrite an immutable object. Small reads check advertised and streamed sizes;
large source downloads stream into newly created private files while checking the
exact size and SHA-256. A mismatch removes the partial file. Upload/download URLs
are explicitly authorized by the caller and limited to at most 15 minutes.

Upload promotion uses standard S3 CopyObject with a source ETag precondition into
a separately reserved non-client-writable key. Destination bytes are streamed
through SHA-256 verification before admission, so mutable upload keys or ETag
collisions cannot change the accepted source. Identical interrupted promotion
reuses a verified destination. Embedded XML copy errors and checksum mismatch fail;
mismatched newly copied destinations are removed. This avoids buffering large
sources or creating multipart-upload orphans.

Real MinIO acceptance covered conditional writes, signed PUT/GET transfers, bounded
reads, streaming verification, removal of mismatched staging, and idempotent purge.
The local server was MinIO DEVELOPMENT.GOGET, image
`sha256:d84beaf11d584d65346338ed90958ddae511db3c5841186e1d93a34a40b9a8fa`.
This is measured protocol evidence, not a portable release-image claim. Production
uses operator-selected S3 infrastructure; portable development provisioning is a
delivery task.

## Derived Typesense index

`Search` creates/checks its configured vector dimension and fields, bounds text and
vectors, rejects nonfinite values, and upserts deterministic content IDs. Hybrid
queries require an explicit bounded batch of authorized, published content IDs.
Empty permission sets return no results. Returned IDs are rechecked against that
allowlist; bodies must be hydrated from PostgreSQL. The client does not expose
unauthorized totals or treat index body copies as authoritative evidence.

The real Typesense 29.1 acceptance test covered collection creation/reopening,
hybrid matching with an equally relevant unauthorized document, empty scope,
dimension mismatch and repeated deletion. Image digest:
`sha256:2d9a7e348ce0de03a6061291023c54f83c47f0fa81f26b1b0d1110286e415b0c`.

Run the two external-service tests with explicit isolated service endpoints:

```console
CONTEXT_TEST_S3_ENDPOINT=http://127.0.0.1:59000 CONTEXT_TEST_S3_ACCESS_KEY=context-test CONTEXT_TEST_S3_SECRET_KEY=context-test-secret CONTEXT_TEST_TYPESENSE_ENDPOINT=http://127.0.0.1:58108 CONTEXT_TEST_TYPESENSE_KEY=context-test-only-key cargo test -p context-storage --test backing_services -- --ignored
```

These credentials are public disposable-test values. Tests create/delete their own
random bucket/collection. Full application authorization, batching across large
authorized scopes, embedding-space/model identity, transactional publication and
recovery/purge coordination remain service/application responsibilities.
