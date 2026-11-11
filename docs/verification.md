# Verification and release evidence

## Reproduce strict and actual-service checks

Pinned tools: Rust1.98.0, cargo-llvm-cov0.9.1, cargo-deny0.20.2 and cargo-about0.9.2.
No model credential is required for the nonpaid verification campaign:

```sh
cargo build --workspace --all-features --locked
python3 tools/audit_release.py
python3 tools/test_services.py
python3 tools/verify.py --coverage
python3 tools/notices.py --check
python3 tools/release.py
python3 tools/test_services.py --stop
```

The disposable stack uses actual PostgreSQL, pinned-source MinIO, Typesense and
Keycloak, plus a SHA-256-verified unchanged sandbox binary and HTLK-discovered catalog.
Fixture service credentials are public development values. Configure the
`CONTEXT_TEST_*` references to use existing isolated services; the scripts do not
print token/model-secret values. Paid cases are explicitly excluded by name.

The 2026-11-11 upgrade campaign passed 52 Rust tests: 25 regular tests and 27 opt-in actual
service/transport tests (including 13 application campaigns, 10 storage tests, 2 identity
tests, the native-host sandbox test and actual binary HTTP/stdio campaign). Additional
checks passed fmt, strict all-target Clippy, strict rustdoc, advisory/license/source/
dependency policy, the independent coverage-gate test, notice generation, core package
archive verification and locked release compilation. CI workflow syntax was checked
with actionlint1.7.12 during the original delivery. GitHub CI execution for this
upgrade occurs after push and is distinct from these local results.
The initial hosted coverage runs exposed the transport test's roughly ten-second
startup allowance; its follow-up uses a finite 60-second wall-clock deadline while
retaining actual readiness/process-exit assertions. Local targeted and full
coverage campaigns pass with that test-only change; consult PR checks for the
separately running hosted verification.

## Embedded HTLK upgrade

