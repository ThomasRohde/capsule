-- Deterministic signed-app/0.3 test data for Capsule format v0.3.
-- Applied after format/capsule-v0.3.sql and the signed-app extension.

CREATE TABLE vector_domain (
    id TEXT PRIMARY KEY NOT NULL,
    note TEXT NOT NULL,
    measurement REAL NOT NULL,
    payload BLOB NOT NULL
);

CREATE TABLE vector_settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

INSERT INTO capsule_manifest (
    id, format_id, format_version, app_id, app_version, entry_asset,
    runtime_protocol, permissions_json, data_schema_id, data_schema_version,
    minimum_host_profile, released_at
) VALUES (
    1, 'org.sqlite-capsule', '0.3', 'org.sqlite-capsule.vector', '3.0.0',
    'app/index.html', 'capsule-http/0.2',
    '{"z":0,"é":"café","":"bmp","😀":"astral","database.read":{"required":true},"database.write":{"required":true},"network":{"value":"none"}}',
    'org.sqlite-capsule.vector-data', 2,
    'org.sqlite-capsule.host-profile/0.3', '2026-08-08T00:00:00Z'
);

INSERT INTO capsule_asset VALUES
    ('app/index.html', 'text/html', X'3c68746d6c3e3c2f68746d6c3e',
     'b633a587c652d02386c4f16f8c6f6aab7352d97f16367c3c40576214372dd628',
     1, 'no-store', 'Minimal vector entry point'),
    ('app/icon.png', 'image/png',
     X'89504e470d0a1a0a0000000d4948445200000001000000010804000000b51c0c020000000b4944415478da6364f80f00010501012718e3660000000049454e44ae426082',
     '431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460',
     0, 'no-store', 'Signed application icon'),
    ('app/large.bin', 'application/octet-stream', zeroblob(1048576),
     '30e14955ebf1352266dc2ff8067e68104607e750abb9d3b36582b8af909fcb58',
     0, 'no-store', 'Large but bounded signed asset');

INSERT INTO capsule_doc VALUES (
    'release-notes', 'Release notes', 'text/markdown', 'Initial v0.3 vector.', 0
);

INSERT INTO capsule_application VALUES (
    1, 'Café Vector', 'Generic v0.3 compatibility fixture.', 'developer-tool',
    'app/icon.png', 'release-notes'
);

INSERT INTO capsule_instance_asset VALUES (
    'instance-icon', 'image/png',
    X'89504e470d0a1a0a0000000d4948445200000001000000010804000000b51c0c020000000b4944415478da6364f80f00010501012718e3660000000049454e44ae426082',
    '431ced6916a2a21a156e38701afe55bbd7f88969fbbfc56d7fe099d47f265460',
    1, 1, 'Mutable instance icon'
);

INSERT INTO capsule_instance VALUES (
    1,
    '11111111-1111-4111-8111-111111111111',
    '22222222-2222-4222-8222-222222222222',
    'Vector document', 'Mutable instance profile.', 'test-document',
    '["vector","café"]', 'instance-icon', NULL,
    '2026-08-08T00:00:00Z', '2026-08-08T00:00:00Z'
);

INSERT INTO capsule_command VALUES (
    'vector.command', 'Empty and Unicode argv', 'any', '{repo}',
    'vector', '["vector","å"]', 'read-only', 'done'
);

INSERT INTO capsule_runbook VALUES (
    'vector.runbook', 'agent', 1, 'Begin', 'Read Café.', 'vector.command'
);

INSERT INTO capsule_endpoint VALUES (
    'vector.write', 'write',
    'UPDATE vector_domain SET note = :value WHERE id = ''domain''',
    '{"value":{"type":"string","required":true}}', 'changes', 'Write one value.', 1
);

INSERT INTO capsule_endpoint_step VALUES
    ('vector.write', 1, 'UPDATE vector_domain SET note = :value WHERE id = ''domain''', 1),
    ('vector.write', 2, 'UPDATE vector_settings SET value = value WHERE key = ''theme''', 1);

