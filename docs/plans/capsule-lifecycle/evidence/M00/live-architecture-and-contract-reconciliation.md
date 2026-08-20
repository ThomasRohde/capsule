# M00 live architecture and contract reconciliation

Captured 2026-08-12 against commit
`e73cf948fba233ef84d4680930b61549012020a7` on Windows x86-64.
This note records inspection, not production lifecycle implementation.

## Baseline movement

The programme kit expected `f67da560fb4baaa13144cea220c9329df87ad534`.
The observed checkout is three commits later:

```text
e73cf94 Regenerate deterministic capsule exports
9cd677d Fix Diagram Studio editing and undo
d5da5d6 Clean up
```

The intervening changes affect Diagram Studio assets/data, generated exports,
runtime tests, native documentation and one creator-plugin reference. They do
not add lifecycle commands, v0.3 dispatch or a workspace crate. The complete
dirty-state/toolchain/path inventory is in `baseline-20260812T143205Z.json`.

## Live call and data flows

### Python inspection, verification and conformance

```text
tools/capsule.py
  -> runtime/capsule_host.py CapsuleDatabase(read_only=True)
       -> metadata/instructions/assets, or CapsuleDatabase.verify()
       -> integrity + FK + versioned platform shape
       -> forbidden objects + asset/path/hash checks
       -> endpoint/permission compilation under authoriser
       -> declared checks under query-only bounds

tools/capsule.py conformance
  -> tools/capsule_conformance.py
       -> independent JSON-described v0.2 structural signal
```

Read-only connections use SQLite URI `mode=ro`, foreign keys,
`trusted_schema=OFF`, disabled extension loading and query-only controls where
applicable. The independent checker does not import the runtime verifier.
Python/HTML/plugin enforce the current 64 MiB capsule limit.

### Native first-open and exhaustive runtime verification

```text
capsule-core::inspect_header / inspect_metadata
  -> capsule-launch::verify_structure
       -> metadata + integrity + FK only (live gap)
  -> capsule-launch::inspect_launch
       -> file SHA-256 + signed-app inventory/verification
  -> desktop host policy/trust evaluation
  -> capsule-runtime::VerifiedCapsule::open
       -> exhaustive machine conformance + assets/endpoints/checks
  -> fresh ProtocolSession -> runnable raw renderer
```

The current `structure_verified` launch field is stronger wording than the live
pre-policy check supports. Direct native signing preparation uses the same
shallow helper, while `tools/sign_release.py` compensates with full Python
verification before and after. ADR 0028 requires a shared exhaustive read-only
verifier before persistent first-open decisions, signing preparation or
lifecycle planning, and aligns native size policy from 512 MiB to the normative
64 MiB.

### Signed application canonicalisation

`native/crates/capsule-crypto/src/lib.rs` is the executable v0.2 definition:

1. require signed-app/0.2 extension shape and `user_version = 2`;
2. reject unknown `capsule_*` tables;
3. frame the v1 stream context;
4. include every non-internal `sqlite_schema` record in binary order;
5. include rows of the exhaustive signed-table list in declared-column order
   and primary-key/BINARY order;
6. canonicalise four declared JSON columns with duplicate-key rejection and a
   1 MiB bound;
7. encode SQLite storage classes with explicit tags and length framing; and
8. SHA-256 the stream and verify Ed25519 over the v1 signature context, digest
   and exact `signed_at`.

Domain table schema is signed; domain rows are not. Mutable platform table
schema is signed; its rows are not. ADR 0022 retains this distinction and the
writer shape under new v2 contexts and an exhaustive v0.3 row allowlist.

### Native first-open and trust transition

The desktop host inspects on a fixed-stack worker, evaluates protected
host-local trust/grants, and either stages first-open review or activates a
complete remembered exact-release decision. Activation deactivates prior
runtime/protocol state, opens a verified runtime, and generates a fresh 256-bit
child session. Rejection, replacement, conflict and trust reset clear the
runtime and return the child to the locked probe.

