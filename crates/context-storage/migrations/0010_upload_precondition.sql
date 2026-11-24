-- A predecessor is a compare-only condition, not a derivation or a retention edge.
-- NULL preserves the behavior of unguarded uploads and jobs.
ALTER TABLE context.uploads ADD COLUMN expected_revision uuid;
ALTER TABLE context.jobs ADD COLUMN expected_revision uuid;
