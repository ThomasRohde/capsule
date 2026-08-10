# Diagram Studio data model

The example keeps diagram semantics in ordinary relational tables.

## `diagram_document`

One canvas with stable identity, title, description, dimensions, background choice, and timestamps.

## `diagram_layer`

Layers have stable IDs, explicit one-based ordering, visibility, lock state, and a diagram foreign key. Nodes, connectors, and groups reference semantic layer records rather than relying on panel-only UI state.

## `diagram_node`

Nodes have stable text IDs, diagram and layer foreign keys, semantic `kind`, label, position, size, z-order, and two JSON documents:

- `style_json` contains visual properties understood by this renderer.
- `data_json` contains semantic annotations such as eyebrow text, description, and whether a node is locked.

Coordinates and dimensions remain scalar columns because they are frequently queried and updated.

## `diagram_edge`

Edges connect source and target node IDs through foreign keys. Deleting a node cascades to its connectors. The semantic relationship is stored separately from the derived SVG route. Explicit/automatic ports, direct/orthogonal routing intent, bounded waypoints, labels, and styles are durable author intent.

## `diagram_group` and `diagram_group_member`

Groups have stable IDs, a semantic layer, lock state, and ordered node membership. Membership is non-nested; a node belongs to at most one group and application checks require the same diagram and layer.

## `diagram_scene`

A scene is a guided view over the live diagram, not a copied slide. `viewport_json` stores the target camera rectangle and `focus_json` stores stable node IDs to emphasise. `diagram_scene_override` optionally changes a stable node's position, size, style, or visibility in that scene. Matching stable IDs morph during presentation; reduced-motion mode applies the same state immediately.

## `diagram_history` and `diagram_operation`

The history row stores a durable cursor and tip. Each operation stores a monotonic sequence, semantic command kind, bounded forward and inverse payloads, named Undo/Redo endpoints, and applied/undone state. A model change, operation row, and cursor update share one v0.2 compound transaction.

## Data-access boundary

The browser does not receive raw SQL. It calls named records from `capsule_endpoint`, including `diagram.nodes`, `nodes.transform`, `group.toggle`, `scenes.apply`, and `diagram.import`. The host validates the exact typed parameter contract, authorises every v0.2 endpoint step, applies one transaction and timeout budget, rolls back any failed precondition, and writes one operation-level change-log entry.

This separation should survive future renderers and examples. New domains may use completely different tables while retaining the generic capsule contract.
