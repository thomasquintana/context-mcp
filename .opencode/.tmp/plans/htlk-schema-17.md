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
2. **Dependency and compatibility — pending.** Pin HTLK, refresh lock/notices,
   verify the resolved SQLite/SQLx graph, compile all targets, and test the existing
   public host, graph suite and authorization/checkpoint behavior. Change existing
   integration code only if a verified compatibility regression warrants it.
3. **Retained-state cutover — pending.** Test schema-13 rejection without altering
   history and same-version reopening/idempotency. Rehearse the cutover with a
   disposable deployment, retained application data, and no duplicate dispatch.
   Update the deployment/workflow-host/user guides and changelog.
4. **Release verification — pending.** Run locked build, tracked-input audit,
   strict checks and coverage through `tools/verify.py --coverage` with actual
   services, `tools/notices.py --check`, and `tools/release.py`. Preserve coverage
   budgets. Record new evidence separately, stop only owned services, and commit.
5. **Final review and PR — pending.** Review complete base diff and branch history,
   confirm clean committed state, push, create a PR against main, and report its
   URL and observed hosted checks.

## Next action

Pin upstream HTLK and verify the locked dependency graph and public integration.
