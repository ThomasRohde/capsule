# Vision: the database is the artefact

## Thesis

A SQLite database can be more than a hidden persistence mechanism. It can be the durable, portable application artefact itself: data, schema, views, media, application assets, interaction contracts, presentation paths, documentation, validation, migrations, and instructions for humans and software agents in one file.

The working name for such an artefact is a **capsule**.

A capsule is opened by a small generic host in the same sense that an HTML file is opened by a browser. The host provides trusted execution and file access. The capsule supplies the actual application and its content.

```text
capsule.sqlite + generic host -> local application
capsule.sqlite + exporter     -> self-contained HTML
capsule.sqlite + sqlite tools -> inspectable data and schema
capsule.sqlite + coding agent -> discoverable, modifiable software artefact
```

## Why this is worth exploring

Modern documents and small applications have converged badly. Documents increasingly require proprietary cloud applications, while local applications scatter their state across folders, caches, services, package managers, and opaque formats. A recipient often has neither a complete artefact nor a durable way to understand it.

SQLite offers a different substrate:

- a stable, cross-platform application file format;
- relational structure and constraints rather than a pile of loosely related JSON files;
- transactions and recoverability;
- full-text search, JSON values, binary media, indexes, views, and triggers;
- broad tooling and language support;
- direct inspection when the original application is gone;
- easy copying, backup, attachment, hashing, and archival.

The idea is not to turn every application into SQLite. It is to create a strong format for bounded, local-first applications whose identity naturally belongs to one portable artefact.

## Principles inherited from single-file HTML applications

The project is inspired by the principles demonstrated by Bento presentations, while changing the primary file format from HTML to SQLite.

### 1. One file is the product

The distributable unit is not a project folder or hosted workspace. It is one database file. Copying the file copies the application state and its intended experience.

### 2. Local-first and offline by default

Opening, viewing, editing, presenting, and saving must not require an account, backend, CDN, package registry, or network connection.

### 3. The viewer and editor travel with the content

Application HTML, CSS, JavaScript, templates, and declarative views live in the capsule. Only a small generic trusted host is external. The self-contained HTML derivative includes the pinned browser host and database payload so a browser alone is sufficient while SQLite remains canonical.

### 4. The artefact remains inspectable without its preferred UI

A capsule must retain semantic value as an ordinary SQLite database. Core content cannot exist only as an opaque rendering cache. Tables, columns, identifiers, constraints, and documentation should make sense to people and tools.

### 5. Saving updates the artefact itself

Edits are committed transactionally to the same database. The file is not merely an import format for a hidden workspace.

### 6. Presentation is a view, not a second document

A deck, guided tour, dashboard, report, gallery, or narrative is a saved path through the underlying data. It should update as the data changes and reuse stable object identities for transitions.

### 7. Agents are first-class readers and operators

The capsule carries an explicit runbook, command templates, schema notes, validation checks, and safe named operations. A coding agent should be able to inspect and run it without relying on stale external setup instructions.

### 8. Longevity beats cleverness

The design should degrade gracefully. If custom rendering fails, the data, SQL schema, embedded documentation, and standard exports remain usable. Avoid dependencies that make old files impossible to open.

## What SQLite adds beyond a self-contained HTML file

A plaintext JSON document is easy to inspect and modify but becomes strained when the artefact contains many related objects, large media, histories, indexes, queries, derived views, and migrations. SQLite supplies those capabilities natively.

The trade-off is that a database cannot execute itself. A capsule therefore has an explicit host boundary. That boundary is useful rather than embarrassing: it creates a place for security enforcement, compatibility, permissions, and file lifecycle management.

## Core product model

A capsule may contain several layers:

```text
Identity and manifest
Embedded runbooks and documentation
Application assets and declarative UI metadata
Domain schema and content
Named read/write endpoints
Saved views, scenes, dashboards, and presentation paths
Media and generated artefacts
Validation checks and migrations
Provenance and change history
```

Not every capsule needs every layer. The format should support a minimal document-like artefact and richer interactive applications without forcing either into the other's complexity.

## Candidate applications

The concept is deliberately not tied to enterprise architecture. Suitable domains include:

- visual diagram and canvas tools;
- personal fieldbooks and nature observations;
- cooking and workshop experiment logs;
- interactive fiction and small game worlds;
- object museums and repair histories;
- offline research dossiers;
- family archives and oral histories;
- generative art instruments;
- annotated maps and walks;
- educational simulations;
- prompt and model experiment archives;
- small scientific notebooks with live views.

The first example is a diagram studio because it makes the idea visible: application code, graphical objects, connectors, scenes, edits, and presentation state all reside in the database.

## Authoring, runtime, and distribution

Three representations must be distinguished.

### Reviewable authoring source

During platform development, HTML, JavaScript, SQL, and documentation live as normal repository files so they can be reviewed, tested, and versioned effectively.

### Runtime source of truth

Once assembled, the `.capsule.sqlite` file is the application the user opens and edits. The host reads assets and contracts from it, and user changes are saved into it.

### Distribution exports

A capsule may be exported as:

- the source `.capsule.sqlite` file;
- a view-only self-contained HTML snapshot;
- an interactive self-contained HTML file carrying a database payload;
- static SVG, PNG, PDF, Markdown, JSON, or CSV views;
- a signed archival package.

Exports are derivatives. The database remains the canonical editable artefact unless an export explicitly becomes a new branch.

## Long-term experience

A mature experience could look like this:

1. A user double-clicks a `.capsule.sqlite` file or drops it onto a host.
2. The host verifies the file, displays its identity and requested permissions, and opens its embedded application offline.
3. The application edits the database through a constrained capability bridge.
4. The user switches between free exploration, saved views, and presentation mode.
5. A coding agent can open the same file, read `START_HERE`, inspect the schema, make controlled changes, run validation, and launch it.
6. The user exports a single HTML file for recipients who do not have the host.
7. Years later, ordinary SQLite tools can still recover and understand the content.

## Non-goals for the bootstrap

The bootstrap does not attempt to provide:

- cloud accounts or hosted workspaces;
- arbitrary server-side code execution;
- multi-user collaboration or CRDTs;
- an application marketplace;
- a universal low-code platform;
- automatic execution of untrusted capsules;
- a final standard or stable public extension;
- parity with mature drawing, presentation, or database products.

The immediate job is to prove the format and interaction loop with one compelling vertical slice.

## Success criteria

The concept is credible when:

- one file clearly behaves as both data and application;
- its preferred UI is visually useful rather than a database demo;
- edits survive restart in the same file;
- an agent can discover how to run it from instructions inside the file;
- the generic host remains ignorant of the example domain;
- the file is useful through ordinary SQLite inspection;
- security boundaries are explicit and conservative;
- the architecture supports self-contained HTML export without making HTML the authoring source of truth.
