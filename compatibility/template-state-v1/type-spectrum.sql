-- Extra dataset-state/1 vector coverage for SQLite storage classes and generated columns.
CREATE TABLE vector_type_spectrum (
    id INTEGER PRIMARY KEY,
    nullable_text TEXT,
    whole INTEGER NOT NULL,
    generated_text TEXT GENERATED ALWAYS AS (coalesce(nullable_text, 'null')) STORED,
    virtual_text TEXT GENERATED ALWAYS AS (coalesce(nullable_text, 'null') || '!') VIRTUAL
);

INSERT INTO vector_type_spectrum (id, nullable_text, whole) VALUES (1, NULL, -42);
INSERT INTO vector_type_spectrum (id, nullable_text, whole) VALUES (2, 'é', 17);

INSERT INTO capsule_dataset VALUES
    ('type-spectrum', 'seed', 'Template-state storage-class coverage.',
     'copy', 'field', 'three-way', 'copy', 'normal', 0);

INSERT INTO capsule_dataset_table VALUES
    ('type-spectrum', 'vector_type_spectrum', 0, '["id"]', '[]', '["id"]');

-- Empty tables still contribute their complete header to the digest.
CREATE TABLE vector_empty_state (
    id TEXT PRIMARY KEY NOT NULL,
    note TEXT
);

INSERT INTO capsule_dataset VALUES
    ('empty-state', 'seed', 'Template-state empty-table header coverage.',
     'copy', 'field', 'three-way', 'copy', 'normal', 0);

INSERT INTO capsule_dataset_table VALUES
    ('empty-state', 'vector_empty_state', 0, '["id"]', '[]', '["id"]');

-- Multi-table and composite-key ordering uses signed sequence first, then
-- BINARY key order without Unicode normalization.
CREATE TABLE vector_composite_a (
    part_a TEXT NOT NULL,
    part_b INTEGER NOT NULL,
    raw_text TEXT,
    payload BLOB,
    virtual_text TEXT GENERATED ALWAYS AS (coalesce(raw_text, '') || '!') VIRTUAL,
    PRIMARY KEY (part_a, part_b)
) WITHOUT ROWID;

CREATE TABLE vector_composite_b (
    id INTEGER PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO vector_composite_a (part_a, part_b, raw_text, payload) VALUES
    ('é', 2, 'precomposed', X'00FF'),
    ('é', 1, 'combining', X'FF00'),
    ('a', 9, '', X'');
INSERT INTO vector_composite_b VALUES (1, 'second-table');

INSERT INTO capsule_dataset VALUES
    ('ordering', 'seed', 'Template-state table and composite-key ordering coverage.',
     'copy', 'field', 'three-way', 'copy', 'normal', 0);

INSERT INTO capsule_dataset_table VALUES
    ('ordering', 'vector_composite_b', 1, '["id"]', '[]', '["id"]'),
    ('ordering', 'vector_composite_a', 0, '["part_a","part_b"]', '[]',
     '["part_a","part_b"]');