INSERT INTO capsule_check VALUES (
    'vector.check', 'error', 'Integer and Boolean JSON.', 'SELECT 1',
    'scalar', '1'
);

INSERT INTO capsule_prompt VALUES (
    'vector.prompt', 'Question?', 'Return “yes”.', 0
);

INSERT INTO capsule_dataset VALUES
    ('content', 'user-content', 'Mutable vector content.', 'copy', 'field',
     'three-way', 'migrate', 'normal', 1),
    ('settings', 'settings', 'Mutable vector settings.', 'copy', 'row',
     'manual', 'copy', 'normal', 1);

INSERT INTO capsule_dataset_table VALUES
    ('content', 'vector_domain', 0, '["id"]', '[]', '["id"]'),
    ('settings', 'vector_settings', 0, '["key"]', '[]', '["key"]');

INSERT INTO capsule_dataset_dependency VALUES (
    'content', 'settings', 'Content interpretation uses settings.'
);

INSERT INTO capsule_migration VALUES (
    'vector-1-to-2', 'org.sqlite-capsule.vector-data', 1, 2,
    'Copy content into the v2 release.', 'org.sqlite-capsule.migration-ops/1', 0
);

INSERT INTO capsule_migration_step VALUES (
    'vector-1-to-2', 1, 'copy_dataset',
    '{"operation":"copy_dataset","dataset_id":"content","mode":"replace-empty-target"}'
);

INSERT INTO capsule_migration_check VALUES
    ('vector-1-to-2', 1, 'pre', 'error', 'Content dataset is declared.',
     '{"kind":"dataset_declared","severity":"error","description":"Content dataset is declared.","dataset_id":"content"}'),
    ('vector-1-to-2', 2, 'post', 'error', 'Schema version is two.',
     '{"kind":"schema_version_equals","severity":"error","description":"Schema version is two.","version":2}');

INSERT INTO capsule_publisher VALUES (
    1, 'org.sqlite-capsule.signed-app/0.3', 'org.example.vector', 'Vector Publisher'
);

INSERT INTO capsule_grant VALUES (
    'database.read', 'allow', 'Excluded mutable grant', NULL
);

INSERT INTO capsule_change_log (
    endpoint_name, parameters_json, changed_rows, occurred_at
) VALUES ('vector.write', '{}', 0, '2026-08-08T00:00:00Z');

INSERT INTO capsule_lineage_event VALUES (
    '33333333-3333-4333-8333-333333333333', 1, 'created',
    '11111111-1111-4111-8111-111111111111',
    '22222222-2222-4222-8222-222222222222',
    '2026-08-08T00:00:00Z',
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    'org.sqlite-capsule.vector-data', 2,
    'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
    '{}'
);

INSERT INTO vector_domain VALUES ('domain', 'mutable', -0.0, X'102030');

-- Deterministic Ed25519 envelope over the v0.3 vector digest. The seed is the
-- public development fixture under compatibility/signed-app-v0.2 and confers
-- no trust. The signature row is excluded from the canonical compartment.
INSERT INTO capsule_signature VALUES (
    'ed25519:sha256:b600306cfa76723fdec395e53a9b3d9fdb78b1e2d7a23c32fcbcd2dc6d0c4092',
    'ed25519',
    X'197f6b23e16c8532c6abc838facd5ea789be0c76b2920334039bfa8b3d368d61',
    X'fba075f1ced1ab72ca26f5f62ca53c0383bf170990989939576b3bb2446f03c6',
    X'571963c6314a915bfa9d9b0afd92e9140d2a2220c39cf46e309555f1aad154f93c94ebc29dd7c29129765d058fb19336977b52f4049ca509fdce00546ddae90a',
    '2026-08-08T12:34:56Z'
);
INSERT INTO vector_settings VALUES ('theme', 'light');
