# ADR 0005: Self-contained HTML is an export profile

- Status: Accepted direction; not implemented
- Date: 2026-08-07

## Context

A single HTML file is the most convenient distribution format for recipients with only a browser, but treating it as canonical would weaken the SQLite-first model.

## Decision

Keep SQLite canonical and design a future exporter that packages a host runtime and database payload into one HTML file. Support view-only, interactive, and editable profiles.

## Consequences

- The project can preserve Bento-like portability.
- Export/save-back compatibility requires careful browser testing.
- Exported files need provenance and branching semantics.
- Browser-only execution should reuse the capsule contract rather than define a separate document model.
