CREATE TABLE trust_meta_v2 (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version  INTEGER NOT NULL CHECK (schema_version = 2),
    created_at      TEXT NOT NULL
) STRICT;

INSERT INTO trust_meta_v2
    SELECT id, 2, created_at FROM trust_meta WHERE id = 1;

DROP TABLE trust_meta;
ALTER TABLE trust_meta_v2 RENAME TO trust_meta;

CREATE TABLE remote_key_revocation (
    key_id          TEXT PRIMARY KEY NOT NULL CHECK (length(key_id) BETWEEN 1 AND 256),
    bundle_sequence INTEGER NOT NULL REFERENCES revocation_bundle(sequence) ON DELETE CASCADE,
    reason          TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 2048)
) STRICT;

CREATE TABLE remote_release_revocation (
    application_id      TEXT NOT NULL CHECK (length(application_id) BETWEEN 1 AND 512),
    application_digest  BLOB NOT NULL CHECK (length(application_digest) = 32),
    bundle_sequence     INTEGER NOT NULL REFERENCES revocation_bundle(sequence) ON DELETE CASCADE,
    reason              TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 2048),
    PRIMARY KEY (application_id, application_digest)
) STRICT;

CREATE TABLE revocation_root (
    key_id          TEXT PRIMARY KEY NOT NULL CHECK (length(key_id) BETWEEN 1 AND 256),
    public_key      BLOB NOT NULL CHECK (length(public_key) = 32),
    decision        TEXT NOT NULL CHECK (decision IN ('delegated', 'revoked')),
    bundle_sequence INTEGER NOT NULL REFERENCES revocation_bundle(sequence) ON DELETE CASCADE,
    reason          TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 2048)
) STRICT;
