PRAGMA foreign_keys = ON;

CREATE TABLE diagram_document (
    id              TEXT PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    canvas_width    REAL NOT NULL CHECK (canvas_width > 0),
    canvas_height   REAL NOT NULL CHECK (canvas_height > 0),
    background      TEXT NOT NULL DEFAULT 'grid',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE diagram_layer (
    id              TEXT PRIMARY KEY,
    diagram_id      TEXT NOT NULL REFERENCES diagram_document(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    position        INTEGER NOT NULL CHECK (position > 0),
    visible         INTEGER NOT NULL DEFAULT 1 CHECK (visible IN (0, 1)),
    locked          INTEGER NOT NULL DEFAULT 0 CHECK (locked IN (0, 1)),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE (diagram_id, position)
);

CREATE TABLE diagram_node (
    id              TEXT PRIMARY KEY,
    diagram_id      TEXT NOT NULL REFERENCES diagram_document(id) ON DELETE CASCADE,
    layer_id        TEXT NOT NULL DEFAULT 'layer-content' REFERENCES diagram_layer(id) ON DELETE RESTRICT,
    kind            TEXT NOT NULL,
    label           TEXT NOT NULL,
    x               REAL NOT NULL,
    y               REAL NOT NULL,
    width           REAL NOT NULL CHECK (width >= 60),
    height          REAL NOT NULL CHECK (height >= 40),
    z_index         INTEGER NOT NULL DEFAULT 0,
    style_json      TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(style_json)),
    data_json       TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(data_json)),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE diagram_edge (
    id              TEXT PRIMARY KEY,
    diagram_id      TEXT NOT NULL REFERENCES diagram_document(id) ON DELETE CASCADE,
    layer_id        TEXT NOT NULL DEFAULT 'layer-connectors' REFERENCES diagram_layer(id) ON DELETE RESTRICT,
    source_id       TEXT NOT NULL REFERENCES diagram_node(id) ON DELETE CASCADE,
    target_id       TEXT NOT NULL REFERENCES diagram_node(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL DEFAULT 'flow',
    label           TEXT NOT NULL DEFAULT '',
    source_port     TEXT NOT NULL DEFAULT 'auto' CHECK (source_port IN ('auto', 'north', 'east', 'south', 'west')),
    target_port     TEXT NOT NULL DEFAULT 'auto' CHECK (target_port IN ('auto', 'north', 'east', 'south', 'west')),
    route_mode      TEXT NOT NULL DEFAULT 'orthogonal' CHECK (route_mode IN ('orthogonal', 'direct')),
    waypoints_json  TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(waypoints_json)),
    style_json      TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(style_json)),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    CHECK (source_id <> target_id)
);

CREATE TABLE diagram_group (
    id              TEXT PRIMARY KEY,
    diagram_id      TEXT NOT NULL REFERENCES diagram_document(id) ON DELETE CASCADE,
    layer_id        TEXT NOT NULL REFERENCES diagram_layer(id) ON DELETE RESTRICT,
    name            TEXT NOT NULL,
    z_index         INTEGER NOT NULL DEFAULT 0,
    locked          INTEGER NOT NULL DEFAULT 0 CHECK (locked IN (0, 1)),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE diagram_group_member (
    group_id        TEXT NOT NULL REFERENCES diagram_group(id) ON DELETE CASCADE,
    node_id         TEXT NOT NULL UNIQUE REFERENCES diagram_node(id) ON DELETE CASCADE,
    position        INTEGER NOT NULL CHECK (position > 0),
    PRIMARY KEY (group_id, node_id),
    UNIQUE (group_id, position)
);

CREATE TABLE diagram_scene (
    id              TEXT PRIMARY KEY,
    diagram_id      TEXT NOT NULL REFERENCES diagram_document(id) ON DELETE CASCADE,
    position        INTEGER NOT NULL,
    title           TEXT NOT NULL,
    narrative       TEXT NOT NULL DEFAULT '',
    viewport_json   TEXT NOT NULL CHECK (json_valid(viewport_json)),
    focus_json      TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(focus_json)),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE (diagram_id, position)
);

CREATE TABLE diagram_scene_override (
    scene_id         TEXT NOT NULL REFERENCES diagram_scene(id) ON DELETE CASCADE,
    node_id          TEXT NOT NULL REFERENCES diagram_node(id) ON DELETE CASCADE,
    x                REAL,
    y                REAL,
    width            REAL CHECK (width IS NULL OR width >= 60),
    height           REAL CHECK (height IS NULL OR height >= 40),
    visible          INTEGER CHECK (visible IS NULL OR visible IN (0, 1)),
    style_json       TEXT CHECK (style_json IS NULL OR json_valid(style_json)),
    PRIMARY KEY (scene_id, node_id)
);

CREATE TABLE diagram_history (
    diagram_id      TEXT PRIMARY KEY REFERENCES diagram_document(id) ON DELETE CASCADE,
    cursor          INTEGER NOT NULL DEFAULT 0 CHECK (cursor >= 0),
    tip             INTEGER NOT NULL DEFAULT 0 CHECK (tip >= cursor),
    updated_at      TEXT NOT NULL
);

CREATE TABLE diagram_operation (
    id              TEXT PRIMARY KEY,
    diagram_id      TEXT NOT NULL REFERENCES diagram_document(id) ON DELETE CASCADE,
    sequence        INTEGER NOT NULL CHECK (sequence > 0),
    command_kind    TEXT NOT NULL,
    summary         TEXT NOT NULL,
    undo_endpoint   TEXT NOT NULL,
    redo_endpoint   TEXT NOT NULL,
    forward_json    TEXT NOT NULL CHECK (json_valid(forward_json)),
    inverse_json    TEXT NOT NULL CHECK (json_valid(inverse_json)),
    state           TEXT NOT NULL CHECK (state IN ('applied', 'undone')),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    UNIQUE (diagram_id, sequence)
);

CREATE INDEX idx_diagram_node_diagram_z
    ON diagram_node(diagram_id, layer_id, z_index, id);

CREATE INDEX idx_diagram_layer_order
    ON diagram_layer(diagram_id, position);

CREATE INDEX idx_diagram_edge_diagram
    ON diagram_edge(diagram_id, id);

CREATE INDEX idx_diagram_edge_source
    ON diagram_edge(source_id);

CREATE INDEX idx_diagram_edge_target
    ON diagram_edge(target_id);

CREATE INDEX idx_diagram_scene_order
    ON diagram_scene(diagram_id, position);

CREATE INDEX idx_diagram_group_order
    ON diagram_group(diagram_id, layer_id, z_index, id);

CREATE INDEX idx_diagram_operation_sequence
    ON diagram_operation(diagram_id, sequence);

CREATE INDEX idx_diagram_operation_state
    ON diagram_operation(diagram_id, state, sequence);
