# Deployment and user CLI

## Runtime layout

One `context-mcp` instance owns authoritative PostgreSQL state, rebuildable Typesense
indexes, unversioned S3-compatible payload storage, private staging and HTLK SQLite.
The reference deployment uses actual llm-mcp and mcp-sandbox as child MCP providers;
all their calls/discovery remain HTLK-owned. Operators may replace role graphs,
catalogs and deployment bindings while preserving their application contracts.

`deploy/compose.yml` supplies PostgreSQL16, pinned-source MinIO, Typesense29.1 and
development Keycloak26.8.1. Default published backing-service ports are55432,59000,
58108 and58080. The optional `application` profile adds the server on8089. All bindings
default to loopback. Passwords/keys come from environment references and are not baked
into images or graph files. Production TLS, public resource URL, issuer/audience and
allowed hosts/origins are explicit operator settings.

The server image builds Rust1.98.0 with the locked graph, builds unchanged llm-mcp
revision `a9b6d2fe3fed686dd2ca2fb67591613f0cf44cb8`, and checks the official sandbox
v0.2.0 Linux archive SHA-256. MinIO is built from unmodified pinned source
`9e49d5e7a648f00e26f2246f4dc28e6b07f8c84a`; source-only upstream distribution does not
require a private registry image. Runtime/provider licenses remain separate and are
included in their images. Rust notices are in THIRD_PARTY_NOTICES.md. The measured
reference container platform is Linux/arm64; local development was verified on macOS/arm64.

## Prepare a local development server

Set environment values for `CONTEXT_POSTGRES_PASSWORD`, `CONTEXT_S3_ACCESS_KEY`,
`CONTEXT_S3_SECRET_KEY`, `CONTEXT_TYPESENSE_KEY`, and `CONTEXT_KEYCLOAK_PASSWORD`.
Set `CONTEXT_DATA_DIR` to an absolute private host directory and
`CONTEXT_CONTAINER_DATABASE_URL` to the container-network PostgreSQL URL (including
the configured password). Keep credentials in the launch environment/private secret
manager. For the host process, `CONTEXT_DATABASE_URL` uses localhost55432.

```sh
docker compose --file deploy/compose.yml up --detach --build postgres minio typesense keycloak
cargo build --workspace --all-features --locked
docker build --tag context-mcp-media:processing --file deploy/media/Dockerfile .
python3 tools/deployment_config.py data
python3 tools/processing_profile.py data/profiles/context.yml
```

The preparation command refuses to overwrite existing operator configs. It writes
private runtime/discovery files containing environment reference names, not live
credentials. For host development, set `ALLOW_DIRS` to the absolute `data/staging`
path, `SANDBOX_PROFILES_DIR` to `data/profiles`, and the Docker context's socket as
`DOCKER_HOST`. Inherit the model credential by the name `OPENAI_API_KEY`; it is never
an operation argument. See processing.md for native profile/image bounds.

```sh
target/debug/context-mcp provision-data --config data/runtime.json
target/debug/context-mcp migrate --config data/runtime.json
target/debug/context-mcp discover-providers --config data/discovery.json --output data/catalogs.json
target/debug/context-mcp bootstrap --config data/runtime.json --issuer context-service --subject agent --kind service --label Development
target/debug/context-mcp issue-credential --config data/runtime.json --principal <returned-principal> --scope read --scope ingest --scope share --scope write_memory --scope synthesize --scope purge --output data/agent-token
target/debug/context-mcp serve --config data/runtime.json --transport http
```

`register-identity` registers exact OIDC issuer/subject pairs without grants.
`membership` adds/removes tenant/workspace membership independently of resource ACLs.
`revoke-credential` revokes an owner-bound credential. These are explicit trusted-local
operator commands, never implicit startup enrollment. Public sharing uses the
versioned `context_set_access` application contract.

Host development config uses the local Keycloak fixture issuer. Production remains
provider-neutral. Configure the exact issuer/audience and identity grants as described
in authentication.md. ID tokens do not authenticate as API access tokens.

## Application container

```sh
docker compose --file deploy/compose.yml build application
python3 tools/deployment_config.py <absolute-data-directory> --container --public-url http://localhost:8089/mcp --public-s3-url http://localhost:59000
```

Run provision/migrate/discover/bootstrap/credential commands with
`docker compose --profile application run --rm --no-deps -T application ...`, then
`up --detach --no-build application`. Set `CONTEXT_UID`/`CONTEXT_GID` to the private
directory owner and `DOCKER_GID` to the socket group. Native processing requires the
Docker socket and an operator-pinned sandbox profile. The host staging path is mounted
at **the identical absolute path** inside the application so the sandbox Docker
backend binds the correct per-job host directory. SQLite/staging live under this
private persistent directory. Drain the one application before another transport or
replacement instance opens it; this is not a horizontally shared worker deployment.

