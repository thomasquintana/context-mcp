ALTER TABLE context.resources ADD COLUMN label text NOT NULL DEFAULT '' CHECK (length(label) <= 256);
ALTER TABLE context.memories ADD COLUMN subject_normalized text;
CREATE UNIQUE INDEX memory_subject_unique ON context.memories(workspace,subject_normalized)
WHERE subject_normalized IS NOT NULL;

ALTER TABLE context.jobs DROP CONSTRAINT jobs_kind_check;
ALTER TABLE context.jobs ADD CONSTRAINT jobs_kind_check
CHECK (kind IN ('ingest','reprocess','synthesis','purge','query','memory','media'));
ALTER TABLE context.jobs ADD COLUMN scope uuid REFERENCES context.resources(id) ON DELETE CASCADE;
ALTER TABLE context.jobs ADD COLUMN auth_scopes text[] NOT NULL DEFAULT '{}';
ALTER TABLE context.jobs ADD COLUMN submitting_credential uuid REFERENCES context.credentials(id);
ALTER TABLE context.jobs ADD COLUMN deadline timestamptz NOT NULL DEFAULT now()+interval '7 days';

-- Trusted job grants contain only identity/source IDs and allowed role revisions.
CREATE TABLE context.workflow_authority (
    job uuid PRIMARY KEY REFERENCES context.jobs(id) ON DELETE CASCADE,
    authority jsonb NOT NULL,
    active boolean NOT NULL DEFAULT true
);

-- Raw provider inputs/results remain HTLK execution records, not duplicated here.
CREATE TABLE context.workflow_stages (
    job uuid NOT NULL REFERENCES context.jobs(id) ON DELETE CASCADE,
    stage text NOT NULL,
    role text NOT NULL,
    fingerprint text NOT NULL,
    request_digest bytea NOT NULL CHECK (octet_length(request_digest)=32),
    run_id text,
    PRIMARY KEY (job,stage)
);
