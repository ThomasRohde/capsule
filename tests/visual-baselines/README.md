# Visual regression baselines

The checked images are the current reviewed Diagram Studio baselines from a
freshly built and verified temporary capsule. Images support, but do not replace,
semantic assertions and persistence tests.

## Baseline environment

- Browser: the Chromium runtime pinned by `@playwright/test` 1.62.1.
- Source: a temporary verified copy of
  `capsules/diagram-studio.capsule.sqlite`.
- Host: `tools/capsule.py` bound to `127.0.0.1` with
  `--trust-capsule` for the repository-owned example.
- Viewports: 1440×900 desktop, 1024×768 laptop, and 720×900 narrow.
- Fonts: the checked CSS system stack on Windows Chromium.

Pixel comparisons are authoritative only in the pinned browser environment.
Cross-platform checks assert semantic layout and behavior.

## Current images

- `diagram-studio-v02-desktop.png`: complete authoring shell.
- `diagram-studio-v02-laptop.png`: wrapped toolbar and three-pane workspace.
- `diagram-studio-v02-narrow.png`: single-pane narrow layout.
- `diagram-studio-v02-selected.png`: keyboard-selected node and resize handle.
- `diagram-studio-v02-multi-layered.png`: multi-selection, grouping, and layers.
- `diagram-studio-v02-routed-connector.png`: selected routed connector.
- `diagram-studio-v02-scene-authoring.png`: selected authored scene.
- `diagram-studio-v02-presentation.png`: scene presentation and navigation.

The semantic suite checks the complete toolbar at all viewports, absence of
document-level horizontal overflow, keyboard connector handles, atomic
interchange paste, restart-safe undo/redo, local downloads, reduced motion, and
hostile interchange input. `html/` covers the self-contained export profiles.

## Updating baselines

1. Build the example and copy it to a fresh temporary path.
2. Inspect and verify the copy before launch.
3. Start only that copy and confirm its health identity.
4. Set the exact viewport and wait for `#app[aria-busy="false"]`.
5. Exercise mutations only on the copy; reload before claiming persistence.
6. Review every changed image and record the intentional rendering difference.
7. Stop the matching host and remove only the verified temporary directory.