Relevant live boundaries are in:

- `native/crates/capsule-launch/src/lib.rs`;
- `native/crates/capsule-policy/`;
- `native/crates/capsule-runtime/src/lib.rs` and `conformance.rs`; and
- `native/desktop/src-tauri/src/lib.rs` (`load_host_path`,
  `first_open_decide`, runtime activation/deactivation).

### Raw renderer protocol and negative boundary

The application renderer is a raw Wry WebView in a separate host-owned native
window. `install_sandbox_webview` registers only the `capsule` custom protocol,
explicitly omits `with_ipc_handler`, starts incognito/hidden/unfocused and
denies navigation/popups/clipboard by default. The exact child grammar in
`capsule-core/src/protocol.rs` admits only:

```text
manifest {}
permissions {}
read  {endpoint, arguments}
write {endpoint, arguments}
```

There is no SQL, path, trust, signing, backup, update or lifecycle method. Tauri
capability `native/desktop/src-tauri/capabilities/host-shell.json` targets only
webview label `main`; `host-first-open.toml` allowlists the trusted-shell
commands. Registered installer commands are intentionally absent even from that
permission.

Negative test points for M02 onward are:

1. raw protocol rejects every lifecycle-like method as unknown;
2. raw window has no Tauri globals or IPC handler;
3. every lifecycle Tauri command is absent/denied for the raw label before its
   handler;
4. raw event emit/listen cannot receive lifecycle sessions/reports;
5. handles/nonces are window/session-bound and reject replay/cross-window use;
6. capability generation continues to target only `main`; and
7. crate dependency tests prove `capsule-runtime`/raw protocol cannot depend on
   `capsule-workspace`.

Current regression locations include `capsule-core/src/protocol.rs` tests,
desktop unit tests around locked protocol handling, `tests/native/raw-child.e2e.mjs`,
and `tests/native/standalone-window.e2e.mjs`. New commands require an enumerated
negative case, not inference from hidden UI.

### Authoring, build and signing publication

- `tools/capsule_author.py::pack_capsule` reconstructs a same-parent temporary
  database, validates schema/data/FKs, vacuums, runs full Python verification
  and publishes. It has an explicit authoring `--replace` mode and is therefore
  not a lifecycle publication primitive.
- `tools/build_example.py::build_example` deterministically rebuilds and replaces
  the generated Diagram Studio distribution. It is example-specific.
- `capsule-signing::prepare_capsule_signing` opens the source read-only, uses the
  SQLite backup API into a private same-directory temporary, prepares the exact
  digest, rechecks before signing, verifies the result and publishes with
  `persist_noclobber`.
- `capsule-lifecycle::PinnedSource` supplies held file identity and replacement
  checks. A plan must additionally bind size, file digest and logical identities
  because same-file/same-size content mutation is not an identity change.

The Windows pin admits `FILE_SHARE_WRITE`, so a pre-execute recheck alone is
not a stable read boundary. ADR 0024 rejects source SQLite sidecars, captures
pinned main-file bytes into private create-new storage without SQLite touching
the input, binds that exact snapshot SHA-256 at plan and reproduces it at
execute, reads only the verified snapshot, and rechecks the source before
publication. M02 must cover same-object/same-size writes, change-capture-restore
ABA, WAL/journal state and destination-parent substitution.

ADR 0024 adopts the signing no-clobber shape, not authoring replacement behavior.

## Draft-contract reconciliation

