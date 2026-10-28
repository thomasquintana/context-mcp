-- A late cleanup/retry must never delete another job's unpublished derivatives.
ALTER TABLE context.content ADD COLUMN producer_job uuid REFERENCES context.jobs(id) ON DELETE SET NULL;
CREATE INDEX content_producer_job ON context.content(producer_job) WHERE producer_job IS NOT NULL;
