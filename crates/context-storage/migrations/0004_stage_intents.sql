-- Canonical intent bridges the PostgreSQL/HTLK admission transaction gap.
-- Clear it after saving the durable HTLK run; purge cascades with the job.
ALTER TABLE context.workflow_stages ADD COLUMN request jsonb;
