-- Create accounts table (single-user)
CREATE TABLE IF NOT EXISTS accounts (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    username    TEXT NOT NULL,
    password    BLOB NOT NULL,
    nonce       BLOB NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Create credentials table
CREATE TABLE IF NOT EXISTS credentials (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL UNIQUE,
    username    BLOB NOT NULL,
    password    BLOB NOT NULL,
    nonce       BLOB NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);
