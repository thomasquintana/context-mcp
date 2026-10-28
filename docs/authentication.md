# Authentication and transactional authorization

## Provider-neutral issuer support

`context-auth` uses trusted OIDC discovery and either asymmetric JWT/JWKS validation
or standard RFC7662 introspection. Configuration supplies issuer, dedicated API
audience, a scope-to-capability mapping, optional access-token claim markers and an
explicit HTTP opt-in for the local test issuer. Issuer metadata must match the exact
configured issuer. Token headers never select arbitrary key/discovery URLs.

JWT validation permits RS256, ES256 and EdDSA, checks signatures, issuer, audience,
required subject/expiry and not-before, and rejects unsupported token/key purposes.
JWKS responses are bounded and cached for five minutes; unknown-key refresh is
rate-limited. Introspection uses the discovered endpoint, configured client identity
and an environment-referenced secret, and checks active state, audience, expiry,
subject, issuer when supplied, and effective access scopes.

Access audiences must identify this API rather than the browser login client.
Issuer-specific markers are ordinary configuration (e.g. Keycloak JWT `typ=Bearer`
or another issuer's `token_use=access`). OIDC ID tokens are not API credentials.
Unknown scopes grant nothing; tenant-like token claims do not become memberships.
Every external identity is keyed by issuer plus subject, without automatic resource
or tenant grants.

## Agent credentials

Existing application service accounts can receive random 256-bit `ctx_` credentials
with explicit scopes and finite expiry (up to 90 days per issuance). Only SHA-256
digests are stored. A credential's secret is deliberately exposed once through its
intended operator/client channel; it has no Debug or Serialize implementation.
Rotation issues another credential and revokes the old identifier. Revocation is
owner-scoped and idempotent.

Authenticating a credential does not grant resources. The effective permission is
the intersection of its current stored scopes, caller-authenticated scope ceiling,
memberships and resource ACLs. Service-credential expiry/revocation is checked again
inside the protected data transaction, closing the authentication/read timing gap.
Agent-only deployments can omit issuer configuration; authentication remains
required and no default administrator is created.

## Shared transaction boundary

Use `Database::authorized_read` or `authorized_write` rather than combining separate
membership and ACL hydration calls. The returned `AuthorizedTransaction` owns both
authorization facts and protected SQL operations. Additional source checks use the
same snapshot, including all synthesis dependencies. Reads use repeatable-read,
read-only transactions. Mutations use serializable transactions and target locks.
Serialization failures require a bounded application retry before external effects;
they do not authorize replay of a provider call.

This prevents a request from seeing membership before revocation and a newly added
resource grant after revocation—a combination that never actually existed. Missing,
forbidden, expired and revoked authority fails closed. Request handlers still need
to bind authenticated sessions to every operation and enforce ownership-specific
rules; these libraries do not claim the HTTP/MCP transport boundary is complete.

## Real development issuer

`deploy/keycloak/context-dev-realm.json` is an operator-owned local test fixture.
All included passwords/client secrets are public development values. It configures
the mandatory subject mapping, API audience and an additional introspection-client
audience required by Keycloak 26.8.1. It does not disable issuer audience checks.
Keycloak is optional infrastructure in production.

The actual tested image is Keycloak26.8.1,
`sha256:79c623a22abacf3fe6c00a5fb30f5b7843d717b234033b80995891c06116d244`.

```console
CONTEXT_TEST_DATABASE_URL=postgres://context:context-test-only@127.0.0.1:55432/context_authority CONTEXT_TEST_OIDC_ISSUER=http://127.0.0.1:58080/realms/context-dev CONTEXT_TEST_OIDC_CLIENT_SECRET=context-development-client-secret cargo test -p context-auth --locked -- --include-ignored
```

Actual tests verify JWT acceptance and stable principal mapping, no implicit tenant
membership, ID-token/audience/tampering rejection, live introspection and inactive
token rejection, and a newly rotated signing key through the cached verifier.
Rotation creates/removes only a test-owned key component in the disposable issuer.
Separate PostgreSQL tests verify scope intersection, revocation/expiry at protected
read time and the mixed-snapshot membership/grant race. No bearer token is printed.
