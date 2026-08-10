# Browser acceptance for self-contained HTML exports

This document records the browser support demonstrated by the checked test suite.
Playwright WebKit is a compatibility engine and is never presented as actual
Safari evidence.

## Automated matrix

The generated Diagram Studio 0.3.0 exports pass under ordinary loopback HTTP and
an independently served COOP/COEP configuration. Service workers are disabled,
and the test guard rejects unexpected HTTP(S) subresources.

| Project | Pinned engine | Result | Secure-origin capabilities observed |
| --- | --- | --- | --- |
| Chromium | 151.0.7922.34 | static matrix passed | Worker, Web Crypto, compression streams, file picker, OPFS |
| Firefox | 153.0 | static matrix passed | Worker, Web Crypto, compression streams, OPFS; no picker |
| WebKit compatibility | 26.5 | static matrix passed | Worker, Web Crypto, compression streams; no picker/OPFS |

The suite covers all three profiles; Python/WASM endpoint-result parity; direct
write denial; compound commit and rollback; dirty state; keyboard create,
Undo/Redo, and presentation; reduced motion; fallback download; independent
revision verification; ordinary SQLite corroboration after extraction; fresh
static reopen; provenance lineage; narrow layout; visible licenses; no-header
hosting; missing optional APIs; bounded hostile payloads; and failure before
entry-asset execution when integrity, schema, endpoint, or application checks
fail.

Chromium also exercises file-picker save success, cancel, denial, and I/O
failure with deterministic API doubles. Twelve baselines cover view,
interactive, editable, dirty, save-success, and error states at 1440×900 and
1280×720.

The supported-limit fixture is a valid 66,588,672-byte capsule, 520,192 bytes
below the 64 MiB policy ceiling. It boots, commits an edit, downloads a revision,
independently verifies it, and proves that its source capsule and initial export
remain unchanged in every automated engine. A just-over-limit declaration and a
compressed stream expanding beyond its declared buffer both fail before
application execution.

Run the matrix and generate the engine/capability report with:

```bash
node tools/browser_matrix_report.mjs
npm run test:browser:html
```

## Direct local-file validation

The optional `file://` lane is enabled with:

```bash
SQLITE_CAPSULE_RUN_FILE_TESTS=1 npm run test:browser:html
```

On Windows PowerShell:

```powershell
$env:SQLITE_CAPSULE_RUN_FILE_TESTS = "1"
npm run test:browser:html
Remove-Item Env:SQLITE_CAPSULE_RUN_FILE_TESTS
```

The current repository does not claim a passing direct local-file lane. The
automated agent browser used during development does not permit `file://`
navigation, so the rebuilt classic-worker bootstrap still requires an external
double-click/reload confirmation.

## Actual Safari validation

Actual Safari remains a manual release check on current macOS. Record macOS and
Safari versions and verify each profile under both `file://` and static
hosting:

| Profile | Required checks |
| --- | --- |
| `view` | boot/read, denied writes, fresh reopen, provenance |
| `interactive` | boot/read, denied writes, diagram download, fresh reopen, provenance |
| `editable` | named writes, HTML download and picker when offered, persisted fresh reopen, revision lineage |

Also check keyboard scene navigation, reduced motion, an invalid export, offline
network behavior, and fallback saving when the picker is unavailable.

## Support claim

The checked release supports the automated static-host matrix above. Direct
local-file use and actual Safari are explicit validation gaps; neither is
inferred from Chromium, Firefox, or Playwright WebKit results.
