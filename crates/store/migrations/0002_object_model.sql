-- The twelve entities of design §03 (fourteen tables: Issue & WorkOrder and
-- Run & Span are paired headings). Closed vocabularies are CHECK-constrained
-- to mirror the domain enums; edge structures (edge, span.parent_span_id,
-- doc.parent_doc_id, node block members) are plain tables/self-references
-- traversed by recursive CTEs (ADR-2).
-- Timestamps are Unix milliseconds. Booleans are 0/1 INTEGER.

CREATE TABLE project (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL UNIQUE,
    repo_path         TEXT NOT NULL,
    assigned_pipeline_id TEXT REFERENCES pipeline(id),
    pipeline_status   TEXT NOT NULL DEFAULT 'published'
                      CHECK (pipeline_status IN ('published','stale')),
    surge_yaml_written INTEGER NOT NULL DEFAULT 0 CHECK (surge_yaml_written IN (0,1)),
    tracker           TEXT NOT NULL DEFAULT 'none'
                      CHECK (tracker IN ('linear','github','builtin','none')),
    branch_format     TEXT NOT NULL DEFAULT 'task/{issue}',
    created_at        INTEGER NOT NULL
) STRICT;

-- One row per published version (INV-DATA-3): (name, version) unique.
CREATE TABLE pipeline (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    version      INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    blessed      INTEGER NOT NULL DEFAULT 0 CHECK (blessed IN (0,1)),
    forked_from  TEXT REFERENCES pipeline(id),
    created_at   INTEGER NOT NULL,
    UNIQUE (name, version)
) STRICT;

-- x/y are presentation state — never part of the content hash (INV-ID-2).
-- Kind-specific config is canonical JSON tagged by the same 'kind' value.
CREATE TABLE node (
    id             TEXT NOT NULL,
    pipeline_id    TEXT NOT NULL REFERENCES pipeline(id) ON DELETE CASCADE,
    label          TEXT NOT NULL,
    x              REAL NOT NULL DEFAULT 0,
    y              REAL NOT NULL DEFAULT 0,
    human_gate     INTEGER NOT NULL DEFAULT 0 CHECK (human_gate IN (0,1)),
    emits_span     INTEGER NOT NULL DEFAULT 1 CHECK (emits_span IN (0,1)),
    metric_binding TEXT,
    metric_note    TEXT,
    kind           TEXT NOT NULL
                   CHECK (kind IN ('doc','agent','hook','skill','stage','block')),
    config         TEXT NOT NULL CHECK (json_valid(config)),
    PRIMARY KEY (pipeline_id, id)
) STRICT;

-- trigger is the raw vocabulary string; unknown values are 'custom' triggers.
CREATE TABLE edge (
    id            TEXT NOT NULL,
    pipeline_id   TEXT NOT NULL REFERENCES pipeline(id) ON DELETE CASCADE,
    from_node     TEXT NOT NULL,
    to_node       TEXT NOT NULL,
    trigger       TEXT NOT NULL,
    gate_required INTEGER NOT NULL DEFAULT 0 CHECK (gate_required IN (0,1)),
    PRIMARY KEY (pipeline_id, id),
    FOREIGN KEY (pipeline_id, from_node) REFERENCES node(pipeline_id, id) ON DELETE CASCADE,
    FOREIGN KEY (pipeline_id, to_node)   REFERENCES node(pipeline_id, id) ON DELETE CASCADE
) STRICT;

-- Immutable per version (INV-DATA-2); trust-gated on import (INV-AUTH-3).
CREATE TABLE library_item (
    id                TEXT PRIMARY KEY,
    kind              TEXT NOT NULL CHECK (kind IN ('hook','subagent','skill')),
    name              TEXT NOT NULL,
    version           INTEGER NOT NULL,
    body              TEXT NOT NULL,
    trust             TEXT NOT NULL DEFAULT 'local'
                      CHECK (trust IN ('local','imported_untrusted','imported_reviewed')),
    trust_reviewed_by TEXT,
    trust_reviewed_at INTEGER,
    created_at        INTEGER NOT NULL,
    UNIQUE (kind, name, version),
    CHECK ((trust = 'imported_reviewed') = (trust_reviewed_by IS NOT NULL)),
    CHECK ((trust = 'imported_reviewed') = (trust_reviewed_at IS NOT NULL))
) STRICT;

CREATE TABLE materialization (
    id           TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    cache_key    TEXT NOT NULL UNIQUE,
    pipeline_id  TEXT NOT NULL REFERENCES pipeline(id),
    project_id   TEXT NOT NULL REFERENCES project(id),
    signed_by    TEXT NOT NULL,
    fresh        INTEGER NOT NULL DEFAULT 1 CHECK (fresh IN (0,1)),
    created_at   INTEGER NOT NULL
) STRICT;

-- Doc chain: parent_doc_id self-reference, traversed by recursive CTE.
CREATE TABLE doc (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES project(id),
    node_id        TEXT NOT NULL,
    path           TEXT NOT NULL,
    parent_doc_id  TEXT REFERENCES doc(id),
    content_hash   TEXT,
    gate           TEXT NOT NULL DEFAULT 'pending' CHECK (gate IN ('pending','approved')),
    approved_by    TEXT,
    approved_at    INTEGER,
    parent_hash_at_approval TEXT,
    created_at     INTEGER NOT NULL,
    CHECK ((gate = 'approved') = (approved_by IS NOT NULL)),
    CHECK ((gate = 'approved') = (approved_at IS NOT NULL))
) STRICT;

