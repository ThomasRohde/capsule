-- Draft SQLite Capsule format v0.3.
-- M00/M01 must reconcile this proposal with the live verifier and accepted ADRs.
PRAGMA application_id = 1129337676;
PRAGMA user_version = 3;
PRAGMA foreign_keys = ON;

CREATE TABLE capsule_manifest (
    id                      INTEGER PRIMARY KEY CHECK (id = 1),
    format_id               TEXT NOT NULL CHECK (format_id = 'org.sqlite-capsule'),
    format_version          TEXT NOT NULL CHECK (format_version = '0.3'),
    app_id                  TEXT NOT NULL,
    app_version             TEXT NOT NULL,
    entry_asset             TEXT NOT NULL,
    runtime_protocol        TEXT NOT NULL,
    permissions_json        TEXT NOT NULL CHECK (json_valid(permissions_json)),
    data_schema_id          TEXT NOT NULL,
    data_schema_version     INTEGER NOT NULL CHECK (data_schema_version >= 1),
    minimum_host_profile    TEXT NOT NULL,
    released_at             TEXT NOT NULL
);

CREATE TABLE capsule_application (
    id                      INTEGER PRIMARY KEY CHECK (id = 1),
    name                    TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 256),
    description             TEXT NOT NULL CHECK (length(description) <= 4096),
    category                TEXT NOT NULL CHECK (length(category) BETWEEN 1 AND 128),
    icon_asset              TEXT,
    release_notes_doc       TEXT
);

CREATE TABLE capsule_instance_asset (
    id                      TEXT PRIMARY KEY NOT NULL,
    media_type              TEXT NOT NULL CHECK (media_type IN ('image/png', 'image/webp')),
    content                 BLOB NOT NULL CHECK (length(content) <= 524288),
    sha256                  TEXT NOT NULL CHECK (
                                length(sha256) = 64
                                AND sha256 NOT GLOB '*[^0-9a-f]*'
                            ),
    width                   INTEGER NOT NULL CHECK (width BETWEEN 1 AND 1024),
    height                  INTEGER NOT NULL CHECK (height BETWEEN 1 AND 1024),
    description             TEXT NOT NULL DEFAULT '' CHECK (length(description) <= 512)
);

CREATE TABLE capsule_instance (
    id                      INTEGER PRIMARY KEY CHECK (id = 1),
    capsule_id              TEXT NOT NULL UNIQUE,
    revision_id             TEXT NOT NULL UNIQUE,
    title                   TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 512),
    description             TEXT NOT NULL CHECK (length(description) <= 8192),
    document_kind           TEXT NOT NULL CHECK (length(document_kind) BETWEEN 1 AND 128),
    tags_json               TEXT NOT NULL CHECK (json_valid(tags_json)),
    icon_asset_id           TEXT REFERENCES capsule_instance_asset(id),
    created_at              TEXT NOT NULL,
    content_updated_at      TEXT NOT NULL
);

CREATE TABLE capsule_grant (
    capability              TEXT PRIMARY KEY NOT NULL,
    decision                TEXT NOT NULL CHECK (decision IN ('allow', 'deny', 'prompt')),
    reason                  TEXT NOT NULL,
    granted_at              TEXT
);

CREATE TABLE capsule_asset (
    path                    TEXT PRIMARY KEY NOT NULL,
    media_type              TEXT NOT NULL,
    content                 BLOB NOT NULL,
    sha256                  TEXT NOT NULL CHECK (
                                length(sha256) = 64
                                AND sha256 NOT GLOB '*[^0-9a-f]*'
                            ),
    executable              INTEGER NOT NULL DEFAULT 0 CHECK (executable IN (0, 1)),
    cache_policy            TEXT NOT NULL DEFAULT 'no-store',
    description             TEXT,
    CHECK (length(path) > 0),
    CHECK (substr(path, 1, 1) <> '/'),
    CHECK (instr(path, char(92)) = 0),
    CHECK (path <> '..'),
    CHECK (path NOT LIKE '../%'),
    CHECK (path NOT LIKE '%/../%'),
    CHECK (path NOT LIKE '%/..')
);

CREATE TABLE capsule_command (
    id                      TEXT PRIMARY KEY NOT NULL,
    purpose                 TEXT NOT NULL,
    platform                TEXT NOT NULL DEFAULT 'any',
    cwd                     TEXT NOT NULL DEFAULT '{repo}',
    command_template        TEXT NOT NULL,
    argv_json               TEXT CHECK (argv_json IS NULL OR json_valid(argv_json)),
    risk_class              TEXT NOT NULL CHECK (
                                risk_class IN ('read-only', 'local-execute', 'write', 'external-effect')
                            ),
    success_condition       TEXT NOT NULL
);

