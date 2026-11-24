# Uploads, durable jobs and processing roles

The application implements explicit upload sessions, bounded inline UTF-8 ingestion,
job status/cancellation/retry and a bounded in-process processing pool. Every provider
operation runs through the shared HTLK host. Operator graphs define extraction,
description/transcription, enrichment, embeddings and cleanup.

The service pipeline is verified against actual PostgreSQL, MinIO and Typesense with
pure processing fixtures and with a real video/model pipeline. Reference full-format
operator roles and measured matrix/provider evidence are in [processing.md](processing.md).
Earlier independent provider evidence is in [integration-spike.md](integration-spike.md).

## Public operations

Both MCP transports export exact descriptors for:

| Tool | Required input | Result |
| --- | --- | --- |
| `context_create_upload` | `collection`, `name`, `media_type`, `size_bytes`, `sha256`, `idempotency_key`; optional `asset`, `profile`, `expected_revision` (requires `asset`) | Upload/asset/revision IDs, signed PUT URL, required headers and remaining lifetime |
| `context_finalize_upload` | `upload`, `idempotency_key` | Durable asset/revision/job admission receipt |
| `context_upload_receipt` | `operation` (`create_upload` or `finalize_upload`) and the original `request`, including the key | Only operation and original upload/asset/revision IDs, plus job ID for finalize |
| `context_ingest_text` | `collection`, `name`, `text`, `idempotency_key`; optional `profile` | Same durable admission receipt |
| `context_job_status` | `job` | State, stage, redacted error class, chunk count and cleanup-pending flag |
| `context_cancel_job` | `job`, `idempotency_key` | Committed cancellation with asynchronous cleanup status |
| `context_retry_job` | Failed/cancelled `job`, `idempotency_key` | A new durable job for the retained immutable revision |

Authenticated HTTP routes use the same services:

- `POST /uploads`: the create-upload JSON object.
- `POST /uploads/{upload}/complete`: `{ "idempotency_key": "..." }`.
- `GET /jobs/{job}`: creator-owned control metadata.

Every request rechecks credential/resource authority. Creating uploads and controlling
jobs require Ingest authority; status accepts current Ingest or Read authority but is
still restricted to the submitting principal. Status never returns source bodies,
provider inputs/outputs, filesystem paths or opaque cleanup handles.

PUT the exact declared bytes to the returned URL using every returned header. The
SHA-256 header is signed. Content-Length is a client requirement, not an assertion
that the standard signer enforces length; admission independently streams/verifies
the stored length and digest. Finalization conditionally copies into a separately
reserved immutable source key and verifies that destination. Changing the temporary
upload afterward cannot change an admitted revision. The application expires tracked
temporary uploads without extending already-issued write capabilities.

Idempotency keys are principal/operation scoped. Changed input conflicts. Text
ingestion uses linked upload/finalization receipts so it can recover admission after
an acknowledgement gap, including after transfer-session expiration. A receipt for
an unrelated upload cannot satisfy that recovery. Cancelling a published success is
rejected. Retry requires settled cleanup, compatible retained role fingerprints,
available queue capacity and no active replacement for that asset. An already
published revision cannot be retried through a previous failed job.

## Publish a new version against a reviewed revision

Supply `asset` and `expected_revision` to replace bytes only if the reviewed
revision is still the live current revision of that asset. The asset must belong to
the supplied `collection`. Omit `expected_revision` for an unguarded replacement;
an explicit `null` is rejected. The application records the condition at reservation and
checks it again in job admission and in the final publication transaction. A stale
reservation conflicts without admitting a job, even if the asset generation still
matches. If the predecessor changes after admission, the worker cannot publish the
candidate; an explicit job retry retains the original condition. Processing failure
leaves the previous current revision intact. Once publication succeeds, the same
asset has a new current revision; both versions remain independently readable at
`context://asset/{asset}/revision/{revision}` with current Read authority.

These are MCP tool arguments and successful `structuredContent` bodies. In this
example the new bytes are `hello`. The response URL is a redacted example; send the
actual returned URL and headers for the PUT.

`context_create_upload`:

```json
{
  "collection": "10000000-0000-4000-8000-000000000001",
  "asset": "10000000-0000-4000-8000-000000000002",
  "expected_revision": "10000000-0000-4000-8000-000000000003",
  "name": "note.txt",
  "media_type": "text/plain",
  "size_bytes": 5,
  "sha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
  "idempotency_key": "wf01-note-v2:create"
}
```

```json
{
  "upload": "10000000-0000-4000-8000-000000000004",
  "asset": "10000000-0000-4000-8000-000000000002",
  "revision": "10000000-0000-4000-8000-000000000005",
  "url": "https://objects.example/upload?signature=REDACTED",
  "headers": {
    "content-length": "5",
    "if-none-match": "*",
    "x-amz-checksum-sha256": "LPJNul+wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ="
  },
  "expires_in_seconds": 899
}
```

After the PUT, call `context_finalize_upload`. The HTTP completion route likewise
uses the stored condition and requires no new field or header.

