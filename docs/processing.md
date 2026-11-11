# Processing profiles and measured format support

The reference operator graphs compose actual `mcp-sandbox` and `llm-mcp` through
unchanged HTLK. Every provider name/schema appears in these graphs/catalogs and
deployment configuration, not application domain code. Replace providers by supplying
equivalent role graphs with the documented application contract.

## Offline native processing

Build from the repository root and create a pinned profile:

```sh
docker build --tag context-mcp-media:processing --file deploy/media/Dockerfile .
python3 tools/processing_profile.py data/profiles/context.yml
export ALLOW_DIRS="$PWD/data/staging"
export SANDBOX_PROFILES_DIR="$PWD/data/profiles"
export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
```

The helper `context-process` is Rust, runs inside the configured sandbox, has no
network client, and never executes source/repository programs. Native FFmpeg and
Poppler are fixed argv subprocesses in that sandbox. Each sandbox mounts only its
private job directory, with networking disabled, a fixed UID/GID, CPU/memory/PID
bounds and a 120-second command budget. Profile creation records the real local image
digest; the runtime does not trust a mutable tag. Full image package versions are
reported by the image's Debian package inventory.

| Formats | Processing and source coordinates |
| --- | --- |
| TXT/Markdown/CSV/JSON/source files | Bounded UTF-8/UTF-16, JSON validation, situated text segments; exact byte positions for unchanged UTF-8 |
| HTML | Extract text/entities, omit script/style; do not fetch images, styles, links or other subresources |
| PDF | Poppler text per page; image rendering for pages without text |
| DOCX/PPTX | Rust ZIP/XML text and embedded image extraction; exact archive-member pointers |
| XLSX | Shared/inline strings and cached cell values per worksheet; no formula/code execution |
| EPUB | Offline XHTML and embedded images; archive-member pointers |
| JPEG/PNG/WebP/GIF/TIFF | Signature verification and bounded raster; representative first frame for animated/multipage image files |
| MP3/WAV/M4A/FLAC/Ogg | Container verification; mono 16-kHz WAV segments of at most 30 seconds |
| MP4/MOV/WebM/MKV | Container verification; frame samples every 10 seconds and audio segments, retaining original time intervals |
| Explicit repository ZIP/TAR/TAR.GZ | Bounded text members with original member paths; no extraction to caller-chosen paths or program execution |

Archives are limited to 1000 entries and 64 MiB expanded bytes. Absolute/traversal
paths, symlinks, special TAR entries and external XML document types are rejected.
Text is segmented at UTF-8 boundaries; individual managed byte outputs are limited
to 2 MiB. Media duration is at most three hours and the combined segment limit still
applies. Operator/application upload/segment/time limits can reject larger inputs;
such rejection does not publish a partial revision.

`extract.htlk` checks command exit status and normal destroy receipt. Its declared
`create` checkpoint permits the approved interrupted-cleanup flow. `describe.htlk`
uses a standard image data URL; `transcribe.htlk` uses bounded audio bytes;
`enrich.htlk` returns a structured summary/entity document; `embed.htlk` declares its
model/space/dimension. Prompts/models remain operator choices.

## Exact URL ingestion

`context_ingest_url` takes collection, optional replacement asset, name, URL, declared
media type, optional profile and idempotency key. Set `application.data.url_hosts`
to explicitly allowed hosts. Empty disables acquisition. HTTPS is required unless
`url_allow_http` is enabled for an explicitly allowed development host. Userinfo and
fragments are rejected; DNS resolution is pinned for the request. Redirects are not
followed. Only the exact response body is streamed, bounded by `max_upload_bytes`,
hashed, and admitted through immutable upload promotion. No remote subresources are
fetched. The source URL provenance omits query strings; full intent comparison uses
a digest. Interrupted acquisitions are tracked for restart/expiry cleanup. Linked
receipts recover already accepted jobs without fetching a mutable URL again.

## Measured evidence

- On 2026-11-11, `python3 tools/format_acceptance.py` passed all 28 format cases and
  four exact media-selection cases with HTLK `1e405c49`, the updated extraction
  graph and unchanged processing image
  `sha256:707838fe31b4f8584ddb1f9e91c7fcef84cf7c760269e1262528c1b9c98b5407`.
  Evidence: `target/providers/formats-hnl2i5wt/evidence.json`. All per-case checks
  and the campaign command completed; no model calls were made. Native destroy
  receipts retain the documented provider boundary.
- `tools/format_acceptance.py` ran 28 synthetic format cases through the actual
  sandbox and compiled reference extraction graph, checking independently known
  text/segment classes, exit/destroy receipts and SHA-256 of managed outputs.
  Earlier delivery evidence: `target/providers/formats-e5psibk0/evidence.json`; image
  `sha256:7aac0dea0dd5c04ec6906b2ba760bdea39bdd550ebcaca2b6f01fc8ddad35803`.
  The command wrapper reached its overall timeout after writing completed evidence;
  all 28 individual HTLK executions and assertions completed. No model calls in
  this matrix test. It is bounded format-fixture evidence, not every possible file.
- Actual production-host application test
  `actual_video_model_workflows_publish_independently_checked_evidence` passed using
  real PostgreSQL/MinIO/Typesense, mcp-sandbox, `gpt-4.1`, `whisper-1`, and
  `text-embedding-3-small`. It ingested a known blue video with synthetic speech,
  checked published evidence containing blue/ocean, finite vector256 values and
  the actual index count. That run used the preceding processing image from
  `formats-3qqtz4uu`; later helper changes add validation/mixed-page handling and
  were rechecked by the full native format matrix. The original combined paid
  delivery E2E is retained in verification.md alongside the new upgrade campaign.
- After explicit authorization on 2026-11-11, that paid Rust case passed again on
  the upgraded runtime with the current processing image. It checks known blue/ocean
  evidence, finite 256-dimensional embeddings and exact index contents. A stale
  test-only attempt to reuse its empty 3-D fixture collection for the real model
  space was corrected to use a distinct collection, preserving the application's
  retained index-identity checks. The fresh paid HTTP/stdio container campaign also
  passed; evidence is `target/deployment/verified-ozka9ng1/evidence.json`.
- Six ordinary backing-service ingestion tests pass, including exact URL egress,
  redirect refusal and receipt recovery; these use pure processing roles.
- Normal tests separately reject archive traversal/excess members, prove source
  scripts are not executed, preserve UTF-8 ranges and verify entity decoding/script
  omission. Fixture-only tests are not counted as live-provider compatibility.

Run ordinary ingestion tests without the opt-in paid provider case:

```sh
cargo test -p context-engine --test ingestion --locked -- --ignored --skip actual_video --test-threads=1
```

The paid provider case additionally requires the configured binaries, actual catalog
snapshot, model credential inherited by environment, fixture video, processing profile
and `CONTEXT_TEST_WORKDIR` under `ALLOW_DIRS`. Run only its explicit test-name filter.

```sh
cargo test -p context-engine --test ingestion actual_video_model_workflows_publish_independently_checked_evidence --locked -- --ignored --exact --test-threads=1
```
