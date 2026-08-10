# Diagram Studio example

This directory contains the reviewable source used to assemble `capsules/diagram-studio.capsule.sqlite`.

The example is intentionally separate from the platform design:

- `domain.sql` defines only the diagram domain.
- `source/app/` contains only the diagram browser application.
- `source/data/diagram.json` contains the sample diagram, semantic layers, groups,
  connector intent, and presentation scenes.
- `source/app/geometry.js` and `source/app/interchange.js` contain deterministic,
  directly testable browser logic without a bundle or runtime dependency.
- the remaining JSON and Markdown files are records embedded into generic capsule tables.

Build and validate from the repository root:

```bash
python tools/build_example.py
python -m unittest discover -s tests -v
python tools/capsule.py verify capsules/diagram-studio.capsule.sqlite
python tools/build_example.py --check
```

Do not hand-edit the generated database as the durable application-authoring workflow. Runtime edits made through the running application prove persistence and can be recovered with generic `capsule unpack` and `capsule diff`; intentional release-source changes should be represented here and rebuilt.

The example is native format v0.2. All browser model writes are named, typed,
parameterised compound commands with durable semantic history. Optional clipboard,
user-selected file read, and download capabilities remain browser-mediated; the
runtime network capability is `none`.