The verified dependency graph pins HTLK `1e405c495fc2e9b3b3fb812123f509178b08a9e5`,
SQLx `10462fa4414c6c7f1ed1d39bb31678e3b726878c` and rmcp 3.9.0. HTLK resolves
rusqlite 0.41.1/libsqlite3-sys 0.39.1 with patched bundled SQLite. SQLx remains the
standard facade with its existing PostgreSQL/macros/migration features; its merged
[upstream fix](https://github.com/launchbadge/sqlx/pull/4502) allows libsqlite3-sys 0.39.x in
dependency resolution. Use this exact Git revision until a suitable registry release
is verified. No advisory exception is configured.

New regression cases compile every shipped graph, reject a synthetic schema-6 store
without changing its bytes, and check uniform tool envelopes through the native host.
The protocol fixture independently records dispatches, checks missing/invalid/error
results and a 70-KB result, runs 70 discovery-only readiness lifetimes, and reopens
terminal results with unavailable provider routing and zero added dispatches. The
shipped extraction/cleanup graphs preserve the JSON checkpoint, destroy before a
nonzero-exit postcondition failure, and deny revoked checkpoint reads. These are
fixture-only protocol checks. Actual PostgreSQL additionally verifies that reopening
a directory-created migration ledger through `sqlx::migrate!` leaves its complete
entries, checksums and installation times unchanged.

Actual service tests used an isolated `context-mcp-ci` stack on alternate ports 57432,
61000/61001, 60108 and 60080; existing user containers and application data were not used.
The retained-data SQLite/fingerprint cutover procedure is in
[deployment.md](deployment.md#upgrading-the-embedded-htlk-runtime).

## Measured per-crate line coverage

Production source files only; integration helpers, examples, dependencies and generated
build output are excluded by the report checker. Embedded unit tests share source
files, matching the sibling HTLK reporting convention. No production coverage-skipping
attributes or test-only behavior are used.

| Crate | Covered / executable lines | Measured coverage |
| --- | --- | --- |
| context-core |210 /233|90.13%|
| context-storage |916 /1001|91.51%|
| context-workflows |726 /881|82.41%|
| context-auth |397 /421|94.30%|
| context-engine |4370 /5110|85.52%|
| context-process |199 /737|27.00%|
| context-mcp |805 /1282|62.79%|

The goal remains 99% meaningful production line coverage; it is **not yet reached**.
CI enforces measured per-crate floors and uncovered-line budgets in
tools/coverage-thresholds.json, with small margins for legal asynchronous outcomes.
A large crate cannot conceal another crate's regression. Raise these ratchets as gaps
are closed. The native processor's external Docker executions are verified separately
and do not contribute to the host's Rust coverage counters; they are not fabricated
as covered lines. Raw JSON is written to target/coverage/coverage.json.

## Actual native formats and paid provider journeys

- 28 reference format cases and 4 actual PDF-page/image-region/video/audio selection
  cases passed again on 2026-11-11 with the final HTLK pin and unchanged processing
  image, using `python3 tools/format_acceptance.py`. Independent size/digest, 16x16
  crop and one-second duration checks passed. Evidence:
  `target/providers/formats-hnl2i5wt/evidence.json`. No model calls were made.
  Details and prior measured limits remain in processing.md/context-services.md.
- After explicit user authorization on 2026-11-11, a fresh **container** journey used
  actual initial providers, gpt-4.1, whisper-1 and text-embedding-3-small through the
  upgraded HTLK runtime. It passed HTTP video upload,
  description/transcription/enrichment/embedding/publication, hybrid retrieval,
  native clip materialization, model-classified contradiction preservation, cited
  saved synthesis, transitive purge, then stdio ingest/query/purge in one retained
  client run. All MCP operations used the HTLK CLI or compiled client graph. The
  reference provider/container limitation remains the accepted receipt boundary.
- The separately invoked paid Rust case
  `actual_video_model_workflows_publish_independently_checked_evidence` also passed.
  It checks at least two published chunks containing the known blue/ocean evidence,
  finite 256-dimensional embeddings and exact Typesense document count. Its first
  attempt stopped before ingestion because the old fixture reused a retained 3-D
  collection identity for the real model space; the corrected test allocates a
  distinct model collection. Production index-identity protections remain intact.
  This is one additional opt-in paid Rust test, separate from the 52-test nonpaid
  coverage campaign and the container journey; its uninstrumented execution is not
  added to the coverage measurements above.

Reproduce the opt-in paid journey with the configured model credential inherited:

```sh
python3 tools/deployment_acceptance.py
```

It uses isolated alternate ports and removes only its own Compose project/volumes.
Latest paid evidence: `target/deployment/verified-ozka9ng1/evidence.json`.
Application image reported by that campaign:
`sha256:b7adc2e45760d8cd7f7ce1f97f2a39af66aea907dedbdda132ee633818f32de6`.
Processing image:
`sha256:707838fe31b4f8584ddb1f9e91c7fcef84cf7c760269e1262528c1b9c98b5407`.
The application image was rebuilt from the upgraded branch using the locked
dependency graph before invoking the campaign with `--skip-build`. Private staging
was empty after purge and teardown; all 26 retained campaign files were checked for
the privately loaded source model credential, with zero matches. These are observed local image
identities, not claims of a published image registry.

The original paid delivery evidence remains at
`target/deployment/verified-f_n7k_au/evidence.json`, with application image
`sha256:c0d69776d25a6149760a4282d13318a708452c62b6ffc0775048041fc1c4f862`.
The initial model429 and mini-model incorrect-color result remain in integration-spike.md.

## Dependency, license and release gates

`deny.toml` rejects unknown registries/Git sources, wildcard dependency requirements,
unapproved licenses, yanked/advisory issues. HTLK and SQLx are explicitly permitted,
exact upstream Git revisions. Duplicate transitive versions are visible warnings
rather than hidden updates. Rust dependency notices are generated and checked with:

```sh
python3 tools/notices.py
python3 tools/notices.py --check
python3 tools/release.py
```

The release script compiles locked optimized workspace binaries and constructs the
source archive twice with fixed order/timestamps/ownership, requiring identical bytes.
It excludes Git/temporary plans, data, environment/credential files and build output,
and writes checksums under target/releases. The publishable core crate is also rebuilt
from its Cargo archive. The application is distributed as source/binaries/containers,
not falsely marked crates.io-publishable with unavailable internal dependencies.
Pinned source/base/provider identities and runtime package inventories accompany
container recipes; rolling OS package repositories do not constitute a claim of
bit-for-bit identical container layers.
