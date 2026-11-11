# HTLK runtime upgrade working plan

## Alignment protocol

Read this plan after every compaction, before selecting work, and after every task.
Update its status, evidence and follow-ups after each completed task, then commit
the task's intended changes. Consult permanent contracts in AGENTS.md throughout.
Finish with all intended changes committed and a new GitHub pull request.

Branch: `feat/htlk-runtime-upgrade`.
Baseline: context-mcp `7ecad1f`, HTLK `3448acb5dc7cebef4d08ef37bb79757fea780659`.
Target: HTLK `1e405c495fc2e9b3b3fb812123f509178b08a9e5` (upstream SQLite fix merged).
Initial reviewed integration target: `521f795d913ae3e13938b8ed0fe19728d2babce6`.

## Constraints and decisions

- Use unchanged public HTLK interfaces and operator graphs; preserve provider
  replaceability, trusted job provenance, revocation and publication/purge fences.
- Preserve existing application JSON contracts while explicitly projecting the new
  `McpToolResult.data` within graphs, including opaque cleanup checkpoints.
- HTLK SQLite schema 6 is incompatible with schema 13. Plan a data-preserving,
  operator-controlled cutover: settle old work under the old binary, retain old
  execution history and use a fresh execution store. Never reset/migrate user state
  automatically or silently retarget frozen workflows/old retry requests.
- Preserve accepted sandbox destroy-receipt limitations and permanent upload fences.
- Paid model/provider campaigns require separate explicit authorization. Nonpaid
  service and actual sandbox checks are part of the task.
- Evidence from upstream's 23-tool campaign used the old context binary; it does
  not establish that the embedded runtime upgrade has been verified.

## Tasks and completion criteria

1. **Branch and working plan — completed.** Create branch, save this plan, and
   record its alignment protocol in AGENTS.md. Verification: clean starting status,
    reviewed recent history, branch/path inspected; release audit and diff check passed.
2. **Dependency/public APIs — completed.** Pin target HTLK; refresh Cargo.lock; add
   compiler policy/document fields, optional output schemas/result policies;
   detach owned compilation artifacts via `into_artifact()`. Review native session
   and execution defaults and fail-closed authorization. Compile workspace/examples/
   tests and run existing ordinary tests. Commit verified public-API changes.
3. **Workflow result migration — completed.** Update 17 checked-in graphs, generated
   CLI/test graphs and role checkpoint handling. Use `McpToolResult` inputs and
   explicit `.data` projection into application JSON. Preserve exit/destroy checks,
   loop results and cleanup checkpoint shape. Compile all shipped graphs against
   exact fixture catalogs; task 5 executes result-size and failure regressions.
4. **Data-preserving cutover — completed.** Document deployment/workflow-host
   procedure, active jobs/cleanup/purge settlement, pending uploads and legacy retry
   restrictions. Verify incompatible stores stay unchanged and same-version reopen
   remains idempotent. Add only narrowly necessary diagnostics, with no new adapters.
5. **Integration verification — completed.** Focused envelope/required-data/checkpoint
   tests and existing auth/revocation/cancel/fenced-cleanup tests. Run fresh nonpaid
   PostgreSQL/S3/Typesense/Keycloak campaign, actual sandbox metadata, both transports
   and CLI, with repeated readiness/session reclamation. Fix observed regressions.
6. **Release gates/evidence — completed.** Refresh notices, changelog and measured
   acceptance/verification docs. Preserve coverage floors/budgets. Run locked build,
   tracked release audit, `tools/verify.py --coverage`, notice check, dependency gates
   and deterministic release. Clearly retain historical paid/native-format evidence.
7. **Final review and PR — completed.** Read plan, review all commits/base diff,
   confirm intended changes committed and no secrets, push this branch and create a
   new PR. Return PR URL and actual local/remote verification status.
