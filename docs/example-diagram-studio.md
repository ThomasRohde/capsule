# First example: Diagram Studio

## Purpose

Diagram Studio is the first capsule application because it demonstrates the
format visibly and interactively. It is not the product definition and no
diagram-specific table, route, or history rule exists in the generic host.

The v0.2 example proves that one SQLite file can contain an offline drawing
application, structured semantic data, durable operations, authored presentation
scenes, a bounded interchange format, its launch instructions, and its compatible
fallback host.

## Experience target

Opening the capsule should feel like opening a small drawing application, not a
database browser. The example provides:

- an SVG canvas with pan, zoom, fit, presentation mode, and a structured
  inspector;
- rectangle, rounded rectangle, ellipse, diamond, pill, note, and container
  shapes with pointer and keyboard resizing;
- ordered multi-selection, Shift-click toggling, marquee selection, alignment,
  distribution, grouping, and semantic layers;
- explicit connector endpoints and ports, direct routing, and deterministic
  obstacle-aware orthogonal routing;
- a durable operation cursor with Undo/Redo that survives reload and host
  restart;
- scene create, rename, capture, duplicate, reorder, delete, stable-ID overrides,
  morph transitions, and reduced-motion behavior;
- preview/apply/cancel layout experiments for grid, directional, and layered
  arrangements;
- bounded JSON clipboard/import plus deterministic offline JSON, SVG, and PNG
  downloads; and
- focusable canvas objects, named controls, live status announcements, standard
  shortcuts, forced-color styles, and a narrow-screen layout.

No external JavaScript, CSS, font, image, analytics, or API dependency is loaded
at runtime.

## Deliberate self-reference

The seed diagram visualises the system that is rendering it:

```text
one-sentence agent prompt
        -> embedded START_HERE runbook
        -> trusted generic host
        -> SQLite capsule containing app, data, APIs, checks and docs
        -> interactive browser experience
        -> future browser-only SQLite distribution
```

This makes the first run an explanation of the full idea as well as a functional
demonstration.

## Domain model

`diagram_document` stores one logical canvas. `diagram_layer` gives stable IDs,
one-based ordering, visibility, and lock state to semantic layers.

`diagram_node` stores stable identity, semantic kind, label, scalar frame,
z-order, layer, style JSON, and application data JSON. Scalar geometry remains
easy to inspect and update with ordinary SQLite.

`diagram_edge` stores the semantic source and target independently from derived
SVG path geometry. It also stores its layer, explicit or automatic endpoint
ports, routing intent, bounded manual waypoints, label, and style.

`diagram_group` and `diagram_group_member` define non-nested ordered groups. A
node can belong to at most one group, and group members remain in the same diagram
and semantic layer.

`diagram_scene` stores an ordered title, narrative, viewport, and stable focus
IDs. `diagram_scene_override` optionally changes a stable node's frame, style, or
visibility for one scene without copying the base diagram.

`diagram_history` stores the durable cursor and tip. `diagram_operation` stores a
stable operation ID, sequence, command kind, summary, named Undo/Redo endpoints,
bounded semantic forward/inverse payloads, state, and timestamps. This is
application-owned history built on the generic v0.2 atomic-command mechanism.

## Interaction and command model

Continuous pointer changes are previews in browser memory. Pointer release sends
one semantic named command, so a drag or resize becomes one durable operation
rather than one database write per pixel. Multi-object transforms, grouping,
scene replacement, layout application, deletion, and import are also single
atomic commands.

Drag and resize input is coalesced onto animation frames. During a gesture the
browser updates only the active node attributes and lightweight paths for its
connected edges; it does not reconstruct every SVG object, rerun full obstacle
routing, or rebuild the inspector for every pointer event. Pointer release does
one authoritative render with wrapped text and obstacle-aware routes before the
single durable command is recorded.

Undo and Redo call the operation's stored named inverse or forward endpoint. The
expected cursor is an optimistic-concurrency guard. A new command after Undo
invalidates the redo branch. No trigger, browser-supplied SQL, or Diagram Studio
logic in the host is involved.

Selection is an ordered browser set. Shift-click and Shift-drag build it; Escape
clears it. Arrow keys move selected objects, modified arrow keys resize, and
inspector controls offer exact numeric editing. Locked objects and layers reject
editing.

Connectors expose focusable source and target handles. Port and endpoint changes
persist author intent. Orthogonal SVG paths are recomputed deterministically from
current nodes, groups, layer visibility, and scene overrides.

Scenes are authored as one bounded ordered sequence. Matching stable elements
morph between scene states; unmatched elements fade. With reduced motion, the
same state change is immediate.

## Interchange boundary

`org.sqlite-capsule.diagram-studio/1` is the example-specific JSON interchange
format. It is separate from the generic capsule format. It contains bounded
arrays of layers, nodes, connectors, groups, and optional scenes. Browser code
validates supported version, counts, IDs, references, finite geometry, ports,
routing modes, focus IDs, override IDs, and layer membership before preview.
The named import endpoint repeats critical relational checks and commits all rows
or none.

Clipboard paste remaps stable IDs and repairs references. User-selected import is
limited to one MiB and does not create a general host filesystem API. JSON and
SVG output are deterministic; standalone SVG embeds its style and has no remote
resources. PNG is derived locally from that SVG. Temporary object URLs are
revoked.

The manifest explicitly requests optional `clipboard.read`, `clipboard.write`,
`file.read.user-selected`, and `download` capabilities and continues to declare
network access as `none`.

## Named endpoints

Representative reads are:

```text
diagram.get
diagram.nodes
diagram.edges
diagram.layers
diagram.groups
diagram.scenes
diagram.history
```

Representative writes are:

```text
node.create / node.move / node.resize
nodes.transform / nodes.delete
edge.create / edge.configure / edge.delete
layer.update / group.toggle
scenes.apply / diagram.import
```

Every model write uses v0.2 ordered endpoint steps and writes one durable semantic
operation in the same transaction. The host knows none of these endpoint names
in advance.

## Scope boundaries

Diagram Studio 0.3.0 includes the host-neutral client and three
self-contained HTML experiences. `view` keeps the reader and scene player;
`interactive` adds read-only selection, pan/zoom, inspection, copy, and diagram
downloads; `editable` retains the full editor while the generic host owns dirty
state and explicit HTML revision saving. Worker policy, not hidden buttons, is
the final write boundary.

The example still does not add collaboration, arbitrary SVG ingestion, a plugin
API, cloud sync, general filesystem access, or rich text. Publisher authenticity
is not proven by internal hashes.

## Acceptance criteria

1. The app and compatible fallback host are served or extracted from the
   database after explicit verification and trust.
2. Browser writes use only typed, parameterised named endpoints.
3. Compound changes, their operation record, and history cursor commit or roll
   back together.
4. Mixed-shape editing, grouping, layers, connectors, scenes, layout, import, and
   export work offline in the real served browser.
5. Undo and Redo remain correct after page reload and host restart.
6. Keyboard operation, reduced motion, forced colors, 200% zoom, and the
   documented desktop/laptop/narrow layouts remain usable.
7. Domain foreign keys and application checks pass.
8. The generated capsule is a deterministic-enough clean rebuild of reviewable
   source and independently verifies and conforms as v0.2.
9. The one-sentence Codex launch path follows the embedded `START_HERE` runbook
   and reaches a matching healthy loopback URL without fetching dependencies.