| Draft surface | Live finding | M00 decision / correction | Owning milestone |
| --- | --- | --- | --- |
| `capsule-v0.3-draft.sql` | v0.2 manifest mixes app/instance; runtime bridge is still 0.2 | New `user_version=3`; split manifest/application/instance; keep `capsule-http/0.2`; add icon/cover pointers and a distinct host-profile ID | M01 |
| signed-app v0.3 SQL | Draft reversed live signature column order and blurred schema versus mutable rows | Preserve envelope order; new profile/v2 contexts; sign all schema plus application-row allowlist; exclude mutable/domain rows | M01 |
| v0.3 conformance draft | Non-exhaustive and lacked exact contexts/size | Record 64 MiB and v2 contexts now; M01 emits exact objects/columns/PK/FK/view/unknown-object policy | M01 |
| application profile | Host profile was populated with runtime protocol | `org.sqlite-capsule.host-profile/0.3` is a distinct recognized profile | M01 |
| instance profile | JSON/SQL icon names differed and no cover pointer existed | Use `icon_asset_id` and `cover_asset_id`; canonical UUID and exact UTC seconds | M01 |
| data contract | JSON alone cannot prove uniqueness, PK equivalence, column existence, acyclicity or full coverage | Rust validator cross-checks signed declarations against inspected schema; six roles plus orthogonal sensitivity | M02 |
| Diagram Studio contract example | Named five nonexistent tables and omitted live tables | Reconciled to all ten tables in `examples/diagram-studio/domain.sql`; remains example-only | M00/M02 |
| lineage projection | JSON omitted SQL result capsule/schema/details fields | Projection now includes them; M02 validates sequence/current revision but treats lineage as claims | M02 |
| lifecycle plan | File identity/expected result were optional; exposed a temp directory; preflight hashing did not close write/ABA or destination-parent races | Require expiry, exact private `snapshot_sha256`, source identity, logical expected result and stable destination parent/leaf identity; reject source journal state; remove client-controlled temp path; Tauri receives opaque handles/redacted projections | M02 |
| compare report | Monolithic schema can expose values and is not paginated | Summary-first, counts/digests by default; split paginated detail before M05 becomes normative | M05 |
| reconcile plan | Raw table/key/value structures require stronger host-token binding | Host-minted change tokens, target row/value preconditions and create-new target copy | M06 |
| upgrade plan | Example datasets did not match live Diagram Studio | Reconciled to content/history; same key/app/schema rules remain; target release is construction base | M07 |
| migration contract | `rebuild_endpoint` created application-SQL execution; literals/maps were untyped; declaration limits lacked canonical signed storage | Remove rebuild callback/operation; allow only copy rows/dataset/discard with typed SQLite wrappers and fixed assertions; limits are host-owned and plan-bound only | M08 |
| v0.2 legacy upgrade | Programme described an adapter not representable by the drafts | Explicitly unavailable until M08 accepts a separate signed legacy-adapter ADR/schema; planners never infer v0.2 datasets | M08 |
| lifecycle errors | Programme sections used divergent aliases | One profile `/1` catalogue now owns stable codes; `capsule-workspace` maps internal causes to its bounded safe messages | M02 |
| safe media | Declared dimensions/hash are attacker assertions | Hash and decode PNG/WebP under compressed, pixel, dimension and memory caps; deterministic fallback | M01/M03 |
| plugin snapshot | Standalone plugin is hard-coded v0.2 and currently passes its independent tests | No M00 framework implementation to sync; M01 must update all plugin surfaces only after canonical v0.3 source/vectors pass | M01 |

## Acceptance invariants at M00

- No production implementation was added; changed paths are programme docs,
  draft contracts, examples, ADRs, status/result and evidence only.
- No lifecycle input was opened writable and no lifecycle transform ran. Existing
  repository tests use their established temporary/disposable paths.
- The checked capsule remained at SHA-256
  `fa6168437d74e372b22485efdbf3db51721ce7f267364c2d2331c1784050f157`
  before/after non-mutating generated checks.
- `format/`, `runtime/`, `native/`, `plugins/capsule-creator/`, `capsules/` and
  `exports/` production/generated content were not modified by M00.
- Diagram Studio names occur only in example contracts/evidence, never generic
  format/runtime/workspace code.
- No Tauri command, permission, event or raw protocol method was added.
