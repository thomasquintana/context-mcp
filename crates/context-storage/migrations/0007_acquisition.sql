-- Content-free tracked local acquisition files; restart/expiry cleanup is idempotent.
CREATE TABLE context.acquisitions (
    id uuid PRIMARY KEY,
    scope uuid NOT NULL REFERENCES context.resources(id) ON DELETE CASCADE,
    principal uuid NOT NULL REFERENCES context.principals(id),
    expires_at timestamptz NOT NULL DEFAULT now()+interval '10 minutes'
);
