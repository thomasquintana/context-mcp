-- Application authority only. HTLK execution history belongs to its SQLite store.
CREATE SCHEMA context;

CREATE TABLE context.principals (
    id uuid PRIMARY KEY,
    issuer text NOT NULL,
    subject text NOT NULL,
    kind text NOT NULL CHECK (kind IN ('human', 'service')),
    UNIQUE (issuer, subject)
);

CREATE TABLE context.resources (
    id uuid PRIMARY KEY,
    kind text NOT NULL CHECK (kind IN ('tenant', 'workspace', 'collection', 'asset')),
    owner uuid NOT NULL REFERENCES context.resources(id) DEFERRABLE INITIALLY DEFERRED,
    parent uuid REFERENCES context.resources(id) ON DELETE CASCADE,
    inherit boolean NOT NULL DEFAULT true,
    acl_version bigint NOT NULL DEFAULT 1 CHECK (acl_version > 0),
    purging boolean NOT NULL DEFAULT false,
    CHECK ((kind = 'tenant' AND owner = id AND parent IS NULL) OR
           (kind <> 'tenant' AND parent IS NOT NULL))
);

CREATE FUNCTION context.check_resource_parent() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE parent_kind text; parent_owner uuid;
BEGIN
    IF TG_OP = 'UPDATE' THEN
        IF NEW.id <> OLD.id OR NEW.kind <> OLD.kind OR NEW.owner <> OLD.owner OR
           NEW.parent IS DISTINCT FROM OLD.parent THEN
            RAISE EXCEPTION 'resource identity and ancestry are immutable' USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    IF NEW.kind = 'tenant' THEN RETURN NEW; END IF;
    SELECT kind, owner INTO parent_kind, parent_owner FROM context.resources WHERE id = NEW.parent FOR KEY SHARE;
    IF EXISTS (
        WITH RECURSIVE ancestors AS (
            SELECT id,parent,purging FROM context.resources WHERE id=NEW.parent
            UNION ALL SELECT r.id,r.parent,r.purging FROM context.resources r JOIN ancestors a ON r.id=a.parent
        ) SELECT 1 FROM ancestors WHERE purging
    ) THEN
        RAISE EXCEPTION 'resource parent is unavailable' USING ERRCODE = '23514';
    END IF;
    IF parent_owner IS DISTINCT FROM NEW.owner OR
       (NEW.kind = 'workspace' AND parent_kind <> 'tenant') OR
       (NEW.kind = 'collection' AND parent_kind <> 'workspace') OR
       (NEW.kind = 'asset' AND parent_kind <> 'collection') THEN
        RAISE EXCEPTION 'resource parent or owner is invalid' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER resource_parent BEFORE INSERT OR UPDATE ON context.resources
FOR EACH ROW EXECUTE FUNCTION context.check_resource_parent();
CREATE INDEX resources_parent ON context.resources(parent);

CREATE TABLE context.memberships (
    principal uuid NOT NULL REFERENCES context.principals(id) ON DELETE CASCADE,
    resource uuid NOT NULL REFERENCES context.resources(id) ON DELETE CASCADE,
    PRIMARY KEY (principal, resource)
);

CREATE TABLE context.grants (
    resource uuid NOT NULL REFERENCES context.resources(id) ON DELETE CASCADE,
    recipient_kind text NOT NULL CHECK (recipient_kind IN ('principal', 'tenant', 'workspace')),
    recipient uuid NOT NULL,
    permissions text[] NOT NULL CHECK (permissions <@ ARRAY['read','ingest','write_memory','synthesize','share','purge']::text[]),
    PRIMARY KEY (resource, recipient_kind, recipient)
);

CREATE TABLE context.credentials (
    id uuid PRIMARY KEY,
    principal uuid NOT NULL REFERENCES context.principals(id) ON DELETE CASCADE,
    digest bytea NOT NULL UNIQUE CHECK (octet_length(digest) = 32),
    scopes text[] NOT NULL CHECK (scopes <@ ARRAY['read','ingest','write_memory','synthesize','share','purge']::text[]),
    expires_at timestamptz NOT NULL,
    revoked boolean NOT NULL DEFAULT false
);

CREATE TABLE context.assets (
    id uuid PRIMARY KEY REFERENCES context.resources(id) ON DELETE CASCADE,
    generation bigint NOT NULL DEFAULT 1 CHECK (generation > 0),
    current_revision uuid
);

