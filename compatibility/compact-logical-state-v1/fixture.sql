PRAGMA application_id = 1129337676;
PRAGMA user_version = 3;
PRAGMA auto_vacuum = NONE;
PRAGMA default_cache_size = -2000;

CREATE TABLE bag (
    note TEXT,
    payload BLOB,
    measurement REAL
);
CREATE TABLE keyed (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    note TEXT NOT NULL
);
CREATE TABLE capsule_signature (
    key_id TEXT PRIMARY KEY NOT NULL,
    signature BLOB NOT NULL
);
CREATE INDEX bag_note ON bag(note);
CREATE VIEW bag_view AS SELECT rowid, note FROM bag;

INSERT INTO bag(rowid, note, payload, measurement) VALUES
    (1, 'alpha', X'00ff', -0.0),
    (3, 'é', X'ff00', 1.5);
INSERT INTO keyed(note) VALUES ('sequence-bound');
UPDATE sqlite_sequence SET seq = 9 WHERE name = 'keyed';
INSERT INTO capsule_signature VALUES ('key', X'0102');
