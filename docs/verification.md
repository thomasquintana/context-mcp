# Verification and release evidence

## Reproduce strict and actual-service checks

Pinned tools: Rust1.98.0, cargo-llvm-cov0.9.1, cargo-deny0.20.2 and cargo-about0.9.2.
No model credential is required for the nonpaid verification campaign:

```sh
cargo build --workspace --all-features --locked
python3 tools/test_services.py
python3 tools/verify.py --coverage
python3 tools/test_services.py --stop
```

The disposable stack uses actual PostgreSQL, pinned-source MinIO, Typesense and
Keycloak, plus a SHA-256-verified unchanged sandbox binary and HTLK-discovered catalog.
Fixture service credentials are public development values. Configure the
`CONTEXT_TEST_*` references to use existing isolated services; the scripts do not
print token/model-secret values. Paid cases are explicitly excluded by name.

The latest fresh-stack run passed47 Rust tests:21 regular tests and26 opt-in actual
service/transport tests (including13 application campaigns,9 storage tests,2 identity
tests, the native-host sandbox test and actual binary HTTP/stdio campaign). Additional
checks passed fmt, strict all-target Clippy, strict rustdoc, advisory/license/source/
dependency policy, the independent coverage-gate test, notice generation, core package
archive verification and locked release compilation. CI workflow syntax was checked
with actionlint1.7.12. GitHub CI execution itself occurs after push.

## Measured per-crate line coverage

Production source files only; integration helpers, examples, dependencies and generated
build output are excluded by the report checker. Embedded unit tests share source
files, matching the sibling HTLK reporting convention. No production coverage-skipping
attributes or test-only behavior are used.

| Crate | Covered / executable lines | Measured coverage |
| --- | --- | --- |
| context-core |210 /233|90.13%|
| context-storage |916 /1001|91.51%|
| context-workflows |723 /875|82.63%|
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

-28 reference format cases passed individual actual HTLK+sandbox extraction checks;
  details/bounds and the completed-evidence/wrapper-timeout observation are preserved
  in processing.md.
-4 actual PDF-page/image-region/video/audio selection cases passed independent size,
  digest,16x16 crop and one-second duration checks; see context-services.md.
-The final **container** journey used actual initial providers, gpt-4.1, whisper-1
  and text-embedding-3-small through unchanged HTLK. It passed HTTP video upload,
  description/transcription/enrichment/embedding/publication, hybrid retrieval,
  native clip materialization, model-classified contradiction preservation, cited
  saved synthesis, transitive purge, then stdio ingest/query/purge in one retained
  client run. All MCP operations used the HTLK CLI or compiled client graph. The
  reference provider/container limitation remains the accepted receipt boundary.

Reproduce the opt-in paid journey with the configured model credential inherited:

```sh
python3 tools/deployment_acceptance.py
```

It uses isolated alternate ports and removes only its own Compose project/volumes.
Latest evidence: target/deployment/verified-f_n7k_au/evidence.json.
Application image:
`sha256:c0d69776d25a6149760a4282d13318a708452c62b6ffc0775048041fc1c4f862`.
Processing image:
`sha256:707838fe31b4f8584ddb1f9e91c7fcef84cf7c760269e1262528c1b9c98b5407`.
These are observed local identities, not claims of a published image registry.
The initial model429 and mini-model incorrect-color result remain in integration-spike.md.

## Dependency, license and release gates

`deny.toml` rejects unknown registries/Git sources, wildcard dependency requirements,
unapproved licenses, yanked/advisory issues. HTLK is an explicitly permitted, exact
Git revision. Duplicate transitive versions are visible warnings rather than hidden
updates. Rust dependency notices are generated and checked with:

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
