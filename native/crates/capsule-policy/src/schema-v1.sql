CREATE TABLE trust_meta (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version  INTEGER NOT NULL CHECK (schema_version = 1),
    created_at      TEXT NOT NULL
) STRICT;

INSERT INTO trust_meta VALUES (1, 1, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'));

CREATE TABLE publisher (
    publisher_id    TEXT PRIMARY KEY NOT NULL CHECK (length(publisher_id) BETWEEN 1 AND 512),
    publisher_name  TEXT NOT NULL CHECK (length(publisher_name) BETWEEN 1 AND 512),
    status          TEXT NOT NULL CHECK (status IN ('active', 'revoked')),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
) STRICT;

CREATE TABLE publisher_key (
    key_id          TEXT PRIMARY KEY NOT NULL CHECK (length(key_id) BETWEEN 1 AND 256),
    publisher_id    TEXT NOT NULL REFERENCES publisher(publisher_id) ON DELETE CASCADE,
    public_key      BLOB NOT NULL CHECK (length(public_key) = 32),
    decision        TEXT NOT NULL CHECK (decision IN ('trusted', 'revoked')),
    reason          TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 2048),
    decided_at      TEXT NOT NULL,
    UNIQUE (publisher_id, public_key)
) STRICT;

CREATE TABLE key_delegation (
    from_key_id         TEXT NOT NULL REFERENCES publisher_key(key_id) ON DELETE CASCADE,
    to_key_id           TEXT NOT NULL REFERENCES publisher_key(key_id) ON DELETE CASCADE,
    publisher_id        TEXT NOT NULL REFERENCES publisher(publisher_id) ON DELETE CASCADE,
    application_id      TEXT NOT NULL DEFAULT '',
    evidence_digest     BLOB NOT NULL CHECK (length(evidence_digest) = 32),
    approved_at         TEXT NOT NULL,
    PRIMARY KEY (from_key_id, to_key_id, application_id),
    CHECK (from_key_id <> to_key_id)
) STRICT;

CREATE TABLE exact_release (
    capsule_id          TEXT NOT NULL,
    application_id      TEXT NOT NULL,
    application_digest  BLOB NOT NULL CHECK (length(application_digest) = 32),
    key_id              TEXT NOT NULL,
    publisher_id        TEXT NOT NULL,
    decision            TEXT NOT NULL CHECK (decision IN ('trusted', 'denied', 'revoked')),
    reason              TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 2048),
    decided_at          TEXT NOT NULL,
    PRIMARY KEY (capsule_id, application_id, application_digest, key_id)
) STRICT;

CREATE TABLE local_exception (
    capsule_id          TEXT NOT NULL,
    application_id      TEXT NOT NULL,
    source_sha256       BLOB NOT NULL CHECK (length(source_sha256) = 32),
    decision            TEXT NOT NULL CHECK (decision IN ('trusted', 'denied')),
    reason              TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 2048),
    decided_at          TEXT NOT NULL,
    PRIMARY KEY (capsule_id, application_id, source_sha256)
) STRICT;

CREATE TABLE capsule_identity (
    capsule_id          TEXT NOT NULL,
    application_id      TEXT NOT NULL,
    last_source_sha256  BLOB NOT NULL CHECK (length(last_source_sha256) = 32),
    first_seen_at       TEXT NOT NULL,
    last_seen_at        TEXT NOT NULL,
    PRIMARY KEY (capsule_id, application_id)
) STRICT;

CREATE TABLE capability_grant (
    capsule_id          TEXT NOT NULL,
    application_id      TEXT NOT NULL,
    application_digest  BLOB NOT NULL CHECK (length(application_digest) = 32),
    capability          TEXT NOT NULL CHECK (length(capability) BETWEEN 1 AND 128),
    decision            TEXT NOT NULL CHECK (decision IN ('allow', 'deny')),
    reason              TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 2048),
    decided_at          TEXT NOT NULL,
    PRIMARY KEY (capsule_id, application_id, application_digest, capability)
) STRICT;

CREATE TABLE revocation_bundle (
    sequence            INTEGER PRIMARY KEY CHECK (sequence > 0),
    issued_at           TEXT NOT NULL,
    next_update         TEXT NOT NULL,
    payload_digest      BLOB NOT NULL CHECK (length(payload_digest) = 32),
    installed_at        TEXT NOT NULL,
    active              INTEGER NOT NULL CHECK (active IN (0, 1))
) STRICT;

CREATE UNIQUE INDEX one_active_revocation_bundle
    ON revocation_bundle(active) WHERE active = 1;

CREATE TABLE backup_inventory (
    backup_id           TEXT PRIMARY KEY NOT NULL,
    source_capsule_id   TEXT NOT NULL,
    canonical_path      TEXT NOT NULL,
    database_digest     BLOB NOT NULL CHECK (length(database_digest) = 32),
    byte_length         INTEGER NOT NULL CHECK (byte_length >= 0),
    created_at          TEXT NOT NULL,
    verified_at         TEXT NOT NULL
) STRICT;

CREATE TABLE audit_event (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_at     TEXT NOT NULL,
    severity        TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'security')),
    action          TEXT NOT NULL CHECK (length(action) BETWEEN 1 AND 128),
    capsule_id      TEXT,
    publisher_id    TEXT,
    key_id          TEXT,
    details_json    TEXT NOT NULL CHECK (json_valid(details_json))
) STRICT;

CREATE INDEX audit_event_time ON audit_event(occurred_at, id);
CREATE INDEX audit_event_capsule ON audit_event(capsule_id, occurred_at);
