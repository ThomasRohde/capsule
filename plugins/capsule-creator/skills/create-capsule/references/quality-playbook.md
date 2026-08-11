# Exceptional capsule quality playbook

A valid capsule is the floor. An exceptional capsule feels like a deliberate
small product whose data, interface, guidance, and recovery behavior agree.

## Begin with a product model

Write a one-page source plan before coding:

1. user and job-to-be-done;
2. domain nouns, identities, ownership, and lifecycle states;
3. the smallest complete read/write vocabulary;
4. critical invariants and failure recovery;
5. screen hierarchy and one signature interaction;
6. offline/import/export expectations;
7. acceptance evidence.

Prefer a coherent narrow application to a feature checklist. Preserve stable
IDs, deterministic ordering, explicit units, UTC storage, and readable domain
names. Separate current state from history when the user must understand both.

## Make every layer say the same thing

| Product claim | Required evidence |
| --- | --- |
| “Offline” | `network.value = none`, no remote URLs, successful offline browser run |
| “Editable” | Named parameterised write, change-log entry, reload persistence, direct DB proof |
| “Read-only” | No enabled writes and no `database.write` capability |
| “Safe import” | Bounded file size/type, inert rendering, explicit failure states |
| “Inspectable” | Useful START_HERE, docs, prompts, checks, stable schema vocabulary |
| “Deterministic” | Unchanged source produces identical SHA-256 |

Never compensate for a weak runtime boundary with prose. Never claim runtime
behavior from source inspection alone.

## Interaction completeness

Every page or mode needs:

- an obvious primary action;
- purposeful initial/empty state;
- loading state that names the work;
- bounded success feedback;
- error text that says what remains safe and what to do next;
- keyboard focus and visible focus rings;
- mobile behavior at the real breakpoint edge;
- reduced-motion behavior;
- no horizontal page overflow;
- accessible names and live regions for asynchronous status.

Use semantic HTML and `textContent` for database strings. Avoid dense dashboard
grids when a list, detail panel, or timeline explains the work more directly.
Choose one visual signature that serves the domain, such as the Inspector's
four-layer X-ray rail.

## Endpoint and data quality

- Keep endpoint names in `noun.verb` form and stable across UI revisions.
- Make list ordering total and deterministic by ending with a unique key.
- Return only fields the consumer needs.
- Enforce length/range/state transitions in SQL constraints or predicates.
- For destructive changes, require the identity/state expected by the caller.
- Use compound steps plus `required_changes` for atomic multi-table workflows.
- Add checks for dangling references, invalid states, impossible dates, and
  seed assumptions.
- Provide a domain-specific read endpoint agents can use to understand state
  without reverse-engineering tables.

## Embedded knowledge quality

START_HERE should lead from distrust to inspection, verification, explicit
trust, launch, health confirmation, and targeted stop. Docs should include the
domain glossary, data ownership, limitations, and recovery. Prompts should be
actionable against real tables/endpoints and state what evidence to preserve.

## Acceptance matrix

Before delivery, capture all relevant rows:

| Gate | Evidence |
| --- | --- |
| Source | Reviewed project tree; no generated DB edits |
| Build | JSON success with identity, inventory, SHA-256 |
| Freshness | `check` reports identical current/expected hash |
| Format | bundled `verify` and independent `conformance` both pass |
| Runbook | bundled `instructions` is complete and correct |
| Browser | real loopback load, initial state, core read path, console review |
| Writes | declared write, reload, direct SQLite row/change-log evidence |
| Responsive | desktop, mobile, and breakpoint-edge screenshots/geometry |
| Offline | browser network disabled after initial local load; core flow remains |
| Safety | malformed/oversized input and untrusted text remain bounded/inert |

Report any acceptance row not exercised as an unresolved gap. Do not call a
feature complete because unit tests passed while the generated artifact or
browser path remains untested.
