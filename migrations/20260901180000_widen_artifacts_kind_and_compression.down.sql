DROP TABLE artifacts;

CREATE TABLE artifacts (
    id TEXT PRIMARY KEY,
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    kind TEXT NOT NULL CHECK (kind IN ('stdout', 'stderr', 'crash_log')),
    original_filename TEXT,
    storage_path TEXT NOT NULL,
    media_type TEXT,
    created_at TEXT NOT NULL,
    metadata TEXT CHECK (metadata IS NULL OR json_valid(metadata)),
    UNIQUE (sha256, size_bytes)
);