8. **Authorized paid model campaigns — completed.** User explicitly requested
   paid tests after PR creation. Obtain the configured credential by environment or
   private-file reference without exposing it. Build the application container from
   this branch; run `tools/deployment_acceptance.py` against isolated services and
   real gpt-4.1/whisper-1/text-embedding-3-small through HTLK. Also run the explicit
   paid Rust video/model case if its configured local-provider prerequisites exist.
   Verify actual published evidence, vectors, model outcomes, citations, purge and
   both transports; accept normal sandbox destroy receipts under the existing
   provider boundary. Record exact evidence/images and failures, clean only owned
   services, update permanent docs, commit and push results to PR #1.
9. **Observed hosted-CI startup follow-up — completed and verified hosted.** PR's nonpaid Linux coverage
   transport test failed after its fixed ~10-second server-readiness allowance while
   the server remained alive. Preserve all outcome assertions and use a finite
   wall-clock startup allowance suitable for instrumented hosted startup. Verify
   targeted/full transport checks and report subsequent actual remote CI separately.
10. **Latest hosted workflow failure — completed and verified hosted.** User reports CI failure after
    the readiness fix. Inspect both latest runs at c44da37: startup now passes, but
    a later `succeeded` assertion hides the call site and failure code. Add useful
    redacted test diagnostics and reproduce the independent native adapter's fixed
    10-second initialization bound with an 11-second-start peer. Fix through existing
    public HTLK timeout settings, preserve no-retry/dispatch invariants, verify local
    gates and follow the pushed GitHub run to an observed successful conclusion.

## Verification commands

```sh
cargo build --workspace --all-features --locked
python3 tools/audit_release.py
python3 tools/test_services.py
python3 tools/verify.py --coverage
python3 tools/notices.py --check
python3 tools/release.py
python3 tools/test_services.py --stop
```

Run targeted checks during tasks; full locked notice/dependency/release gates must
pass before delivery. Regenerate notices with `python3 tools/notices.py` after the
dependency update. Do not invoke `tools/deployment_acceptance.py` without explicit
paid-test authorization. Remove only this campaign's disposable backing services.

## Task log

- Setup: repository started clean on `main`; created `feat/htlk-runtime-upgrade`.
  Plan stored at the user-requested temporary path. No application data touched.
- Task 2: exact target pin and public constructor/artifact updates compile with
  `cargo check --workspace --all-targets --all-features`. All 21 ordinary Rust
  tests and doctests pass with `cargo test --workspace --all-features --locked`;
  `cargo fmt --all --check` passes. Native default policies remain enforced,
  callback services uninstalled, replay authority false and unknown authorization
  variants denied. Production engine retains its intentional single-store scope.
  Dependency graph removes unused upstream SDK/parser dependencies; notices refresh
  remains task 6. New native client lifetime owner bound (4096 by default) must be
  documented/checked during task 5; retired transport capacity is separate.
- Task 3: migrated all 17 shipped graphs and generated CLI/transport/native-host
  graphs to explicit envelope projections. Extraction retains `create` as a JSON
  projection checkpoint, preserving cleanup shape and operator checkpoint config.
  Added a compiler regression over all shipped graphs with explicit fixture-only
  provider descriptors plus actual exported context descriptors. It caught the
  diagnostic compiler's 1000-node expansion bound after adding projections inside
  two 200-iteration loops; raised that finite bound to 2000. The graph suite,
  all 10 ordinary workflow tests and fmt pass. Runtime edge/failure/large-result
  checks remain task 5, distinct from this compile compatibility evidence.
- Task 4: documented maintenance cutover, old-state settlement, new schema-13 path,
  protected historical-store retention, legacy retries/receipts, explicit resubmission
  and rollback boundary. Added independent Python-SQLite legacy-header/sentinel
  fixture; public HTLK runner rejects it with StoreError::Schema and leaves the
  database byte-for-byte unchanged. All five public-runtime tests and production
  terminal-run same-version reopen test pass; diff check and fmt pass. No operator
  migration tool or private-runtime-table copy added, and no deployment state touched.
