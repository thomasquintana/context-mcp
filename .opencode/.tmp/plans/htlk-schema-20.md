# HTLK schema-20 upgrade working plan

Branch: `feat/htlk-schema-20`; base: `origin/main` at `c88acad` (merged PR #3).
Target: HTLK `873da82326f00c55f2f7e9549b7bc07dd78b0b54` from upstream `main`,
replacing `e6fa8c15d6d313f4b3fe4259dbaf39a5cc8bcd8b`. Both identify as `0.1.0`;
the published `v0.1.0` tag predates both pins.

Read this plan after every compaction, before choosing work, after every task,
and for general guidance. Record each task's measured results here and commit
each verified task. Review the complete diff, push the branch, and open a new PR.

## Scope

- Keep the one-instance SQLite HTLK execution host and unchanged public HTLK APIs.
  Keep operator graphs, exact catalogs, role authorization, frozen fingerprints,
  cleanup checkpoints, application wire contracts and publication/purge fences.
- Settle old operations with the old binary before switching to a fresh schema-20
  execution path. Retain schema-17 history with its matching binary/configuration;
  do not edit private stores or move PostgreSQL, payloads, or search indexes.
- Use owned disposable services and fixture data; no paid provider tests or sibling
  project changes. Do not waive baseline advisories or lower coverage gates.

## Tasks

1. **Branch and plan — complete.** Confirmed PR #3 was merged, fetched `main`,
   branched from `c88acad`, verified upstream `main`, and reviewed the permanent
   compatibility and release contracts. Added branch-specific alignment guidance.
2. **Dependencies and compatibility — complete.** Pinned SQLite-only HTLK and
   aligned Actix, Tokio, serde_json and URL. Published SQLx `0.9.1` permits
   `libsqlite3-sys` `0.40.0`; the build links it through HTLK's `rusqlite` `0.43.0`
   only. Removed the unused SQLx Git allowance. Locked build, all-target check,
   26 regular tests (including all shipped graphs), notices and license/source/bans
   checks passed. Neither host interfaces nor graph sources needed changes.
3. **Retained-state cutover — pending.** Update deployment/host/user docs and the
   changelog. Reject schema 17 unchanged, verify schema 20 and durable reopen, and
   rehearse with real schema-17 and schema-20 binaries using separate execution
   stores and shared disposable PostgreSQL, MinIO and Typesense. Check retained
   published evidence and new work, including the single-ingestion receipt.
4. **Release verification — pending.** Run the strict verify/coverage, dependency,
   notice, tracked-input, package and deterministic release gates. If the combined
   verifier stops on baseline `chrono`/`humantime` maintenance advisories, compare
   with the unchanged baseline and run remaining checks separately. Record actual
   evidence and clean up only campaign-owned services and the detached worktree.
5. **Delivery — pending.** Review every commit and the cumulative diff against
   fresh `origin/main`. Commit all intended changes, push, create a new PR against
   `main`, and record the PR and observed check status here.
