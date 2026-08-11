# Windows 11 Fluent UI reference

Use this reference when the requested capsule should share the native Capsule
Host's Windows 11 visual language. It is a browser implementation: do not add
Tauri, Rust, WebView2 IPC, native window calls, or client-app imports.

The executable reference is the Capsule Inspector source at
`assets/examples/capsule-inspector/source/app/`. Reuse its tokens and component
grammar, then adapt hierarchy and signature element to the new domain.

## Visual system

Use Segoe UI Variable Text for controls/body and Segoe UI Variable Display for
page headings, with Segoe UI and system UI fallbacks. Use Cascadia Mono or
Consolas only for IDs, hashes, paths, and technical values.

The six anchors are:

| Role | Dark | Light |
| --- | --- | --- |
| Window smoke | `#0d0d0d` | `#d8d8d8` |
| App layer | `#202020` | `#f3f3f3` |
| Card | `rgba(255,255,255,.0512)` | `#ffffff` |
| Primary text | `#ffffff` | `rgba(0,0,0,.8956)` |
| Accent | `#60cdff` | `#005fb8` |
| Destructive | `#ff99a4` | `#c42b1c` |

Derive secondary text, strokes, hover surfaces, success, and warning from the
Inspector token block. Add no extra brand rainbow. Use two very faint radial
Mica-like tints over the app layer; keep content surfaces quiet.

## Geometry and hierarchy

- 48px application/title bar.
- 200–224px navigation rail on wide screens.
- Content surface with 1px top/left inner stroke and 8px top-left seam.
- 4px radius for controls, settings rows, tables, cards, and status panels.
- 12px only for a large welcoming empty surface; pill radii only for badges.
- 8px primary layout gap, 12–16px component padding, 24–36px page padding.
- 28px/600 page title, 14px/600 section title, 13–14px body, 10–12px metadata.

Fluent depth is expressed with layered translucency and strokes, not large
shadows. Use a shadow only for a true overlay or floating preview.

## Component grammar

- Navigation selection: quiet filled row plus a 3px accent rail.
- Primary button: accent fill, accent text, subtle darker bottom border.
- Secondary button: control fill, thin control stroke.
- Status: circular mark + title + one explanatory sentence; tint the whole
  panel for success/warning/failure.
- Settings/property rows: term in secondary text, technical value in mono.
- Tables: sticky quiet header, one-pixel row dividers, no zebra stripes unless
  row scanning has measured value.
- Empty state: one domain illustration, one sentence, one primary action, and
  at most three compact boundary facts.

Icons should be small, outline-based, and visually consistent. Prefer simple
inline SVG with `currentColor`; never fetch icon fonts.

## Design sequence

Before coding, state:

1. palette and theme behavior;
2. body/display/technical type roles;
3. navigation and content layout;
4. primary action and critical status;
5. one domain-specific signature element.

Build that direction, then inspect a real desktop screenshot and critique
hierarchy, density, contrast, clipping, and whether the signature element
actually clarifies the domain. Do not stop at “looks Fluent.”

## Responsive and accessible behavior

Below roughly 700px, turn the left rail into a horizontally scrollable page
bar, remove decorative rail content, and let the page flow vertically. Collapse
two-column cards and four-metric grids before text becomes cramped. Test near
the actual edge, not only at a phone preset.

Provide a skip link, semantic headings, accessible button names, `aria-live`
status, visible focus, sufficient contrast in both themes, and
`prefers-reduced-motion`. Never encode pass/fail only by color. Avoid a fixed
desktop minimum width—the capsule may run in a narrow browser or phone.

## Native boundary

“Same UI as the Windows app” means shared visual language and interaction
quality. It does not mean fake caption buttons, native APIs, Tauri globals, or
desktop-only assumptions. A capsule must still run through the generic Python
loopback host with all application assets embedded and offline.
