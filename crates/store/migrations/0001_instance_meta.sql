-- Instance-level key/value metadata (instance id, session-claim state, …).
-- Domain entities arrive in later migrations (phase 0 item 2).
CREATE TABLE instance_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