CREATE TABLE context.content (
    id uuid PRIMARY KEY,
    workspace uuid NOT NULL REFERENCES context.resources(id) ON DELETE CASCADE,
    asset uuid REFERENCES context.assets(id) ON DELETE CASCADE,
    revision uuid,
    kind text NOT NULL CHECK (kind IN ('source','chunk','memory','summary','entity','media')),
    origin text NOT NULL CHECK (origin IN ('original_evidence','agent_assertion','generated')),
    body jsonb NOT NULL,
    published boolean NOT NULL DEFAULT false,
    embedding_space text,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX content_revision ON context.content(revision);
CREATE INDEX content_workspace ON context.content(workspace, published);

CREATE TABLE context.revisions (
    id uuid PRIMARY KEY,
    asset uuid NOT NULL REFERENCES context.assets(id) ON DELETE CASCADE,
    content uuid NOT NULL UNIQUE REFERENCES context.content(id) ON DELETE CASCADE,
    author uuid NOT NULL REFERENCES context.principals(id),
    media_type text NOT NULL,
    size_bytes bigint NOT NULL CHECK (size_bytes >= 0),
    sha256 bytea NOT NULL CHECK (octet_length(sha256) = 32),
    created_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE context.assets ADD CONSTRAINT current_revision_exists FOREIGN KEY (current_revision)
REFERENCES context.revisions(id) ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED;
ALTER TABLE context.content ADD CONSTRAINT content_revision_exists FOREIGN KEY (revision)
REFERENCES context.revisions(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED;

CREATE TABLE context.objects (
    object_key text PRIMARY KEY,
    content uuid NOT NULL REFERENCES context.content(id),
    size_bytes bigint NOT NULL CHECK (size_bytes >= 0)
);

CREATE TABLE context.dependencies (
    source uuid NOT NULL REFERENCES context.content(id) ON DELETE CASCADE,
    dependent uuid NOT NULL REFERENCES context.content(id) ON DELETE CASCADE,
    relationship text NOT NULL CHECK (relationship IN ('derivation','citation')),
    PRIMARY KEY (source, dependent, relationship)
);
CREATE INDEX dependencies_dependent ON context.dependencies(dependent);

CREATE TABLE context.memories (
    id uuid PRIMARY KEY,
    workspace uuid NOT NULL REFERENCES context.resources(id) ON DELETE CASCADE,
    version bigint NOT NULL DEFAULT 0 CHECK (version >= 0),
    current_revisions uuid[] NOT NULL DEFAULT '{}'
);
CREATE TABLE context.memory_revisions (
    id uuid PRIMARY KEY,
    memory uuid NOT NULL REFERENCES context.memories(id) ON DELETE CASCADE,
    content uuid NOT NULL UNIQUE REFERENCES context.content(id) ON DELETE CASCADE,
    version bigint NOT NULL CHECK (version > 0),
    author uuid NOT NULL REFERENCES context.principals(id),
    UNIQUE(memory, version)
);

CREATE FUNCTION context.immutable_revision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'source and memory revisions are immutable' USING ERRCODE = '23514';
END;
$$;
CREATE TRIGGER source_revision_immutable BEFORE UPDATE ON context.revisions
FOR EACH ROW EXECUTE FUNCTION context.immutable_revision();
CREATE TRIGGER memory_revision_immutable BEFORE UPDATE ON context.memory_revisions
FOR EACH ROW EXECUTE FUNCTION context.immutable_revision();

CREATE TABLE context.jobs (
    id uuid PRIMARY KEY,
    principal uuid NOT NULL REFERENCES context.principals(id),
    asset uuid REFERENCES context.assets(id) ON DELETE CASCADE,
    revision uuid REFERENCES context.revisions(id) ON DELETE CASCADE,
    generation bigint NOT NULL,
    kind text NOT NULL CHECK (kind IN ('ingest','reprocess','synthesis','purge')),
    state text NOT NULL CHECK (state IN ('accepted','running','succeeded','failed','cancelled')),
    stage text NOT NULL DEFAULT 'admitted',
    workflow text NOT NULL,
    runtime_run text,
    error_code text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX jobs_pending ON context.jobs(created_at) WHERE state = 'accepted';

CREATE TABLE context.receipts (
    principal uuid NOT NULL REFERENCES context.principals(id),
    operation text NOT NULL,
    idempotency_key text NOT NULL,
    request_digest bytea NOT NULL CHECK (octet_length(request_digest) = 32),
    receipt jsonb NOT NULL,
    PRIMARY KEY (principal, operation, idempotency_key)
);

CREATE TABLE context.uploads (
    id uuid PRIMARY KEY,
    principal uuid NOT NULL REFERENCES context.principals(id),
    asset uuid NOT NULL REFERENCES context.assets(id) ON DELETE CASCADE,
    revision uuid NOT NULL,
    object_key text NOT NULL UNIQUE,
    expected_size bigint NOT NULL CHECK (expected_size >= 0),
    expected_sha256 bytea NOT NULL CHECK (octet_length(expected_sha256) = 32),
    media_type text NOT NULL,
    workflow text NOT NULL,
    expires_at timestamptz NOT NULL,
    finalized boolean NOT NULL DEFAULT false
);

-- Durable cleanup work, containing only generated IDs/keys, never retained bodies.
CREATE TABLE context.purges (
    id uuid PRIMARY KEY,
    principal uuid NOT NULL REFERENCES context.principals(id),
    manifest jsonb NOT NULL,
    not_before timestamptz NOT NULL DEFAULT now(),
    completed boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now()
);
