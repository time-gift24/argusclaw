PRAGMA defer_foreign_keys = ON;

CREATE TABLE _agent_id_migration_map (
    old_id TEXT PRIMARY KEY,
    new_id TEXT NOT NULL UNIQUE
);

INSERT INTO _agent_id_migration_map (old_id, new_id)
SELECT id, lower(hex(randomblob(16)))
FROM agents
WHERE id != 'arguswing';

UPDATE jobs
SET agent_id = (
    SELECT new_id
    FROM _agent_id_migration_map
    WHERE old_id = jobs.agent_id
)
WHERE agent_id IN (SELECT old_id FROM _agent_id_migration_map);

UPDATE agents
SET id = (
    SELECT new_id
    FROM _agent_id_migration_map
    WHERE old_id = agents.id
)
WHERE id IN (SELECT old_id FROM _agent_id_migration_map);

DROP TABLE _agent_id_migration_map;
