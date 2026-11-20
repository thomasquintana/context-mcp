# HTLK schema-17 upgrade working plan

Branch: `feat/htlk-schema-17`; base: `origin/main` at `8ea4fe7`.
Upgrade HTLK from `1e405c495fc2e9b3b3fb812123f509178b08a9e5` to merged upstream
`e6fa8c15d6d313f4b3fe4259dbaf39a5cc8bcd8b` (both package version `0.1.0`).

Read this plan after every compaction, before choosing work, after every task, and
for general guidance. Update status and actual evidence after each task. Commit
each completed, verified task. Push the committed branch and open a new PR.

## Scope

- Keep the single-instance SQLite execution host and existing public HTLK APIs.
  Preserve exact provider catalogs, workflow authority, opaque cleanup checkpoints,
  publication/purge fences and application wire contracts.
- Do not reset or mutate old stores, retarget frozen fingerprints, or modify sibling
  projects. Retain old execution history and require a fresh schema-17 store after
  settling old work. PostgreSQL application state, S3 and Typesense are separate.
- Use only owned disposable data/services for verification. No paid provider cases.

## Tasks

1. **Branch and plan — complete.** Started from clean GitHub main `8ea4fe7` and
   created `feat/htlk-schema-17`. Read permanent contracts and verified upstream
   main still points at the planned revision. Record branch-specific alignment.
2. **Dependency and compatibility — complete.** Pinned SQLite-only HTLK; the lock
   changes only five HTLK source entries and regenerated notices match. The locked
   build, all-target check, 26 ordinary tests (including all shipped graphs and
   host authorization/checkpoint/offline replay), and license/source/bans checks
   pass. No host/graph adaptation was needed. The advisory check fails on existing
   `chrono` RUSTSEC-2026-0543; independently reproduced it on unchanged `8ea4fe7`.
   The published and upstream-main `object_store` still require `chrono`; tracked
   upstream in apache/arrow-rs-object-store#909. Keep the strict gate intact;
   independently run the functional parts of verification if it stops at that gate.
3. **Retained-state cutover — complete.** Synthetic schemas 6 and 13 are rejected
   byte-for-byte unchanged; current-version idempotent reopen and offline
   no-dispatch tests pass. The two-binary transport rehearsal passed using an actual
   schema-13 executable built from `8ea4fe7`: legacy HTTP upload/publication, schema
   rejection and byte comparison, new stdio serving with a fresh execution file,
   reading the same exact published evidence, and new ingestion with one receipt.
   Both binaries used real disposable PostgreSQL/MinIO/Typesense and pure fixture
   workflow roles. Updated deployment/workflow-host/user docs and changelog;
   strict Clippy, fmt and tracked-input audit pass.
4. **Release verification — complete with a baseline advisory blocker.** Locked
   build, tracked-input audit, strict fmt/Clippy/rustdoc, license/source/bans,
   notice comparison, package and full deterministic release pass. The combined
   verifier stops on maintenance advisories RUSTSEC-2026-0543 (`chrono`) and
   RUSTSEC-2026-0542 (`humantime`), both confirmed on the unchanged baseline too.
   The published `object_store` release still uses both (upstream issue 909 and
   PR 908). Ran the remaining verifier commands independently and all 53 nonpaid
   tests passed under coverage, with no budget changes. The first coverage run saw
   an OOM-killed disposable Keycloak; after restarting only that container, the
   full rerun passed including Keycloak, actual sandbox metadata, transport and
   purge. Removed the owned service containers/volumes and detached baseline
   worktree, leaving other services in place. Evidence is in verification.md.
5. **Final review and PR — pending.** Review complete base diff and branch history,
   confirm clean committed state, push, create a PR against main, and report its
   URL and observed hosted checks.

## Next action

Review the complete diff and commit range, push and open the pull request. Report
the advisory blocker and observed hosted status without changing policy.