CREATE TABLE capsule_runbook (
    id                      TEXT PRIMARY KEY NOT NULL,
    audience                TEXT NOT NULL CHECK (audience IN ('agent', 'human', 'runtime', 'all')),
    sequence                INTEGER NOT NULL,
    title                   TEXT NOT NULL,
    body_md                 TEXT NOT NULL,
    command_id              TEXT REFERENCES capsule_command(id),
    UNIQUE (audience, sequence)
);

CREATE TABLE capsule_doc (
    slug                    TEXT PRIMARY KEY NOT NULL,
    title                   TEXT NOT NULL,
    media_type              TEXT NOT NULL DEFAULT 'text/markdown',
    content                 TEXT NOT NULL,
    sequence                INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE capsule_endpoint (
    name                    TEXT PRIMARY KEY NOT NULL,
    operation               TEXT NOT NULL CHECK (operation IN ('read', 'write')),
    sql_text                TEXT NOT NULL,
    parameters_json         TEXT NOT NULL CHECK (json_valid(parameters_json)),
    result_mode             TEXT NOT NULL CHECK (result_mode IN ('rows', 'row', 'scalar', 'changes')),
    description             TEXT NOT NULL,
    enabled                 INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);

CREATE TABLE capsule_endpoint_step (
    endpoint_name           TEXT NOT NULL REFERENCES capsule_endpoint(name) ON DELETE CASCADE,
    sequence                INTEGER NOT NULL CHECK (sequence > 0 AND sequence <= 16),
    sql_text                TEXT NOT NULL,
    required_changes        INTEGER CHECK (required_changes IS NULL OR required_changes >= 0),
    PRIMARY KEY (endpoint_name, sequence)
);

CREATE TABLE capsule_check (
    id                      TEXT PRIMARY KEY NOT NULL,
    severity                TEXT NOT NULL CHECK (severity IN ('error', 'warning', 'info')),
    description             TEXT NOT NULL,
    sql_text                TEXT NOT NULL,
    result_mode             TEXT NOT NULL CHECK (result_mode IN ('scalar', 'rows', 'empty')),
    expected_json           TEXT NOT NULL CHECK (json_valid(expected_json))
);

CREATE TABLE capsule_prompt (
    id                      TEXT PRIMARY KEY NOT NULL,
    title                   TEXT NOT NULL,
    prompt_text             TEXT NOT NULL,
    sequence                INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE capsule_dataset (
    id                      TEXT PRIMARY KEY NOT NULL,
    role                    TEXT NOT NULL CHECK (
                                role IN ('seed', 'user-content', 'settings', 'history', 'derived', 'cache')
                            ),
    description             TEXT NOT NULL CHECK (length(description) <= 2048),
    fork_policy             TEXT NOT NULL CHECK (
                                fork_policy IN ('copy', 'reset', 'omit', 'prompt', 'forbid')
                            ),
    compare_policy          TEXT NOT NULL CHECK (
                                compare_policy IN ('ignore', 'summary', 'row', 'field')
                            ),
    reconcile_policy        TEXT NOT NULL CHECK (
                                reconcile_policy IN ('ignore', 'manual', 'three-way', 'forbid')
                            ),
    upgrade_policy          TEXT NOT NULL CHECK (
                                upgrade_policy IN ('copy', 'target', 'migrate', 'rebuild', 'omit', 'forbid')
                            ),
    sensitivity             TEXT NOT NULL CHECK (sensitivity IN ('normal', 'sensitive')),
    required                INTEGER NOT NULL DEFAULT 0 CHECK (required IN (0, 1))
);

CREATE TABLE capsule_dataset_table (
    dataset_id              TEXT NOT NULL REFERENCES capsule_dataset(id) ON DELETE CASCADE,
    table_name              TEXT NOT NULL UNIQUE,
    sequence                INTEGER NOT NULL CHECK (sequence >= 0),
    primary_key_json        TEXT NOT NULL CHECK (json_valid(primary_key_json)),
    ignored_columns_json    TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(ignored_columns_json)),
    immutable_columns_json  TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(immutable_columns_json)),
    PRIMARY KEY (dataset_id, table_name)
);

CREATE TABLE capsule_dataset_dependency (
    dataset_id              TEXT NOT NULL REFERENCES capsule_dataset(id) ON DELETE CASCADE,
    depends_on_dataset_id   TEXT NOT NULL REFERENCES capsule_dataset(id),
    reason                  TEXT NOT NULL,
    PRIMARY KEY (dataset_id, depends_on_dataset_id),
    CHECK (dataset_id <> depends_on_dataset_id)
);

