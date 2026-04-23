CREATE TABLE IF NOT EXISTS admin_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    instance_name TEXT NOT NULL DEFAULT 'ArgusWing',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO admin_settings (id, instance_name)
VALUES (1, 'ArgusWing')
ON CONFLICT(id) DO NOTHING;
