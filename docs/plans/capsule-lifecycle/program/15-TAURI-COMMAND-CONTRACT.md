# Trusted Tauri lifecycle command contract

## 1. Boundary

Lifecycle commands belong only to the trusted Tauri shell. The raw Wry
application window retains its existing named endpoint bridge and receives none of
the commands, events, state handles or destination tokens in this document.

The native command allowlist and generated capability files are authoritative.
JavaScript conditionals and hidden controls are not security boundaries.

## 2. Command families

Exact names may adapt to repository conventions, but command semantics and
ownership are fixed.

### Selection and overview

```text
open_capsule_picker() -> CapsuleSelection
inspect_capsule(selection_id) -> CapsuleOverview
close_capsule(selection_id) -> ()
list_recent_capsules(page) -> CabinetPage
remove_recent_capsule(recent_id) -> ()
```

`CapsuleSelection` is an opaque host handle. `CapsuleOverview` contains bounded
host-defined fields only. Opening/inspecting does not execute app content.

### Destination

```text
choose_create_new_destination(suggested_name) -> DestinationSelection
cancel_destination(destination_id) -> ()
```

The host picker creates or reserves no user-visible file until execute. The token
records the exact parent/path decision and must fail if the path later exists or
aliases an input.

### Copy/fork/template

```text
prepare_copy(request_with_selection_tokens) -> PreparedCopy
execute_copy(plan_id, confirmation_nonce) -> OperationHandle
```

An internal `preview_copy` service may build the review model before this
command exists. It is not registered as a Tauri command and returns no
authority. When `prepare_copy` is enabled it accepts only host-minted selection,
destination and dataset-choice tokens; it never accepts the preview JSON,
filesystem paths, table names, SQL or a serialized lifecycle plan.

### Compare

```text
start_comparison(left_id, right_id, optional_base_id, limits) -> CompareHandle
get_comparison_summary(handle) -> CompareSummary
get_comparison_page(handle, validated_page_token) -> ComparePage
reveal_sensitive_page(handle, page_token, explicit_confirmation) -> ComparePage
close_comparison(handle) -> ()
```

The shell cannot send table names, SQL, offsets or arbitrary filter syntax.
Pagination tokens are minted by Rust and bound to the comparison/session digest.

### Reconcile

```text
prepare_reconcile(compare_handle, selected_change_tokens, resolutions,
                  destination_id) -> PreparedReconcile
execute_reconcile(plan_id, confirmation_nonce) -> OperationHandle
```

Selected change tokens refer to host-validated rows/fields. The shell does not
supply raw table/column identifiers.

### Upgrade

```text
prepare_application_upgrade(working_id, release_id, dataset_choices,
                            destination_id) -> PreparedUpgrade
execute_application_upgrade(plan_id, confirmation_nonce) -> OperationHandle
```

### Operation progress

```text
get_operation(operation_id) -> OperationStatus
cancel_operation(operation_id) -> ()
acknowledge_result(operation_id) -> ()
```

Progress events are sent only to the trusted shell label. Event payloads are
bounded and contain phase/counts, not row values.

## 3. Confirmation nonce

A prepared plan response includes:

- plan ID and digest;
- input identities/digests;
- output path display;
- identity effects;
- data/sensitivity choices;
- capability delta;
- checks to run;
- expiry;
- random one-use confirmation nonce tied to the trusted shell session.

Execution consumes the nonce. A raw window, stale UI, replayed command or process
restart cannot reuse it.

A nonce is not a substitute for command allowlisting, source rebinding and
destination no-replace enforcement.

## 4. Overview view model

```json
{
  "profile": "org.sqlite-capsule.tauri-overview/1",
  "selection_id": "<opaque>",
  "instance": {
    "title": "Payments architecture sketch",
    "description": "Working visual model",
    "document_kind": "diagram",
    "tags": ["payments"],
    "capsule_id": "<uuid>",
    "revision_id": "<uuid>",
    "profile_trust": "self-described"
  },
  "application": {
    "name": "Diagram Studio",
    "version": "0.4.0",
    "app_id": "org.sqlite-capsule.diagram-studio",
    "digest": "<sha256>",
    "publisher": {
      "state": "verified",
      "name": "SQLite Capsule Project",
      "key_id": "<fingerprint>"
    }
  },
  "file": {
    "display_path": "<bounded path>",
    "size_bytes": 412000,
    "writability": "writable"
  },
  "actions": {
    "open": {"enabled": true},
    "duplicate": {"enabled": true},
    "fork": {"enabled": true},
    "compare": {"enabled": true},
    "upgrade": {"enabled": true}
  }
}
```

The trusted shell renders fields as text. It does not render capsule-supplied HTML
or CSS.

## 5. Icon response

Prefer a host-owned object URL/custom protocol that serves only a verified,
decoded/re-encoded or safely decoded image under a random selection-bound token.
Do not expose a general `capsule_asset` path URL.

The token:

- is scoped to the trusted shell;
- expires with the selection;
- has fixed content type and length;
- refuses range or path variation unless explicitly implemented;
- cannot be requested by the raw window.

## 6. Cabinet cache

The Cabinet cache is host-local, owner-protected, versioned and rebuildable. It
may contain:

- canonical recent ID;
- source path/path identity;
- bounded cached title/application name/icon thumbnail;
- last-opened time;
- last observed digest/trust badge;
- missing/unavailable state.

It must not contain trust decisions, raw comparison values or operation
credentials. Opening always triggers fresh inspection.

## 7. Negative command tests

For every lifecycle command:

1. call from trusted shell — normal policy applies;
2. call from raw Wry renderer — command unavailable/denied before handler;
3. forge an event — raw window receives nothing;
4. replay an expired/consumed handle — rejected;
5. provide a handle from another window/session — rejected;
6. mutate arbitrary request JSON/table/path fields — deserialisation/validation
   fails closed;
7. close the trusted shell during operation — operation follows explicit
   cancellation/background policy, never transfers authority to raw window.

Generate or inspect Tauri capabilities in tests. Do not infer isolation solely
from integration behaviour.
