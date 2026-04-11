-- Flatten subagent persistence by introducing subagent_names without rebuilding the
-- agents table, so existing foreign-key references remain valid during migration.

ALTER TABLE agents ADD COLUMN subagent_names TEXT NOT NULL DEFAULT '[]';

UPDATE agents AS parent
SET subagent_names = COALESCE(
    (
        SELECT json_group_array(children.display_name)
        FROM (
            SELECT child.display_name
            FROM agents AS child
            WHERE child.parent_agent_id = parent.id
            ORDER BY child.display_name
        ) AS children
    ),
    '[]'
);
