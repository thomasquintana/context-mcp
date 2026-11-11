# HTLK runtime upgrade working plan

## Alignment protocol

Read this plan after every compaction, before selecting work, and after every task.
Update its status, evidence and follow-ups after each completed task, then commit
the task's intended changes. Consult permanent contracts in AGENTS.md throughout.
Finish with all intended changes committed and a new GitHub pull request.

Branch: `feat/htlk-runtime-upgrade`.
Baseline: context-mcp `7ecad1f`, HTLK `3448acb5dc7cebef4d08ef37bb79757fea780659`.
Target: HTLK `521f795d913ae3e13938b8ed0fe19728d2babce6` (reviewed upstream main).

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
   reviewed recent history, branch/path inspected; release audit below before commit.
2. **Dependency/public APIs — pending.** Pin target HTLK; refresh Cargo.lock; add
   compiler policy/document fields, optional output schemas/result policies;
   detach owned compilation artifacts via `into_artifact()`. Review native session
   and execution defaults and fail-closed authorization. Compile workspace/examples/
   tests and run existing ordinary tests. Commit verified public-API changes.
3. **Workflow result migration — pending.** Update 17 checked-in graphs, generated
   CLI/test graphs and role checkpoint handling. Use `McpToolResult` inputs and
   explicit `.data` projection into application JSON. Preserve exit/destroy checks,
   loop results and cleanup checkpoint shape. Compile all shipped graphs against
   exact fixture catalogs; verify representative result sizes and failure behavior.
4. **Data-preserving cutover — pending.** Document deployment/workflow-host
   procedure, active jobs/cleanup/purge settlement, pending uploads and legacy retry
   restrictions. Verify incompatible stores stay unchanged and same-version reopen
   remains idempotent. Add only narrowly necessary diagnostics, with no new adapters.
5. **Integration verification — pending.** Focused envelope/required-data/checkpoint
   tests and existing auth/revocation/cancel/fenced-cleanup tests. Run fresh nonpaid
   PostgreSQL/S3/Typesense/Keycloak campaign, actual sandbox metadata, both transports
   and CLI, with repeated readiness/session reclamation. Fix observed regressions.
6. **Release gates/evidence — pending.** Refresh notices, changelog and measured
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

## Current next action

Verify and commit setup; begin task 2 against the exact target revision.
