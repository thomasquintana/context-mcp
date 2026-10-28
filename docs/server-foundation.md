# Server foundation and operator commands

The actual `context-mcp` binary serves authenticated MCP over Streamable HTTP and
stdio. Foundation tools are `context_identity`, `context_resource` and
`context_create_collection`. Upload/text-ingestion and job tools are documented in
[ingestion.md](ingestion.md). Full-format processing roles, retrieval, memory,
synthesis and content resources remain subsequent milestones.

## Operator setup

The example `examples/operator/development.json` references credentials by environment
name. Configure the PostgreSQL URL, load the sibling model credential into the launch
environment and set sandbox paths. Generate the exact provider snapshot as described
in integration-spike.md. The development example references both required initial
providers and verifies live descriptors at startup; it does not execute an inference
just to declare readiness. Models/bindings remain operator graph configuration.

From the repository root:

```console
mkdir -m 700 -p data
cargo run --locked -- migrate --config examples/operator/development.json
cargo run --locked -- bootstrap --config examples/operator/development.json --issuer context-service --subject development-agent --kind service --label Development
cargo run --locked -- issue-credential --config examples/operator/development.json --principal <principal-from-bootstrap> --scope read --scope share --scope ingest --output data/agent-token
cargo run --locked -- serve --config examples/operator/development.json --transport http
```

`bootstrap` is an explicit trusted local operator command. It creates a new tenant,
workspace and collection and grants only the chosen principal; membership alone
does not give other users broad rights. It never runs implicitly on server startup.
`issue-credential` writes a new private mode0600 file and refuses overwrite. Failure
to write revokes the newly issued credential. No secret is printed. Export the
credential into the configured `CONTEXT_STDIO_TOKEN` environment for stdio mode.
The deployment configuration remains operator-owned and contains no live secret.

Human tokens come from the configured issuer. Register/grant their exact issuer and
subject through operator commands; email/tenant-like claims do not grant access.

## Transport boundaries

HTTP authenticates every MCP request before SDK dispatch. It advertises public
protected-resource metadata at `/.well-known/oauth-protected-resource/mcp`, emits
the Bearer metadata challenge for unauthorized requests, validates configured hosts
and exact browser origins, and applies CORS/body/deadline bounds. Stateless HTTP
uses standard JSON request/response behavior; durable job state belongs to the
application and HTLK, not protocol sessions.

Stdio requires an explicitly configured service credential, revalidated for every
operation. Stdout carries only MCP traffic; operational logs use stderr. Both modes
negotiate MCP2025-11-25, return structured object outputs and redacted tool errors,
and expose the same exact descriptors. Every client/provider MCP operation performed
by this project goes through HTLK; the transport layer creates no outbound MCP client.

Export a catalog without starting any service or reading provider credentials:

```console
cargo run --locked -- catalog --transport http
cargo run --locked -- catalog --transport stdio
```

## Actual acceptance

The ignored external-service test launches the actual application binary and calls
it only through compiled/admitted HTLK role graphs over HTTP and stdio:

```console
# Set the isolated PostgreSQL/MinIO/Typesense fixture variables in ingestion.md.
cargo test -p context-mcp --test transports --locked -- --ignored
```

It verifies identity, protected metadata, another tenant's denial, credential-scope
intersection, revocation, sequential/concurrent idempotency and one created row per
key. It also verifies uploads, immutable finalization, text ingestion/publication,
job status and fenced replay/cleanup dispatch restrictions after host restart. Its
application-role fixtures are pure graphs, not proof of model/media processing.
Actual provider checks are recorded separately. It verifies the unauthenticated HTTP
challenge and resource metadata. No bearer token is printed.
