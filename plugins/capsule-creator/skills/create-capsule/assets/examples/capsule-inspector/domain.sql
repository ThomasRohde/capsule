CREATE TABLE inspector_reference (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    supported_format    TEXT NOT NULL,
    engine_package      TEXT NOT NULL,
    engine_version      TEXT NOT NULL,
    max_file_bytes      INTEGER NOT NULL CHECK (max_file_bytes = 67108864),
    execution_policy    TEXT NOT NULL CHECK (execution_policy = 'catalogue-only'),
    target_write_policy TEXT NOT NULL CHECK (target_write_policy = 'never'),
    network_policy      TEXT NOT NULL CHECK (network_policy = 'none')
);
