# Documentation

The documents are layered so the generic SQLite Capsule format does not depend
on the Diagram Studio example.

## Concept and architecture

| Document | Scope |
| --- | --- |
| [`vision.md`](vision.md) | Product thesis, principles, and intended use cases |
| [`architecture.md`](architecture.md) | Components, execution model, boundaries, lifecycle, and distribution forms |
| [`security.md`](security.md) | Threat model, trust surfaces, controls, and current residual risks |
| [`decisions/`](decisions/) | Architecture decision records and rationale |

## Format and authoring

| Document | Scope |
| --- | --- |
| [`format-contract.md`](format-contract.md) | Current v0.2 database and launch contract |
| [`authoring.md`](authoring.md) | Clean source, mutable runtimes, semantic round trips, signing, and export boundaries |
| [`html-export-contract.md`](html-export-contract.md) | Self-contained HTML envelope, profiles, provenance, and save rules |
| [`references.md`](references.md) | Primary technical references and design influences |

## Host implementations and acceptance

| Document | Scope |
| --- | --- |
| [`native-host-contract.md`](native-host-contract.md) | Product-independent native trust, renderer, protocol, lifecycle, and update contract |
| [`browser-acceptance.md`](browser-acceptance.md) | Automated HTML-export browser matrix and manual validation gaps |
| [`../native/README.md`](../native/README.md) | Native host layout, build, test, packaging, and current platform limits |
| [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | Repository development and verification rules |

## Example

| Document | Scope |
| --- | --- |
| [`example-diagram-studio.md`](example-diagram-studio.md) | Diagram Studio behavior, domain model, and acceptance criteria |
| [`../examples/diagram-studio/README.md`](../examples/diagram-studio/README.md) | Reviewable source and rebuild instructions for the example capsule |