- Task 5: native fixture tests cover declared/absent/invalid/error/text-only results,
  a literal 70-KB response, 70 discovery-only readiness lifetimes, zero-dispatch
  terminal offline reopen, actual shipped extract/cleanup graph checkpoint shape,
  nonzero native-exit rejection after destroy, and revoked checkpoint denial.
  All 51 nonpaid workspace tests pass against fresh context-mcp-ci services on
  alternate ports (Postgres 57432, S3 61000/61001, Typesense 60108, Keycloak 60080).
  Existing user containers were left running. Actual sandbox metadata and both
  binary transports/CLI passed. Strict all-target Clippy and diff check pass.
  `python3 tools/format_acceptance.py` additionally passed all 28 native format and
  four selection cases; evidence `target/providers/formats-opziwhbp/evidence.json`,
  unchanged processing image `sha256:707838fe31b4f8584ddb1f9e91c7fcef84cf7c760269e1262528c1b9c98b5407`.
  No model calls were made. Native owner's lifetime bound documented, no new
  mechanism or fence-reset workaround introduced. Disposable services stay up for
  task 6 coverage, then stop only context-mcp-ci.
- Task 6 is blocked by the current advisory gate, not by a failed functional test.
  `tools/verify.py --coverage` passed fmt/Clippy then stopped before coverage on:
  RUSTSEC-2026-0459 (`libsqlite3-sys` 0.35.0 via HTLK's tokio-rusqlite 0.7.0 /
  rusqlite 0.37.0) and RUSTSEC-2026-0462 (context-mcp rmcp 3.8.0).
  Native HTLK client replaced rmcp; local rmcp server features do not enable the
  advisory's HTTP clients, but the package-level gate still fails. Plan a direct
  local rmcp >=3.9.0 update rather than an advisory ignore.
  Upstream HTLK main remains 521f795d. User requested a Markdown handoff prompt for
  a different agent to fix HTLK; sibling modification was NOT authorized here.
  Patched SQLite requires libsqlite3-sys >=0.39.0 / rusqlite >=0.41 (SQLite >=3.54).
  Registry check: latest tokio-rusqlite 0.8.0 still requires rusqlite ^0.40.1, so
  merely updating tokio-rusqlite is not an established fix. The handoff must resolve
  this upstream dependency boundary without an unapproved fork/shim or ignore.
  Notices regenerated and `tools/notices.py --check` passed for the current lock;
  tracked release audit passed. Coverage/release/package are NOT yet verified for
  this branch. Stop only this task's context-mcp-ci stack while awaiting handoff.
- Task 6 dependency-unblock subtask completed: HTLK pinned to merged PR #23 commit
  1e405c49, resolving rusqlite 0.41.1/libsqlite3-sys 0.39.1; rmcp pinned to 3.9.0.
  User approved PostgreSQL-only exact SQLx crates after the umbrella's unused
  SQLite driver caused a native-links resolution conflict. A small shared storage
  module reexports upstream SQLx queries/types; migration execution uses its public
  embedded Migrator API. All original migration versions/descriptions/SQL/checksums
  match the independent SQLx directory loader. Actual PostgreSQL test first applies
  that original loader then reopens through Database, proving the full ledger
  (including installation times) remains unchanged. All 26 ordinary tests, locked
  workspace build/check, strict Clippy, advisory/license/source/bans, notice check,
  release audit and diff check pass. The isolated alternate-port services are up
  again for final task 6 coverage/release and final-pin native verification.
- User correction: undo the PostgreSQL-only SQLx removal and use SQLx with its
  upstream fix. The earlier SQLx-core/driver module, manual migration manifest and
  import refactor are superseded and being reversed in a new commit (retain the
  verified HTLK/rmcp security pins). Official merged SQLx PR #4502 at
  `10462fa4414c6c7f1ed1d39bb31678e3b726878c` permits libsqlite3-sys 0.39.x; no 0.9.1
  release is available, so pin the standard SQLx facade to that exact upstream Git
  revision and explicitly allow only that upstream source. Restore `sqlx::migrate!`
  and direct `sqlx` imports; keep the useful actual-PostgreSQL ledger reopen test.
  Prior direct-driver full coverage run passed 53 tests and unchanged floors, and
  final-HTLK native formats passed 28+4 cases under `formats-hnl2i5wt`; the SQLx
  configuration change requires a new final coverage/service run before delivery.
