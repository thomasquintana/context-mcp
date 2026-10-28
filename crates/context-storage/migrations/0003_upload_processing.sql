ALTER TABLE context.uploads ADD COLUMN source_object_key text;
ALTER TABLE context.uploads ADD COLUMN generation bigint;
ALTER TABLE context.uploads ADD COLUMN profile jsonb;
CREATE UNIQUE INDEX upload_source_key ON context.uploads(source_object_key)
WHERE source_object_key IS NOT NULL;
ALTER TABLE context.jobs ADD COLUMN detail jsonb NOT NULL DEFAULT '{}';
