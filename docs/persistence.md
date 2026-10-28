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

Repositories are trusted infrastructure, not an authorization API. A caller must
establish identity and authorize the mutation. Resource-chain and membership reads
are available for domain policy hydration; the request service must use a consistent
transaction for permission facts and protected data. These primitives alone do not
establish the finished HTTP/MCP authentication boundary.

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

S3-compatible object storage and Typesense client implementation/verification are
the next persistence subtask. Full application recovery and purge are later gates.