- SQLx correction verified: restored standard facade, original query imports and
  migration macro; removed direct-driver reexport module/manual manifest. Exact
  official upstream 10462fa4 pin resolves with HTLK's libsqlite3-sys 0.39.1.
  Locked workspace check, all 25 ordinary tests and actual PostgreSQL macro/ledger
  reopen pass; strict advisory/license/source/bans and regenerated notice check pass.
  Final full coverage/release campaign remains pending for this corrected graph.
- Task 6 completed on the corrected standard-SQLx graph: locked build and
  `tools/verify.py --coverage` pass all 52 Rust tests (25 ordinary, 27 actual-service
  opt-in), fmt, strict Clippy/rustdoc, advisory/license/source/bans, coverage budget
  checker and core Cargo package rebuild. Exact measured lines: core 210/233,
  storage 916/1001, workflows 726/881, auth 397/421, engine 4370/5110, process
  199/737, application 807/1282. Existing floors/uncovered budgets remain unchanged.
  Notice check and tracked release audit pass. `tools/release.py` built locked
  optimized binaries and verified identical deterministic source archives. Final
  native HTLK 28+4 cases passed under formats-hnl2i5wt; no paid calls invoked.
  Updated permanent acceptance/verification/native evidence with historical paid
  boundaries. Stopped only context-mcp-ci services/volumes; original user containers
  remain running. Refresh deterministic archive after final documentation edits.
- Task 7: reviewed all nine implementation/evidence commits and the complete base
  diff against freshly fetched origin/main. The requested SQLx reversal is reflected
  in the final tree: standard SQLx facade/macros and original storage/domain sources
  remain, with only the useful PostgreSQL ledger regression added. Regenerated the
  deterministic source archive after final permanent docs (`release.py --skip-build`;
  optimized binaries already verified by the full locked release build), and reran
  the tracked-input audit/notice check. All intended implementation changes committed
  and pushed to `feat/htlk-runtime-upgrade`. New PR:
  https://github.com/thomasquintana/context-mcp/pull/1
  GitHub quality workflows were IN_PROGRESS at the observed remote check; local
  verification passes are not claimed as completed remote CI. Completion record
  is committed/pushed with the same task-delivery workflow.
- Task 8 preparation: user explicitly authorized paid tests and identified `.env`
  credentials. No `.env` is present in context-mcp; the existing provider reference
  `../llm-mcp/.env` contains the required model key. Checked presence without
  displaying its value; load only the required key privately for test subprocesses.
  Built this branch's application image from deploy/Dockerfile with locked deps:
  `sha256:c9366f98e2af3950b0dde88550390c486e41ffbb6c61fa1f0e6f21c843b766a9`.
  Pinned unchanged model provider and sandbox layers were used. Existing processing
  image is `sha256:707838fe31b4f8584ddb1f9e91c7fcef84cf7c760269e1262528c1b9c98b5407`.
  No application data or existing service containers modified during preparation.
- Paid deployment campaign passed using the rebuilt image and real gpt-4.1,
  whisper-1 and text-embedding-3-small. Evidence:
  `target/deployment/verified-ozka9ng1/evidence.json`. All HTTP video/retrieval/native
  clip/model conflict/saved synthesis/transitive purge and stdio ingest/query/purge
  assertions passed. Its project/services/volumes were removed by the harness.
  Observed Docker image identity in the receipt is
  `sha256:b7adc2e45760d8cd7f7ce1f97f2a39af66aea907dedbdda132ee633818f32de6`;
  c9366f98 above is the build's config digest, not the campaign's reported image ID.
  The additional paid Rust case initially failed before ingestion/model work: its
  stale fixture tried to reuse a retained 3-D index identity for the 256-D model
  space, correctly rejected by application startup. Correct only the test setup
  to allocate a distinct model collection and retire its own unused empty fixture
  collection/binding. Rerun the explicit paid Rust case and retain this finding.
