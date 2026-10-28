# Working on context-mcp

Read `.opencode/.tmp/plans/context-mcp.md` before work, after every compaction, and
when resuming a task. It records the agreed requirements, acceptance criteria,
current task, verification evidence, and next action. Update it after completing
each task and commit the completed task's intended changes.

## Architecture boundaries

- One Rust application instance. PostgreSQL, Typesense, and S3-compatible storage.
- Every MCP operation is dispatched through unchanged public HTLK interfaces.
- All MCP providers are replaceable through operator-managed graphs, catalogs,
  and deployment bindings. Provider-specific tools do not belong in domain logic.
- Do not modify sibling/external projects. Explain and ask the user before adding
  any adaptation, custom adapter, compatibility shim, or extra HTLK mechanism.
- Permanent purge covers context-mcp-managed content; HTLK execution history is
  outside its scope. Preserve explicit provenance and authorization boundaries.

## Delivery

Follow the sibling htlk project's Rust quality standards: documented APIs, strict
formatting/lint/documentation checks, meaningful tests, dependency/license policy,
and reproducible release validation. Never describe fixture-only tests as proof of
real provider compatibility.

The user requested a commit after every completed task. Inspect status, diff, and
recent history first; stage only intended files and never secrets. When all tasks
are verified, remove the temporary plan, update this guidance to permanent docs,
commit the cleanup, and push all commits to origin.
