# Trusted shell UX specification

## 1. Product framing

Rename the primary shell presentation from a security settings experience to a
Capsule Cabinet. Security remains visible and explicit, but it is one part of
understanding and managing the file.

Suggested window title:

```text
SQLite Capsule
```

When a capsule is selected:

```text
<instance title> — SQLite Capsule
```

## 2. Navigation

```text
Cabinet
Overview
Lineage
Compare
Versions
Security
Recovery
Settings
```

Existing pages map as follows:

- Trust review + Capabilities → Security
- Data protection + Restore → Recovery
- Publisher signing → Security / Publisher tools
- Host updates → Settings / Host updates
- Application window → Overview status or Security boundary details
- Local trust controls → Security / Local trust

Do not remove existing controls until their replacement is covered by tests.

## 3. Cabinet

When no file is selected, show a host-local recent-file grid/list:

- safe cached icon/fallback;
- capsule title;
- application name/version;
- last opened time;
- file availability;
- trust badge;
- read-only/unsupported badge.

The cache is not canonical. Opening a card always re-inspects the file. Missing
files can be removed from recents without touching trust history.

Primary actions:

```text
Open capsule…
Create from template…
```

Template discovery is local only and may initially require selecting a capsule
release file.

## 4. Overview

Overview is the first page after successful bounded metadata inspection.

### Header card

```text
[icon]  Payments architecture sketch
        Diagram Studio · 0.4.0
        Working diagram from the August workshop

        [Open] [Open read-only] [Create copy…] [Compare…]
                                             [Upgrade application…]
```

### Identity chips

- Verified publisher / unsigned / invalid signature
- Local file
- Fork / upgraded / reconciled
- Format v0.3
- Data schema v4

### Sections

1. **About this capsule**
   - instance description, tags, document kind;
   - capsule and revision IDs under technical details.

2. **Application**
   - signed name, description, app ID, version and exact digest;
   - publisher identity and key fingerprint;
   - requested capabilities and delta from a selected upgrade.

3. **Lineage**
   - compact event path;
   - immediate parents;
   - open Lineage page.

4. **File**
   - path, bytes, modified time, writable/read-only classification;
   - backup/recovery status.

5. **Security summary**
   - network/filesystem/database capability summary;
   - trust decision;
   - link to full Security page.

## 5. Create copy workflow

A single entry action `Create copy…` opens choices:

### Duplicate

> Create a consistent snapshot with the same capsule and revision identity.
> Use this for transfer or backup.

### Compact duplicate

> Create the same logical revision while removing unused SQLite pages. File
> bytes and hashes will differ.

### Fork with current data

> Create an independent capsule with a new identity, retaining selected current
> data.

### Create from template

> Create a new blank/seeded instance from a clean application release.

### Selective fork

Only shown if the data contract supports it. Show datasets, role, sensitivity,
dependency warnings and estimated row counts.

All workflows:

- show source and destination;
- preview identity effects;
- show application-signature result;
- require destination selection;
- produce a final verified result card;
- never offer overwrite.

## 6. Compare

### Pair selection

- left and right capsule;
- optional common-ancestor/base capsule;
- no execution;
- compatible/incompatible summary before detail.

### Comparison layers

Tabs or stacked sections:

1. Identity and lineage
2. Application
3. Data schema
4. Data

Dataset rows show counts:

```text
diagram-content   3 added · 2 changed · 0 removed
history           ignored by policy
layout-cache      derived · not compared
```

Detail view supports bounded row and field pages. Values from sensitive datasets
are masked until explicit reveal.

## 7. Reconcile

Use the phrase:

```text
Apply selected changes to a new copy
```

Never label the primary action simply `Merge`.

The review screen shows:

- target-derived output identity;
- selected additions, updates and deletions;
- conflicts and resolutions;
- unresolved conflict count;
- source and target digests;
- output path;
- application signature expected unchanged;
- validation checks that will run.

Execution is disabled while conflicts or validation preconditions remain.

## 8. Application upgrade

Label clearly to avoid confusion with host updates:

```text
Upgrade application…
```

Wizard:

1. Select clean newer application release.
2. Verify application/publisher compatibility.
3. Review version, signed assets and permission delta.
4. Review data schema and migration path.
5. Review datasets carried, reset, rebuilt or omitted.
6. Select new output path.
7. Execute and validate.
8. Open upgraded copy or return to Overview.

The original remains unchanged. A permission increase returns the upgraded copy
to normal trust review before application execution.

## 9. Accessibility

- full keyboard navigation and visible focus;
- semantic headings and landmarks;
- no status communicated by colour alone;
- `aria-live` only for concise operation status;
- dialogs trap focus and restore it on close;
- long digests have copy buttons and accessible labels;
- comparison grids have a non-grid list/table fallback;
- motion respects `prefers-reduced-motion`;
- all destructive-looking actions state that inputs remain unchanged.

## 10. Empty/error states

Provide precise, stable explanations:

- `This v0.2 capsule can be duplicated, but signature-preserving fork requires a v0.3 application release.`
- `The two capsules use different applications. Data reconciliation is unavailable.`
- `No unique migration path exists from data schema 2 to 4.`
- `The source changed after the plan was reviewed. Create a new plan.`
- `The destination now exists. Choose another path.`