- Paid Rust case now passes (19.58 seconds of test execution), independently
  checking >=2 published chunks containing the known blue/ocean evidence, finite
  256-dimensional embeddings and exact Typesense document count. Its managed
  fixture cleanup completed. Both authorized paid campaigns passed with no model
  failure; the initial Rust failure was a pre-ingestion test configuration issue.
  Hosted CI failure is separately attributed to the nonpaid transport test's
  100x100-ms readiness loop; replace it with a finite 60-second wall-clock startup
  bound while retaining process-exit and actual HTTP readiness assertions.
- Task 9 local fix verification: actual HTTP/stdio/CLI transport campaign passes
  with a finite 60-second wall-clock startup allowance (68.75 seconds test total).
  No production timeout or transport assertion changed. Commit/push this scoped
  fix to start a hosted rerun while finishing paid evidence and final checks.
  Paid campaign staging is empty and a private equality scan of all 26 retained
  files found zero source model-credential matches. The Rust fixture's test setup
  correction remains scoped to its own empty bootstrap collection/binding.
- Final paid follow-up gates pass: all 52 nonpaid Rust tests under coverage plus
  strict fmt/Clippy/rustdoc, unchanged per-crate budgets, package rebuild, dependency
  policy, notice check, tracked-input audit and locked reproducible release. Latest
  application line coverage is 805/1282 (62.79%), within the existing budget; paid
  uninstrumented execution is not added to coverage. Permanent docs now record both
  authorized paid successes and their exact image/evidence references, preserving
  original evidence and the initial pre-ingestion fixture failure. Owned CI test
  services/volumes are removed; original user containers remain untouched. Hosted
  startup-fix rerun is still running (observed at its service-setup step), not yet
  a remote success claim. Commit/push these measured results and update PR #1.
- Task 10 diagnosis/regression: both latest hosted runs passed HTTP readiness and
  failed at a later generic `succeeded` assertion after ~122 seconds. The adapter
  still used its independent 10-second MCP initialization/discovery limit. An
  independent peer delaying startup by 11 seconds reproduces E_DEPENDENCY before
  the first tool operation with that old bound. With the existing public native
  adapter allowance set to 30 seconds, the same regression passes and its dispatch
  log proves exactly one tool invocation, without retry/replay. Added track-caller
  and redacted failure-code diagnostics to the transport assertion helper so any
  remaining hosted failure identifies its phase. Strict Clippy/fmt pass. Commit and
  push this targeted verified fix, then run the full nonpaid/coverage/release gates
  and follow hosted CI to success; the exact hosted phase remains to be confirmed
  by the improved diagnostic or a passing full run.
- Hosted 2e4f194 reruns still failed: the failure changed from a terminal workflow
  failure to the test observer's own `transport workflow deadline exceeded` at
  result() after its 500x20-ms poll allowance. Thus the observation budget also
  needs to cover the now-bounded native startup. Replace result polling with a
  finite 60-second wall-clock deadline and report the logical role on inspection/
  timeout failures. Do not confuse observer timeout with a failed provider result.
- Moved the slow-start regression into tests/transports.rs so it exercises the
  actual shared call/result observer as well as the real native host. The fixture
  delay is still 11 seconds, and its independent dispatch log must contain exactly
  one context_identity call. A temporary 10-second observer deadline reproduces
  `transport workflow deadline exceeded for role identity`; the 60-second observer
  passes the identical case. Strict Clippy/fmt pass. This is one moved regression,
  not an additional test; total nonpaid Rust tests remains 53. Push this follow-up
  and re-run local gates/hosted checks, keeping both native and observer bounds.
- Task 10 full local verification passes with the native-startup fix: locked build,
  all 53 nonpaid Rust tests (new slow-start regression), fmt/Clippy/rustdoc,
  dependency/advisory/license/source gates, unchanged coverage floors/uncovered
  budgets, coverage checker and core package. Latest workflow coverage is 728/881
  (82.63%); application coverage remains 805/1282 (62.79%). Hosted runs on 2e4f194
  are in progress; do not stop at local success or repeat the earlier unobserved
  hosted-success assumption. Finish release/audit/notices and watch GitHub result.
