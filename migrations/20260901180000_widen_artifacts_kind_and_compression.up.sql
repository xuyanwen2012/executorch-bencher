-- The pre-release `runs` table references `artifacts` by foreign key, so
-- it must go first when this migration runs against a database that
-- already holds rows; `20260901180002` recreates it with the new shape.
DROP TABLE IF EXISTS runs;
DROP TABLE artifacts;

CREATE TABLE artifacts (
    id TEXT PRIMARY KEY,
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    kind TEXT NOT NULL CHECK (kind IN (
        'prompt', 'stdout', 'stderr', 'output', 'crash_log', 'logcat', 'correctness_report'
    )),
    original_filename TEXT,
    storage_path TEXT NOT NULL,
    media_type TEXT,
    compression TEXT NOT NULL DEFAULT 'none' CHECK (compression IN ('none', 'zstd')),
    created_at TEXT NOT NULL,
    metadata TEXT CHECK (metadata IS NULL OR json_valid(metadata)),
    UNIQUE (sha256, size_bytes)
);