Container preparation defaults to agent credentials (`oidc:null`); configure a real
externally reachable OIDC issuer for human access. The separate local Keycloak remains
available for host development and actual issuer verification. `s3_endpoint` is the
private backend address; `s3_public_endpoint` signs client-reachable byte URLs.
Internal text/URL ingestion signs transfers against the private endpoint, so returned
localhost URLs need not resolve inside the application container.

HTTP concurrency, body size and processing deadlines are bounded. Readiness is served
only after initialization and provider descriptor/bucket/index verification. Graceful
termination drains local owners while retaining durable work for restart. Keep old
role revisions/index credential references until unfinished work and retained managed
collections are cleaned. Store/bucket migrations must move managed data and maintain
the purge policy; versioned payload storage is rejected.

## Upgrading the embedded HTLK runtime

The upgrade from HTLK `3448acb5` to `1e405c49` changes its private SQLite schema
from 6 to 13 and changes executable/native implementation fingerprints. The new
runtime rejects old execution databases; startup never migrates, deletes or resets
them. PostgreSQL application records, payload storage and the derived search index
remain separate. Use a maintenance cutover for retained application data:

1. Keep the old binary/image, configuration, catalogs and processing profiles
   available. Stop new user/agent admission at the deployment boundary while the old
   instance's workers and maintenance finish existing work. Allow authenticated
   status/cancellation/finalization as needed to settle already accepted operations.
2. Under the old instance, finish or cancel active jobs and wait for cleanup to
   settle. Complete pending purges and unpublished-stage cleanup. Resolve existing
   uploads by finalizing them before the admission freeze or letting them expire
   and retire under the existing conditional-PUT fencing procedure. Wait for tracked
   URL acquisitions and transient requests to settle. A terminal job with pending
   cleanup, an unresolved run intent or an unconfirmed cleanup receipt blocks cutover.
3. Stop the old instance gracefully. Inspect authoritative operational state while
   it is stopped: no accepted/running jobs, active workflow authorities, pending
   purge/cleanup work, unresolved uploads/acquisitions or private job staging may
   require the old execution store. If any remains, resume the old instance and
   settle it; do not edit authority, stage handles or fingerprints to bypass it.
4. Preserve the stopped deployment's old SQLite database and any associated WAL/SHM
   files together with its exact binary/configuration. Retain this protected history
   for inspection with that matching binary; it stays outside application purge scope.
   Select a **new, unused** `application.workflows.database` path for schema 13.
   Keep PostgreSQL, S3, Typesense and permanent zero-byte upload fences intact.
5. Install the upgraded operator graphs and exact provider catalogs. Keep model/space
   identities consistent with retained vectors. Start the one upgraded instance and
   verify readiness, authorized reads of existing published evidence/memory, and new
   upload/processing/query/purge operations. A same-version restart reuses its new
   store and saved handles normally.

Frozen legacy failed/cancelled jobs cannot use `context_retry_job` with newly compiled
role fingerprints. The request is rejected rather than retargeted. Previously saved
idempotency receipts can still return their original control result; they do not
resume work. For a published source, explicitly request `context_reprocess` with its
expected current revision; otherwise explicitly resubmit original bytes/text or an
exact URL as a new operation with a new key. Never implicitly regenerate purged data.

A rollback before any new admissions can restore the old binary/configuration and
old database path after stopping the upgraded instance. After new work has been
accepted, first settle it under its matching runtime and review all retained workflow
selections before rollback. Mixing old/new handles or copying private HTLK tables is
not a supported migration. Disposable test deployments may be recreated explicitly;
that is a separate choice from this retained-data procedure.

## User and agent CLI

Load the bearer into `CONTEXT_TOKEN` (or use `--token-env` to select another reference).
The token never appears in command arguments. `call` reads a bounded JSON object from
stdin or `--arguments <file>` and supports every exported application tool:

```sh
target/debug/context-mcp call --endpoint https://context.example/mcp --tool context_query --arguments query.json
target/debug/context-mcp upload --endpoint https://context.example/mcp --file document.pdf --collection <collection-id> --idempotency-key document-v1 --wait
```

Use `--allow-http` explicitly for loopback development. Upload streams the file,
computes size/SHA-256, honors conditional headers, finalizes immutable bytes and can
poll durable processing. No outbound MCP client bypass exists: the CLI compiles/adopts
HTLK graphs pinned to the exported descriptors. Stdio serving uses
`serve --transport stdio` with explicit `CONTEXT_STDIO_TOKEN`; normal MCP clients retain
that connection. HTLK workflow consumers keep one run alive across dependent stdio
calls, as shown by examples/integration/stdio_context_journey.htlk.

Authenticated resource templates are `context://content/{id}`,
`context://asset/{asset}/revision/{revision}`, and `context://memory/{id}`. They expose
bounded protected JSON/byte URLs, not a public content inventory. Every read rechecks
current transitive source authority.
