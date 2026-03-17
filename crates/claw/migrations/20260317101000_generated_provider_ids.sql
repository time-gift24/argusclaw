PRAGMA defer_foreign_keys = ON;

CREATE TABLE _provider_id_migration_map (
    old_id TEXT PRIMARY KEY,
    new_id TEXT NOT NULL UNIQUE
);

INSERT INTO _provider_id_migration_map (old_id, new_id)
SELECT id, lower(hex(randomblob(16)))
FROM llm_providers;

UPDATE agents
SET provider_id = (
    SELECT new_id
    FROM _provider_id_migration_map
    WHERE old_id = agents.provider_id
)
WHERE provider_id IN (SELECT old_id FROM _provider_id_migration_map);

UPDATE threads
SET provider_id = (
    SELECT new_id
    FROM _provider_id_migration_map
    WHERE old_id = threads.provider_id
)
WHERE provider_id IN (SELECT old_id FROM _provider_id_migration_map);

UPDATE llm_providers
SET id = (
    SELECT new_id
    FROM _provider_id_migration_map
    WHERE old_id = llm_providers.id
)
WHERE id IN (SELECT old_id FROM _provider_id_migration_map);

DROP TABLE _provider_id_migration_map;
