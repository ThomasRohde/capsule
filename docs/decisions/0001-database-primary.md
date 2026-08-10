# ADR 0001: SQLite is the canonical runtime artefact

- Status: Accepted for bootstrap
- Date: 2026-08-07

## Context

A Bento-like self-contained application could use HTML as both shell and document. The proposed system also needs relational data, constraints, queries, media, histories, and multiple derived views.

## Decision

Use a SQLite database as the canonical editable runtime and distribution artefact. HTML is an application asset inside the database or a derivative export, not the primary source of state.

## Consequences

- The file gains transactional and relational capabilities.
- Ordinary SQLite tooling remains useful after the preferred UI disappears.
- A generic host is required because a database cannot execute itself.
- Source-code authoring remains a separate reviewable representation during development.
- Self-contained HTML distribution becomes an export problem rather than the canonical model.