CREATE TABLE capsule_migration (
    id                      TEXT PRIMARY KEY NOT NULL,
    data_schema_id          TEXT NOT NULL,
    from_version            INTEGER NOT NULL CHECK (from_version >= 1),
    to_version              INTEGER NOT NULL CHECK (to_version >= 1 AND to_version <> from_version),
    description             TEXT NOT NULL,
    operation_profile       TEXT NOT NULL CHECK (
                                operation_profile = 'org.sqlite-capsule.migration-ops/1'
                            ),
    reversible              INTEGER NOT NULL DEFAULT 0 CHECK (reversible IN (0, 1)),
    UNIQUE (data_schema_id, from_version, to_version)
);

CREATE TABLE capsule_migration_step (
    migration_id            TEXT NOT NULL REFERENCES capsule_migration(id) ON DELETE CASCADE,
    sequence                INTEGER NOT NULL CHECK (sequence > 0 AND sequence <= 256),
    operation               TEXT NOT NULL CHECK (
                                operation IN (
                                    'copy_rows',
                                    'copy_dataset',
                                    'rebuild_dataset',
                                    'discard_dataset'
                                )
                            ),
    definition_json         TEXT NOT NULL CHECK (json_valid(definition_json)),
    PRIMARY KEY (migration_id, sequence)
);

CREATE TABLE capsule_migration_check (
    migration_id            TEXT NOT NULL REFERENCES capsule_migration(id) ON DELETE CASCADE,
    sequence                INTEGER NOT NULL CHECK (sequence > 0 AND sequence <= 256),
    stage                   TEXT NOT NULL CHECK (stage IN ('pre', 'post')),
    severity                TEXT NOT NULL CHECK (severity IN ('error', 'warning', 'info')),
    description             TEXT NOT NULL,
    definition_json         TEXT NOT NULL CHECK (json_valid(definition_json)),
    PRIMARY KEY (migration_id, sequence)
);

CREATE TABLE capsule_lineage_event (
    event_id                TEXT PRIMARY KEY NOT NULL,
    sequence                INTEGER NOT NULL UNIQUE CHECK (sequence > 0),
    operation               TEXT NOT NULL CHECK (
                                operation IN (
                                    'created',
                                    'created-from-template',
                                    'fork',
                                    'reconcile',
                                    'application-upgrade',
                                    'import'
                                )
                            ),
    result_capsule_id       TEXT NOT NULL,
    result_revision_id      TEXT NOT NULL,
    occurred_at             TEXT NOT NULL,
    application_digest      TEXT NOT NULL CHECK (
                                length(application_digest) = 64
                                AND application_digest NOT GLOB '*[^0-9a-f]*'
                            ),
    data_schema_id          TEXT NOT NULL,
    data_schema_version     INTEGER NOT NULL CHECK (data_schema_version >= 1),
    plan_digest             TEXT NOT NULL CHECK (
                                length(plan_digest) = 64
                                AND plan_digest NOT GLOB '*[^0-9a-f]*'
                            ),
    details_json            TEXT NOT NULL CHECK (json_valid(details_json))
);

CREATE TABLE capsule_lineage_parent (
    event_id                TEXT NOT NULL REFERENCES capsule_lineage_event(event_id) ON DELETE CASCADE,
    ordinal                 INTEGER NOT NULL CHECK (ordinal > 0 AND ordinal <= 8),
    relation                TEXT NOT NULL CHECK (
                                relation IN (
                                    'created-from',
                                    'forked-from',
                                    'target-derived-from',
                                    'changes-applied-from',
                                    'upgraded-from',
                                    'application-release'
                                )
                            ),
    parent_capsule_id       TEXT,
    parent_revision_id      TEXT,
    parent_file_sha256      TEXT NOT NULL CHECK (
                                length(parent_file_sha256) = 64
                                AND parent_file_sha256 NOT GLOB '*[^0-9a-f]*'
                            ),
    PRIMARY KEY (event_id, ordinal)
);

CREATE TABLE capsule_change_log (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint_name           TEXT NOT NULL,
    parameters_json         TEXT NOT NULL CHECK (json_valid(parameters_json)),
    changed_rows            INTEGER NOT NULL,
    occurred_at             TEXT NOT NULL
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
CREATE INDEX idx_capsule_dataset_table_sequence
    ON capsule_dataset_table(dataset_id, sequence, table_name);
CREATE INDEX idx_capsule_lineage_sequence
    ON capsule_lineage_event(sequence, event_id);
CREATE INDEX idx_capsule_migration_edge
    ON capsule_migration(data_schema_id, from_version, to_version);
