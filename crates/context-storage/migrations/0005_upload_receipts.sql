ALTER TABLE context.uploads ADD COLUMN job uuid REFERENCES context.jobs(id) ON DELETE SET NULL;
