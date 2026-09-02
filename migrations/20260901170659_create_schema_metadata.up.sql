CREATE TABLE schema_metadata (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version INTEGER NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT INTO schema_metadata (id, schema_version, updated_at)
VALUES (1, 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
