# Diagram Studio capsule

Diagram Studio is the first worked example of the SQLite Capsule idea. It is deliberately visual and interactive: the database renders a diagram of its own architecture and supports mixed-shape editing, multi-selection, layers, groups, routed connectors, authored scenes, deterministic layouts, durable Undo/Redo, and offline interchange.

It is an example, not the definition of the platform. The generic capsule format and host know nothing about diagrams. They know how to inspect and verify a capsule, serve assets, expose a narrow set of named parameterised data operations, persist transactions, and report health.

The example demonstrates five claims:

1. A SQLite database can be the distributable application artefact rather than hidden persistence.
2. Application assets, semantic content, views, validation, documentation, and operating instructions can travel together.
3. A small external host can remain generic, like a browser or virtual machine.
4. Visual edits can round-trip into the same file without granting browser code arbitrary SQL execution.
5. A coding agent can discover how to operate the artefact from instructions stored inside it.

Version 0.3.0 is a strong offline demonstration rather than a mature diagram editor. Every model mutation is a bounded atomic named command; semantic operation history survives restart. The same host-neutral client now runs through the Python loopback host or the self-contained SQLite WASM HTML host. View and interactive profiles are worker-enforced read-only; editable HTML can create a verified next revision through explicit save or download. Clipboard and user-selected JSON import remain validated and bounded, SVG/PNG exports have no remote dependencies, and optional capabilities are declared in the manifest.
