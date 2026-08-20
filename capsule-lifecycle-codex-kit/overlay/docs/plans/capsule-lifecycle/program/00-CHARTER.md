# Programme charter

## Mission

Turn SQLite Capsule from a trusted launcher plus embedded application format
into a durable, user-owned application-document system with explicit lifecycle
semantics.

The native host must support operations that users expect from documents and
versioned application artefacts without compromising the existing security
model:

- understand a capsule before execution;
- create safe copies and independent variants;
- compare compatible capsules;
- selectively reconcile data;
- move a working capsule to a newer application release;
- retain provenance, recoverability and audit evidence.

## Product thesis

A Capsule is simultaneously:

- a SQLite application file;
- a document or working dataset;
- a signed embedded application release;
- a portable interface and presentation;
- an agent-readable software artefact.

The lifecycle model must therefore avoid conflating the software publisher's
authority with the user's ownership of a particular document instance.

## Core invariant

> The publisher signs the application release. The user owns the capsule
> instance and its domain data.

Application assets, endpoint declarations, permissions, checks, data contracts
and migration declarations belong to the signed application compartment.
Instance identity, title, description, safe cover metadata, lineage and ordinary
domain rows remain mutable without invalidating that signature.

## Success criteria

A user can:

1. select a capsule and see a host-rendered overview without executing it;
2. distinguish application identity, publisher identity, capsule identity and
   file identity;
3. duplicate a consistent snapshot;
4. fork a capsule into an independent logical instance;
5. create a clean instance from a declared template/application release;
6. compare two compatible capsule revisions without executing either;
7. apply selected changes to a new target-derived copy;
8. upgrade a working capsule to a newer trusted application release;
9. inspect lineage and verification evidence for every non-identical result.

## Constraints

- Offline-first. No remote marketplace or application download is introduced.
- Windows x86-64 remains the native acceptance target unless the repository
  expands support during implementation.
- The Python bootstrap remains standard-library-only.
- Browser applications remain free of runtime network dependencies.
- No raw SQL or generic native IPC is exposed to embedded application code.
- Inputs are never modified by copy, compare, reconcile or upgrade operations.
- New outputs are published only after complete validation.
- Existing v0.2 semantics remain explicit and compatible.

## Programme governance

Material semantic choices are recorded as ADRs. Every milestone has its own
acceptance gate. Security and compatibility evidence are first-class outputs,
not final documentation chores.
