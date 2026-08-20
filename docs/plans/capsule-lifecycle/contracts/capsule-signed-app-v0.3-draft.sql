-- Draft signed application extension for Capsule v0.3.
-- The canonical stream/signature contexts must be distinct from signed-app/0.2.

CREATE TABLE capsule_publisher (
    id                      INTEGER PRIMARY KEY CHECK (id = 1),
    profile                 TEXT NOT NULL CHECK (
                                profile = 'org.sqlite-capsule.signed-app/0.3'
                            ),
    publisher_id            TEXT NOT NULL CHECK (length(publisher_id) BETWEEN 1 AND 512),
    publisher_name          TEXT NOT NULL CHECK (length(publisher_name) BETWEEN 1 AND 512)
);

CREATE TABLE capsule_signature (
    key_id                  TEXT PRIMARY KEY NOT NULL
                            CHECK (length(key_id) = 79)
                            CHECK (substr(key_id, 1, 15) = 'ed25519:sha256:')
                            CHECK (substr(key_id, 16) NOT GLOB '*[^0-9a-f]*'),
    algorithm               TEXT NOT NULL CHECK (algorithm = 'ed25519'),
    public_key              BLOB NOT NULL CHECK (length(public_key) = 32),
    application_digest      BLOB NOT NULL CHECK (length(application_digest) = 32),
    signature               BLOB NOT NULL CHECK (length(signature) = 64),
    signed_at               TEXT NOT NULL CHECK (
                                length(signed_at) = 20
                                AND strftime('%Y-%m-%dT%H:%M:%SZ', signed_at) = signed_at
                            )
);
