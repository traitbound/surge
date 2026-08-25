-- The two credentials (INV-AUTH-1) plus the one-time claim token that mints
-- a session (INV-AUTH-5). Only hashes are stored — plaintext tokens exist
-- once, in the mint response / spawn env / terminal claim URL (INV-AUTH-4).
CREATE TABLE token (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT NOT NULL CHECK (kind IN ('session','runtime','claim')),
    token_hash TEXT NOT NULL UNIQUE,
    project_id TEXT REFERENCES project(id),
    created_at INTEGER NOT NULL,
    revoked_at INTEGER,
    -- Runtime tokens are per-project (INV-AUTH-1); the others are instance-level.
    CHECK ((kind = 'runtime') = (project_id IS NOT NULL))
) STRICT;
