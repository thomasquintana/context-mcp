ALTER TABLE context.content ADD COLUMN purging boolean NOT NULL DEFAULT false;
ALTER TABLE context.purges ADD COLUMN owner uuid;
ALTER TABLE context.purges ADD COLUMN scope uuid;
ALTER TABLE context.purges ADD COLUMN attempted_at timestamptz NOT NULL DEFAULT '-infinity';
CREATE TABLE context.purge_resources (
    purge uuid NOT NULL REFERENCES context.purges(id) ON DELETE CASCADE,
    resource uuid NOT NULL,
    PRIMARY KEY(purge,resource)
);
CREATE TABLE context.purge_items (
    purge uuid NOT NULL REFERENCES context.purges(id) ON DELETE CASCADE,
    content uuid NOT NULL,
    embedding_space text,
    PRIMARY KEY(purge,content)
);
CREATE TABLE context.purge_objects (
    purge uuid NOT NULL REFERENCES context.purges(id) ON DELETE CASCADE,
    object_key text NOT NULL,
    PRIMARY KEY(purge,object_key)
);
CREATE TABLE context.purge_jobs (
    purge uuid NOT NULL REFERENCES context.purges(id) ON DELETE CASCADE,
    job uuid NOT NULL,
    PRIMARY KEY(purge,job)
);
CREATE TABLE context.purge_acquisitions (
    purge uuid NOT NULL REFERENCES context.purges(id) ON DELETE CASCADE,
    acquisition uuid NOT NULL,
    PRIMARY KEY(purge,acquisition)
);
CREATE TABLE context.index_bindings (
    endpoint text NOT NULL,
    collection text NOT NULL,
    space text NOT NULL,
    dimensions integer NOT NULL,
    key_env text NOT NULL,
    PRIMARY KEY(endpoint,collection)
);