-- Human-owned fields (disposition, priority) beside status, never inside it.
CREATE TABLE issue (
    id                  TEXT PRIMARY KEY,
    project_id          TEXT NOT NULL REFERENCES project(id),
    title               TEXT NOT NULL,
    wave                INTEGER NOT NULL,
    phase               TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'draft'
                        CHECK (status IN ('draft','eligible','dispatched','leased',
                                          'verifying','verified','failed','aborted','cut')),
    work_order_hash     TEXT NOT NULL,
    gate2               TEXT NOT NULL DEFAULT 'pending' CHECK (gate2 IN ('pending','reviewed')),
    gate2_reviewed_by   TEXT,
    gate2_reviewed_at   INTEGER,
    lease_owner         TEXT,
    lease_run_id        TEXT,
    lease_expires_at    INTEGER,
    lease_heartbeat_at  INTEGER,
    retry_count         INTEGER NOT NULL DEFAULT 0,
    disposition         TEXT,
    priority            INTEGER NOT NULL DEFAULT 0,
    is_wave_integration INTEGER NOT NULL DEFAULT 0 CHECK (is_wave_integration IN (0,1)),
    created_at          INTEGER NOT NULL,
    CHECK ((gate2 = 'reviewed') = (gate2_reviewed_by IS NOT NULL)),
    CHECK ((gate2 = 'reviewed') = (gate2_reviewed_at IS NOT NULL)),
    -- A lease is all-or-nothing (INV-EXEC-1/2).
    CHECK ((lease_owner IS NOT NULL) = (lease_run_id IS NOT NULL)),
    CHECK ((lease_owner IS NOT NULL) = (lease_expires_at IS NOT NULL)),
    CHECK ((lease_owner IS NOT NULL) = (lease_heartbeat_at IS NOT NULL))
) STRICT;

-- Revisions clear their Gate-2 review (enforced in the repository layer).
CREATE TABLE work_order (
    id           TEXT PRIMARY KEY,
    issue_id     TEXT NOT NULL REFERENCES issue(id) ON DELETE CASCADE,
    path         TEXT NOT NULL,
    revision     INTEGER NOT NULL DEFAULT 1,
    content_hash TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    UNIQUE (issue_id, revision)
) STRICT;

CREATE TABLE run (
    id                   TEXT PRIMARY KEY,
    project_id           TEXT NOT NULL REFERENCES project(id),
    issue_id             TEXT REFERENCES issue(id),
    kind                 TEXT NOT NULL CHECK (kind IN ('doc','work_order')),
    materialization_hash TEXT NOT NULL,
    work_order_hash      TEXT,
    status               TEXT NOT NULL DEFAULT 'running'
                         CHECK (status IN ('running','succeeded','failed','aborted','refused')),
    started_at           INTEGER NOT NULL,
    ended_at             INTEGER,
    cost                 REAL NOT NULL DEFAULT 0,
    -- Work-order runs carry an issue and a work-order hash; doc runs neither
    -- (design §23-Fourteen).
    CHECK ((kind = 'work_order') = (issue_id IS NOT NULL)),
    CHECK ((kind = 'work_order') = (work_order_hash IS NOT NULL))
) STRICT;

-- Span tree: parent_span_id self-reference, traversed by recursive CTE.
-- body is compactable; structure columns are kept forever (INV-OBS-2).
CREATE TABLE span (
    id              TEXT PRIMARY KEY,
    run_id          TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
    parent_span_id  TEXT REFERENCES span(id),
    node_id         TEXT,
    role            TEXT NOT NULL CHECK (role IN ('coordinator','worker','verifier')),
    started_at      INTEGER NOT NULL,
    duration_ms     INTEGER,
    status          TEXT NOT NULL DEFAULT 'running'
                    CHECK (status IN ('running','ok','error','refused')),
    cost            REAL NOT NULL DEFAULT 0,
    depth           INTEGER NOT NULL DEFAULT 0,
    policy_decision TEXT,
    body            TEXT
) STRICT;
CREATE INDEX span_by_run ON span(run_id);

CREATE TABLE coe (
    id           TEXT PRIMARY KEY,
    run_id       TEXT REFERENCES run(id),
    issue_id     TEXT REFERENCES issue(id),
    text         TEXT NOT NULL,
    ratchet      TEXT CHECK (ratchet IS NULL OR json_valid(ratchet)),
    created_at   INTEGER NOT NULL,
    CHECK (run_id IS NOT NULL OR issue_id IS NOT NULL)
) STRICT;

-- INV-OBS-1: every privileged act writes one. Append-only by discipline.
CREATE TABLE audit_entry (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    action     TEXT NOT NULL,
    subject    TEXT NOT NULL,
    actor      TEXT NOT NULL,
    project_id TEXT REFERENCES project(id),
    at         INTEGER NOT NULL
) STRICT;

-- Mirrored, never written back (INV-DATA-5). labels is a JSON array.
CREATE TABLE plan_issue (
    id              TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL REFERENCES project(id),
    number          TEXT NOT NULL,
    title           TEXT NOT NULL,
    labels          TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(labels)),
    milestone       TEXT,
    assignee        TEXT,
    pr_state        TEXT,
    commit_count    INTEGER NOT NULL DEFAULT 0,
    sprint          TEXT,
    planning_status TEXT,
    linked_issue_id TEXT REFERENCES issue(id),
    mirrored_at     INTEGER NOT NULL,
    UNIQUE (project_id, number)
) STRICT;