- The combined native + observer fix on 37958d1 passes the full 53-test local
  nonpaid coverage campaign, strict checks, all existing budgets and core package.
  New observed coverage: workflows 724/881 (82.18%), engine 4371/5110 (85.54%),
  application 805/1282 (62.79%); budgets remain unchanged. The regression now covers
  the actual transport helper and native peer together, not merely the lower host.
- Hosted 37958d1 runs now pass every transport operation and the slow-start
  regression, then fail DELETE of the disposable S3 bucket. The transport fixture
  deleted only objects still referenced in PostgreSQL; slow runs cross the 60-second
  upload expiry and leave approved permanent zero-byte upload fences. Reproduce
  deterministically by including one generated zero-byte uploads/ control record
  after writers stop. Fix only disposable test-bucket teardown: enumerate leftovers,
  assert they are content-free, delete them, then require successful bucket removal.
  Never remove fences from an active deployment or hide unexpected retained payloads.
- The disposable-fence regression reproduces the exact hosted DELETE-bucket
  assertion locally (74.79 seconds), after all transport operations pass. Teardown
  now lists remaining keys after all application writers stop, requires size zero
  and uploads/ namespace, removes only those owned disposable fences, and still
  requires successful bucket deletion. The same targeted real-service campaign
  passes (72.22 seconds); locked all-target check, fmt and strict Clippy pass.
  Added only the already-resolved futures-util dev dependency for stream collection.
  Push this verified cleanup fix and follow the full hosted pipeline again.
- Hosted 68c96e4 runs pass the entire strict/test/coverage/package step. The next
  notice step fails immediately. Reproducing CI's exact `cargo install cargo-about
  --version 0.9.2 --locked` in a clean pinned Rust Linux image confirms it installs
  no executable: cargo reports that bin cargo-about requires feature `cli`, and
  `cargo about --version` fails with "no such command: about". This is NOT an
  established notice-content/platform mismatch. Fix CI install with `--features
  cli` and immediately print its version; retain the existing unchanged notice
  generator and golden. Verify the actual Linux notice comparison before pushing.
- Notice fix verified in the same clean pinned Rust Linux image: install with
  `--locked --features cli` produces executable cargo-about 0.9.2; running the
  unchanged `python3 tools/notices.py --check` against a read-only checkout passes.
  The notice golden, generator and target-selection policy need no changes.
  Latest local full 53-test campaign passes with all original coverage budgets
  (workflows 728/881, engine 4370/5110, application 807/1282). One intervening local
  S3 read error did not reproduce in the targeted case or the subsequent full run;
  discarded the temporary unrelated diagnostic edit to keep the fix scoped.
- Task 10 completed: both full hosted Linux/arm64 pipelines on 45dac37 succeeded.
  PR run https://github.com/thomasquintana/context-mcp/actions/runs/43099143905
  and push run https://github.com/thomasquintana/context-mcp/actions/runs/43099139020
  each passed every step: tool install, locked build, audit, actual services, strict
  tests/coverage/package, notice comparison, locked reproducible release, artifacts
  and cleanup. No notice content/policy exception or application adaptation added.
  All preceding fixes have independent failure/success reproductions. Record these
  observed links in permanent evidence, commit/push the completion record and verify
  the latest metadata-only head's checks before final delivery.

## Current next action

User reported updated HTLK; reviewed merged upstream PR #23 at 1e405c49. It pins
rusqlite 0.41.1 with patched bundled SQLite, replaces affected tokio-rusqlite with
an upstream-private connection worker, preserves public APIs/schema 13 and changes
native/executable fingerprints. Upstream dependency/quality/line/branch CI passed.
All fixes and local/hosted verification are complete. Commit/push the final observed
CI run links and completion record, update PR #1's CI summary, and verify latest-head
checks. Keep the notice generator/golden and all strict gates unchanged.
Prior paid evidence remains valid; no paid rerun is required to diagnose this
nonpaid initialization boundary unless new observed behavior justifies it.