```json
{"upload":"10000000-0000-4000-8000-000000000004","idempotency_key":"wf01-note-v2:finalize"}
```

```json
{"job":"10000000-0000-4000-8000-000000000006","asset":"10000000-0000-4000-8000-000000000002","revision":"10000000-0000-4000-8000-000000000005","state":"accepted"}
```

An exact committed replay recovers the original receipt even after the pointer
moves. Adding, removing or changing `expected_revision` under the same create key
conflicts. Create replays can return transfer details during the original session,
or `finalized` and `job` while the finalized session remains. When no transfer
capability can be issued, a retained, accessible reservation can return only its
upload, asset and revision IDs.

## Recover a receipt after a missing response

Call `context_upload_receipt` with the original request object (including the exact
key) under its submitting principal. The tool verifies the same request identity as
admission, including normalized MIME and decoded checksum for create, and rechecks
current Ingest authority on the asset. It never renews a URL, returns transfer
headers, raw keys or bytes, admits a job, or replays the mutation.

Create lookup:

```json
{
  "operation": "create_upload",
  "request": {
    "collection": "10000000-0000-4000-8000-000000000001",
    "asset": "10000000-0000-4000-8000-000000000002",
    "expected_revision": "10000000-0000-4000-8000-000000000003",
    "name": "note.txt",
    "media_type": "text/plain",
    "size_bytes": 5,
    "sha256": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
    "idempotency_key": "wf01-note-v2:create"
  }
}
```

```json
{"operation":"create_upload","upload":"10000000-0000-4000-8000-000000000004","asset":"10000000-0000-4000-8000-000000000002","revision":"10000000-0000-4000-8000-000000000005"}
```

Finalize lookup:

```json
{"operation":"finalize_upload","request":{"upload":"10000000-0000-4000-8000-000000000004","idempotency_key":"wf01-note-v2:finalize"}}
```

```json
{"operation":"finalize_upload","upload":"10000000-0000-4000-8000-000000000004","asset":"10000000-0000-4000-8000-000000000002","revision":"10000000-0000-4000-8000-000000000005","job":"10000000-0000-4000-8000-000000000006"}
```

`unavailable` means no committed, accessible receipt was observed in the read
snapshot. This includes an in-flight commit, lost authority or removed resources.
**No committed receipt observed does not prove the first request never ran and does
not authorize an automatic retry.** For an accessible receipt, a different request
under the same key returns `conflict`. **A finalize receipt proves job admission,
not successful processing or publication.** Use `context_job_status` for progress.

### Migration and compatibility

At startup PostgreSQL migration `0010` adds nullable predecessor IDs to uploads and
jobs. Existing rows keep their prior behavior; no expectation is inferred. Migration
`0011` adds asset/revision IDs to legacy create receipts wherever their original
upload row still exists. It cannot reconstruct create IDs when that association was
already removed. New create receipts retain those IDs after transfer-row retirement
as long as the affected revision remains accessible. Finalize lookup uses the
original upload-UUID digest, so legacy finalize receipts do not need their upload
rows. Existing unguarded request digests and key hashing are unchanged. Stop the old
application before upgrading and do not run an older writer or worker against new
guarded rows. Refresh exact MCP catalog pins to use the new schema and tool.

## Operator data configuration

Add `application.data` to the operator configuration. The bucket must already exist;
startup verifies bucket access and compatible Typesense collection schemas. Credential
values are read only from the named environment variables. For example (role names
must refer to your admitted operator graphs):

```json
{
  "s3_endpoint": "http://127.0.0.1:59000",
  "s3_bucket": "context",
  "s3_region": "us-east-1",
  "s3_access_key_env": "CONTEXT_S3_ACCESS_KEY",
  "s3_secret_key_env": "CONTEXT_S3_SECRET_KEY",
  "s3_allow_http": true,
  "typesense_endpoint": "http://127.0.0.1:58108",
  "typesense_key_env": "CONTEXT_TYPESENSE_KEY",
  "spaces": {
    "document-v1": {
      "collection": "context_document_v1",
      "dimensions": 256,
      "document_role": "embed-document",
      "query_role": "embed-query"
    }
  },
  "default_space": "document-v1",
  "profiles": {
    "documents": {
      "media_types": ["text/plain", "text/markdown"],
      "extract_role": "extract-document",
      "enrich_role": "enrich-document",
      "space": "document-v1",
      "chunk_bytes": 4096
    }
  },
  "default_profile": "documents",
  "staging_directory": "../../data/staging",
  "max_upload_bytes": 1073741824,
  "upload_lifetime_seconds": 900,
  "max_pending_jobs": 1024,
  "workers": 2,
  "max_segments": 1000
}
```

The unfinished-job limit includes pending cleanup and is at most 4096. Workers are
bounded to 1–16; extraction segments to 1–10000. Uploads are bounded to 5 GB and
upload capabilities to 900 seconds. Optional profile roles are `describe_role`,
`transcribe_role`, `artifact_role`, and paired `cleanup_role`/`cleanup_node`. A cleanup
node must be an explicitly admitted checkpoint on the extraction role. Profiles and
role fingerprints are frozen at admission; changing a graph requires retaining its
old revision while unfinished jobs depend on it. Missing capabilities reject admission.

