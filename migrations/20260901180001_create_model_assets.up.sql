CREATE TABLE model_assets (
    id TEXT PRIMARY KEY,
    sha256 TEXT NOT NULL UNIQUE CHECK (length(sha256) = 64),
    original_name TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK (size_bytes > 0),
    model_format TEXT NOT NULL DEFAULT 'pte',
    storage_mode TEXT NOT NULL CHECK (storage_mode IN ('external', 'managed')),
    external_path TEXT,
    relative_path TEXT,
    file_modified_at TEXT,
    registered_at TEXT NOT NULL,
    last_verified_at TEXT,
    available INTEGER NOT NULL DEFAULT 1 CHECK (available IN (0, 1)),

    CHECK (
        (storage_mode = 'external'
            AND external_path IS NOT NULL
            AND relative_path IS NULL)
        OR
        (storage_mode = 'managed'
            AND external_path IS NULL
            AND relative_path IS NOT NULL)
    )
);

CREATE INDEX model_assets_available_idx ON model_assets (available);
