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
3. **Retained-state cutover — complete.** Documented the operator cutover. Public
   runtime tests passed: schemas 6, 13 and 17 are rejected byte-for-byte unchanged,
   schema 20 is confirmed through a read-only SQLite pragma, and current admission
   reopens with the same run ID, fingerprint and result. The two-binary transport
   rehearsal passed with a locked `c88acad` baseline: old HTTP publication, old
   store rejection and byte comparison, same evidence over new stdio, and new
   ingestion with one receipt. Used separate execution stores, disposable actual
   PostgreSQL/MinIO/Typesense, and pure role fixtures.
4. **Release verification — complete with baseline advisory failures.** The combined
   verifier passed fmt and strict Clippy, then stopped on RUSTSEC-2026-0543 (`chrono`)
   and RUSTSEC-2026-0542 (`humantime`), both reproduced on unchanged `c88acad`.
   License/source/bans, notice comparison, tracked-input audit, strict rustdoc,
   coverage-checker unit test, core package and locked release with deterministic
   archive passed. All 53 nonpaid tests passed when run under coverage directly,
   including the PostgreSQL migration ledger, Keycloak, sandbox metadata, transports,
   and purge; measured per-crate budgets passed unchanged. Updated the permanent
   evidence and removed only the campaign-owned services/volumes and worktree.
5. **Delivery — complete.** Reviewed every delivery commit and the cumulative diff
   against fresh `origin/main`; the tracked-input audit and diff checks passed.
   Pushed `feat/htlk-schema-20` and opened
   [PR #4](https://github.com/thomasquintana/context-mcp/pull/4) against `main`.
   Hosted quality checks were queued at the first check; no hosted pass is claimed.
   The PR documents both independently reproduced baseline advisories.
