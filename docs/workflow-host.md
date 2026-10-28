# Long-lived HTLK role host

`context_workflows::host::WorkflowHost` owns one runtime, native registry, durable
SQLite store, native MCP adapter and execution engine for the application lifetime.
It uses unchanged public HTLK interfaces. PostgreSQL separately owns application
jobs, grants, workflow selection and saved run handles.

## Operator roles

Every configured graph has one required `request: json` input and one required
`result: json` output. The accepted result must be an object. Role-specific data
schemas and publication rules are enforced by application services. Exact source,
catalog and execution fingerprints are retained; provider tools/argument shapes
belong in the graph, endpoint routing/credentials in trusted deployment configuration.

The host verifies port contracts before registration and checks that every required
deployment is referenced by an admitted role. `ready` uses HTLK discovery to compare
complete selected descriptor digests, implementation identity and protocol. This
establishes tool availability/schema agreement, not account quota or model quality.

Retain operator workflow revisions needed by active jobs across restart. An upgrade
must not silently retarget old jobs to a different graph, catalog or evaluator
policy. The application persists exact fingerprints/handles and restores current
authorized job records before launching execution. Changing executable policies or
removing required old role revisions requires draining/migrating those jobs through
an explicit supported deployment procedure.

## Trusted job authority

Application grants also freeze exact role fingerprints; startup rejects a retained
job if its selected role revision no longer matches. Handles include the logical
role as well as executable/run identity, preventing equal-fingerprint aliases from
expanding checkpoint permissions.

An operator role may declare recoverable root-node outputs in `checkpoints`.
The host reads those accepted values with HTLK's existing `inspect_node` and
`read_artifact` APIs even when a later graph step fails. This is ordinary use of
the public inspection interface, not a new runtime mechanism or custom adapter.
Only declared nodes and the job's granted role are readable. Trusted cleanup reads
its saved opaque handle before revoking job authority.

Keep create/execute/destroy inside one extraction graph: HTLK's native stdio
sessions are run-scoped. An interrupted cleanup can use a separately configured
cleanup graph with the saved handle, accepting provider success receipts per the
agreed provider boundary. No direct provider/Docker cleanup path is introduced.

A `JobGrant` comes from authenticated application records and contains the job,
initiating principal, tenant, source asset IDs and permitted logical roles. Ordinary
request fields do not create these facts. The runtime principal is a service-owned
job identity; security labels independently retain initiating principal, tenant,
job and asset provenance. Provider binding digests are restricted to granted graphs.

Only the operator registers graphs. Jobs can start only their assigned roles,
dispatch their pinned bindings, inspect their own protected run, and read their own
accepted artifacts. Revocation denies future dispatch and artifact reads; a saved
handle can still request cancellation. Unknown authorization variants fail closed.
There is no externally exposed raw graph-registration or arbitrary artifact-read API.

The service authenticates callers, checks current memberships/ACLs and manages job
revocation before using this host. The host is not a replacement for resource ACLs.

## Lifecycle and bounds

- Persistent SQLite WAL/FULL state, compatible registry/policy reconstruction.
- Bounded in-process workers/runs using HTLK's existing engine.
- Default role execution: 30-minute run, 120-second attempt, four concurrent calls;
  4MB evaluator value/output bounds and 64M deterministic steps.
- Start returns a durable handle without waiting for completion. Stable keys bridge
  admission/recovery; inspection distinguishes running/succeeded/failed/cancelled.
- Only accepted object-valued results cross into application publication.
- Shutdown stops host admission, engine and native sessions, preserving execution
  history. The application drains request/worker ownership before shutdown.

Sandbox cleanup continues to use ordinary operator graphs and successful provider
receipts, per the user's accepted boundary. No direct provider calls, custom MCP
adapter, watchdog/TTL wrapper, or external-project change is involved.

## Verification

Public-interface tests exercise shared-host concurrent job separation, forged
identity/handle denial, exact role permissions (even when aliases share a graph),
revocation, durable reopening/idempotency, required-provider/port validation and
rejection of scalar results before application publication.

An additional actual-provider test checks exact live sandbox readiness and a real
metadata call through the production policy:

```console
CONTEXT_TEST_SANDBOX_BINARY="$PWD/target/providers/mcp-sandbox" CONTEXT_TEST_MCP_CATALOGS="$PWD/target/providers/catalogs.json" cargo test -p context-workflows --test production_host actual_sandbox --locked -- --ignored
```

This real test proves that policy/provenance and native dispatch work together.
It performs no model request or container creation. The prior real model/media
evidence remains in the integration report. Full application/container E2E has since
passed; see verification.md for the current evidence and exact boundaries.
