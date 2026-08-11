PRAGMA application_id = 1129337676;
PRAGMA user_version = 2;
PRAGMA foreign_keys = ON;

CREATE TABLE capsule_manifest (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    format_id           TEXT NOT NULL,
    format_version      TEXT NOT NULL,
    capsule_id          TEXT NOT NULL UNIQUE,
    title               TEXT NOT NULL,
    summary             TEXT NOT NULL,
    app_id              TEXT NOT NULL,
    app_version         TEXT NOT NULL,
    entry_asset         TEXT NOT NULL,
    runtime_protocol    TEXT NOT NULL,
    permissions_json    TEXT NOT NULL CHECK (json_valid(permissions_json)),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE TABLE capsule_grant (
    capability          TEXT PRIMARY KEY NOT NULL,
    decision             TEXT NOT NULL CHECK (decision IN ('allow', 'deny', 'prompt')),
    reason               TEXT NOT NULL,
    granted_at           TEXT
);

CREATE TABLE capsule_asset (
    path                TEXT PRIMARY KEY NOT NULL,
    media_type          TEXT NOT NULL,
    content             BLOB NOT NULL,
    sha256              TEXT NOT NULL CHECK (length(sha256) = 64),
    executable          INTEGER NOT NULL DEFAULT 0 CHECK (executable IN (0, 1)),
    cache_policy        TEXT NOT NULL DEFAULT 'no-store',
    description         TEXT,
    CHECK (length(path) > 0),
    CHECK (substr(path, 1, 1) <> '/'),
    CHECK (instr(path, char(92)) = 0),
    CHECK (path <> '..'),
    CHECK (path NOT LIKE '../%'),
    CHECK (path NOT LIKE '%/../%'),
    CHECK (path NOT LIKE '%/..')
);

CREATE TABLE capsule_command (
    id                  TEXT PRIMARY KEY NOT NULL,
    purpose             TEXT NOT NULL,
    platform            TEXT NOT NULL DEFAULT 'any',
    cwd                 TEXT NOT NULL DEFAULT '{repo}',
    command_template    TEXT NOT NULL,
    argv_json           TEXT CHECK (argv_json IS NULL OR json_valid(argv_json)),
    risk_class          TEXT NOT NULL CHECK (risk_class IN ('read-only', 'local-execute', 'write', 'external-effect')),
    success_condition   TEXT NOT NULL
);

CREATE TABLE capsule_runbook (
    id                  TEXT PRIMARY KEY NOT NULL,
    audience            TEXT NOT NULL CHECK (audience IN ('agent', 'human', 'runtime', 'all')),
    sequence            INTEGER NOT NULL,
    title               TEXT NOT NULL,
    body_md             TEXT NOT NULL,
    command_id          TEXT REFERENCES capsule_command(id),
    UNIQUE (audience, sequence)
);

CREATE TABLE capsule_doc (
    slug                TEXT PRIMARY KEY NOT NULL,
    title               TEXT NOT NULL,
    media_type          TEXT NOT NULL DEFAULT 'text/markdown',
    content             TEXT NOT NULL,
    sequence            INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE capsule_endpoint (
    name                TEXT PRIMARY KEY NOT NULL,
    operation           TEXT NOT NULL CHECK (operation IN ('read', 'write')),
    sql_text            TEXT NOT NULL,
    parameters_json     TEXT NOT NULL CHECK (json_valid(parameters_json)),
    result_mode         TEXT NOT NULL CHECK (result_mode IN ('rows', 'row', 'scalar', 'changes')),
    description         TEXT NOT NULL,
    enabled             INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);

CREATE TABLE capsule_endpoint_step (
    endpoint_name       TEXT NOT NULL REFERENCES capsule_endpoint(name) ON DELETE CASCADE,
    sequence            INTEGER NOT NULL CHECK (sequence > 0 AND sequence <= 16),
    sql_text            TEXT NOT NULL,
    required_changes    INTEGER CHECK (required_changes IS NULL OR required_changes >= 0),
    PRIMARY KEY (endpoint_name, sequence)
);

CREATE TABLE capsule_check (
    id                  TEXT PRIMARY KEY NOT NULL,
    severity            TEXT NOT NULL CHECK (severity IN ('error', 'warning', 'info')),
    description         TEXT NOT NULL,
    sql_text            TEXT NOT NULL,
    result_mode         TEXT NOT NULL CHECK (result_mode IN ('scalar', 'rows', 'empty')),
    expected_json       TEXT NOT NULL CHECK (json_valid(expected_json))
);

CREATE TABLE capsule_prompt (
    id                  TEXT PRIMARY KEY NOT NULL,
    title               TEXT NOT NULL,
    prompt_text         TEXT NOT NULL,
    sequence            INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE capsule_change_log (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint_name       TEXT NOT NULL,
    parameters_json     TEXT NOT NULL CHECK (json_valid(parameters_json)),
    changed_rows        INTEGER NOT NULL,
    occurred_at         TEXT NOT NULL
);

CREATE VIEW START_HERE AS
SELECT
    r.sequence,
    r.audience,
    r.title,
    r.body_md AS instruction,
    r.command_id,
    c.purpose AS command_purpose,
    c.platform,
    c.cwd,
    c.command_template,
    c.argv_json,
    c.risk_class,
    c.success_condition
FROM capsule_runbook AS r
LEFT JOIN capsule_command AS c ON c.id = r.command_id
WHERE r.audience IN ('agent', 'all')
ORDER BY r.sequence, r.id;

CREATE INDEX idx_capsule_runbook_sequence
    ON capsule_runbook(audience, sequence);

CREATE INDEX idx_capsule_doc_sequence
    ON capsule_doc(sequence, slug);

CREATE INDEX idx_capsule_prompt_sequence
    ON capsule_prompt(sequence, id);

CREATE INDEX idx_capsule_endpoint_step_order
    ON capsule_endpoint_step(endpoint_name, sequence);

CREATE INDEX idx_capsule_change_log_time
    ON capsule_change_log(occurred_at, id);
