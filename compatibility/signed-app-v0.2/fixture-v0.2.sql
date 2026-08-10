
-- Deterministic signed-app/0.2 test data for capsule format v0.2.
-- This file is applied after format/capsule-v0.2.sql and the signed extension.

INSERT INTO capsule_manifest (
    id, format_id, format_version, capsule_id, title, summary, app_id,
    app_version, entry_asset, runtime_protocol, permissions_json,
    created_at, updated_at
) VALUES (
    1, 'org.sqlite-capsule', '0.2', 'urn:vector:signed-app:v0.2',
    'Café ☕ 😀', '', 'org.sqlite-capsule.vector', '2.0.0',
    'app/é.bin', 'capsule-http/0.2',
    '{"z":0,"é":"café","":"bmp","😀":"astral","database.read":{"required":true}}',
    '2026-08-08T00:00:00Z', '2026-08-08T00:00:00Z'
);

INSERT INTO capsule_asset VALUES
    ('app/é.bin', 'application/octet-stream', X'00ff7f80',
     '1111111111111111111111111111111111111111111111111111111111111111',
     1, 'no-store', 'decomposed é'),
    ('app/large.bin', 'application/octet-stream', zeroblob(1048576),
     '2222222222222222222222222222222222222222222222222222222222222222',
     0, 'immutable', 'large-but-bounded'),
    ('app/empty.bin', 'application/octet-stream', X'',
     '3333333333333333333333333333333333333333333333333333333333333333',
     0, 'no-store', NULL);

INSERT INTO capsule_command VALUES (
    'vector.command', 'Empty and Unicode argv', 'any', '{repo}',
    'vector', '["",1,true,"å"]', 'read-only', 'done'
);
INSERT INTO capsule_runbook VALUES (
    'vector.runbook', 'agent', -1, 'Begin', 'Read Café.', 'vector.command'
);
INSERT INTO capsule_doc VALUES (
    'vector', 'Vector', 'text/markdown', '', -7
);
INSERT INTO capsule_endpoint VALUES (
    'vector.write', 'write', '',
    '{"value":{"type":"text","required":true}}', 'changes', 'Write one value.', 1
);
INSERT INTO capsule_endpoint_step VALUES
    ('vector.write', 2, 'UPDATE vector_domain SET note = :value WHERE id = ''domain''', 1),
    ('vector.write', 1, 'SELECT 1', NULL);
INSERT INTO capsule_check VALUES (
    'vector.check', 'error', 'Integer and Boolean JSON.', 'SELECT 1',
    'scalar', '{"ok":true,"value":1}'
);
INSERT INTO capsule_prompt VALUES (
    'vector.prompt', 'Question?', 'Return “yes”.', 0
);
INSERT INTO capsule_publisher VALUES (
    1, 'org.sqlite-capsule.signed-app/0.2', 'org.example.vector', 'Vector Publisher'
);

INSERT INTO capsule_grant VALUES (
    'database.read', 'allow', 'excluded mutable grant', NULL
);
INSERT INTO capsule_change_log (
    endpoint_name, parameters_json, changed_rows, occurred_at
) VALUES ('vector.write', '{}', 0, '2026-08-08T00:00:00Z');

CREATE TABLE vector_domain (
    id TEXT PRIMARY KEY,
    note TEXT NOT NULL,
    measurement REAL NOT NULL,
    payload BLOB NOT NULL
);
INSERT INTO vector_domain VALUES ('domain', 'mutable', -0.0, X'102030');