## Role documents

All roles receive `request: json` and return `result: json` objects as documented in
[workflow-host.md](workflow-host.md). Provider schema projection lives in the graph.

- **Extract:** request contains job/asset metadata, private staging `directory`,
  `source_file`, an expiring download URL, cursor and limits. The application writes
  `source.input` and `request.json` with private permissions. Return
  `{ "document": { "encoding": "json", "value": { "media_type": "text/plain",
  "segments": [{ "kind": "text", "text": "Evidence", "location": { "kind": "asset" } }]
  } } }`. `json_text` instead of `json` accepts a JSON-encoded string. Optional
  `next_cursor` pages are distinct stable stages; repeating cursors are rejected.
- **Segments:** exactly one byte source: inline text, local `file`, `data_base64`, or
  opaque `reference`. Image/audio segments use configured description/transcription
  roles. Local files must be plain private-job basenames, regular files, not symlinks,
  and carry `sha256`. Byte reads are bounded to 2 MiB per segment; text to 256 KiB.
  References are resolved only through `artifact_role`, returning `data_base64`.
- **Describe/transcribe:** image requests contain a standard `data_url`; audio
  requests contain `data_base64`. Both carry media type and original location;
  return `{ "text": "..." }`.
- **Enrich:** request contains a bounded source excerpt, name and media type. Return
  a tagged document with `summary`, `entities` (`name`/`entity_type`), and optional
  `relationships` (`from`/`to`/`relationship`). These are staged source metadata;
  generated entity/relationship records retain source protection for navigation.
- **Embed:** request contains `text`; return `{ "space": "document-v1",
  "embedding": [...] }`, with exactly the configured finite, nonzero vector dimension.
- **Cleanup:** request contains `job` and accepted opaque `checkpoint`; return
  `{ "done": true }` after the provider's successful cleanup receipt.

Extraction graphs must arrange normal-path cleanup within their own run when native
provider sessions are run-scoped. Mounted private files support durable source access;
expiring download URLs are convenience capabilities, not permanent replay inputs.
An expired provider capability can fail a resumed run; explicit retry creates fresh
stage admissions. Provider-specific cleanup behavior retains the accepted sandbox
limitation recorded in integration-spike.md.

## Publication and recovery

Each stage saves its canonical request digest and stable admission key before HTLK
admission, then saves the public run handle and clears the temporary request copy.
Normal processing resumes saved handles. Worker shutdown preserves application and
HTLK state. After all required outputs validate, chunks are staged with job ownership,
indexed, and atomically published only if the asset generation and authorization are
still valid. Readers must select authoritative published/current records. A failed
replacement preserves the previous successful revision.

Cancellation commits a new generation/state and fenced authority. At restart, terminal
unfinished jobs are restored fenced **before** the HTLK engine starts. The existing
RuntimePolicy blocks normal MCP dispatch while stable-key replay recovers missing
handles, including the admission/handle-save crash gap. All normal runs are cancelled
or confirmed terminal before only configured cleanup bindings are allowed. Cleanup
reads accepted checkpoints through existing public HTLK inspection/artifact APIs.
The host retains admission receipts for 30 days; missing-handle replay is restricted
to jobs younger than 29 days. Older unresolved intents stay fenced/pending rather
than treating an expired receipt as proof that the original run never existed.

Provider/checkpoint errors keep cleanup pending. Job ownership prevents stale cleanup
from deleting another attempt's staged content. Local staging/index cleanup completes
before authority is revoked and the job is marked settled. Workers and maintenance
share single-instance ownership so they cannot settle the same job concurrently.
HTLK execution history remains outside application purge scope.

## Reproduce the actual backing-service checks

With the disposable services documented in persistence.md running:

```sh
export CONTEXT_TEST_DATABASE_URL=postgres://context:context-test-only@127.0.0.1:55432/context_authority
export CONTEXT_TEST_S3_ENDPOINT=http://127.0.0.1:59000
export CONTEXT_TEST_S3_ACCESS_KEY=context-test
export CONTEXT_TEST_S3_SECRET_KEY=context-test-secret
export CONTEXT_TEST_TYPESENSE_ENDPOINT=http://127.0.0.1:58108
export CONTEXT_TEST_TYPESENSE_KEY=context-test-only-key
cargo test -p context-engine --test ingestion --locked -- --ignored --skip actual_video --test-threads=1
cargo test -p context-mcp --test transports --locked -- --ignored
```

Five ingestion tests cover publication, failed replacement, owned cancellation,
job-scoped staged cleanup, explicit/concurrent retry, capacity, acknowledgement-gap
recovery, accepted-job reopen, and failed-job restart with admitted/never-admitted
missing handles, expiry-window refusal, and unconfirmed cleanup retaining its fence
and refusing retry.
The binary test covers both transports, upload/finalization REST
and MCP paths, text ingestion/status, isolation/revocation/idempotency, and fenced
replay with zero external collection mutations. Processing fixtures are pure graphs;
these checks do not establish full-format or model-provider compatibility.
