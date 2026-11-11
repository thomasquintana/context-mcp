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
6. **Release gates/evidence — in progress.** Refresh notices, changelog and measured
   acceptance/verification docs. Preserve coverage floors/budgets. Run locked build,
   tracked release audit, `tools/verify.py --coverage`, notice check, dependency gates
   and deterministic release. Clearly retain historical paid/native-format evidence.
7. **Final review and PR — pending.** Read plan, review all commits/base diff,
   confirm intended changes committed and no secrets, push this branch and create a
   new PR. Return PR URL and actual local/remote verification status.

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

## Current next action

User reported updated HTLK; reviewed merged upstream PR #23 at 1e405c49. It pins
rusqlite 0.41.1 with patched bundled SQLite, replaces affected tokio-rusqlite with
an upstream-private connection worker, preserves public APIs/schema 13 and changes
native/executable fingerprints. Upstream dependency/quality/line/branch CI passed.
Follow latest user direction: reverse PostgreSQL-only SQLx changes and pin the
standard SQLx facade to official merged PR #4502. Verify/commit this scoped reversal
and fix; then rerun full coverage/service/release checks, update measured docs and
finish task 7 with a new context-mcp PR. Native formats on the final HTLK revision
already passed; SQLx does not participate in the independent native host campaign.
