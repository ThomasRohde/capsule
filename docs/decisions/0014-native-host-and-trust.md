# ADR 0014: Native host and trust boundary

## Status

Accepted for the Windows implementation. Equivalent WKWebView and WebKitGTK
evidence is not present, so this decision does not claim macOS or Linux support.

The same-window placement in Decision 4, 5, and 8 is superseded by
[ADR 0020](0020-standalone-native-application-window.md). The raw no-Tauri,
no-IPC renderer boundary remains in force.

## Context

The Python loopback host and browser-only HTML host prove the capsule contract,
but neither supplies an installed desktop identity, publisher trust store, file
open lifecycle, native recovery UI, or signed updater. The native host needs those
facilities without moving application or Diagram Studio logic into a privileged
shell.

The desktop shell will process attacker-controlled SQLite files and execute
attacker-controlled browser assets after verification. Its privileged surface
therefore has to remain substantially smaller than the application surface.
Tauri capabilities apply to named windows and WebViews, not to arbitrary trust
assertions made by application content. The application child must receive no
generic Tauri API and no raw database or filesystem handle.

The Windows spike rejected a same-WebView iframe boundary. WebView2 exposed
both `__TAURI__` and `__TAURI_INTERNALS__` inside a sandboxed `srcdoc` child,
even though the child had an opaque origin and could not read its parent DOM.
The metadata command did not complete, but exposing the generic Tauri bootstrap
is already outside the native host's default-deny contract. CSP and iframe
sandboxing cannot be treated as a Tauri-IPC boundary on this platform.

The current development suffix, `.capsule.sqlite`, is also a compound suffix.
Linux shared MIME globs can match it precisely, but Windows file associations
are registered by filename extension and would associate the final `.sqlite`
extension. Claiming every SQLite database for the Capsule Host is unsafe and
surprising.

## Decision

1. Add a Rust workspace under `native/`. A dependency-light `capsule-core`
   crate owns metadata-only inspection and will grow the independently tested
   current v0.2 verifier and named-endpoint runtime. A thin Tauri 2 shell owns
   platform windows, signed packaging, and host-controlled interaction.
2. Keep the standard-library Python host and the SQLite WASM browser host. The
   native host is a third implementation of the same generic contract, not a
   replacement or Diagram Studio wrapper.
3. Pin the Rust toolchain and every direct crate version. Commit `Cargo.lock`,
   build with bundled pinned SQLite, prohibit dynamic SQLite extensions, and
   record licenses/advisories before distribution.
4. Split the desktop renderer into two native WebViews. The bundled host shell
   remains a Tauri-managed WebView with a minimal compile-time capability. The
   application is a raw Wry child WebView attached to the same native window;
   Tauri does not create, initialise, or grant it. Do not register a Wry IPC
   handler. Run it incognito with clipboard, developer tools, navigation,
   popups, network, and file URLs denied by construction and CSP.
5. Keep the child inside native bounds below the host-owned trust seam. Capsule
   CSS and script cannot overlap the identity, publisher, permission, recovery,
   or close controls because those controls live in the separate Tauri surface.
6. The host-neutral child bridge must be a dedicated, session-bound
   custom protocol or loopback RPC with an exact typed grammar. It may expose
   only manifest, effective permissions, named reads, and named writes. It must
   not use generic Tauri commands, a Wry `window.ipc` handler, raw SQL, database
   bytes, paths, trust mutation, backup state, or updater APIs.
7. No capsule executable asset is materialised until the Rust core has completed
   structural verification, publisher authentication, revocation evaluation,
   and required capability decisions. Pre-authorisation inspection exposes
   metadata only.
8. Keep identity, publisher, permission, recovery, and close controls in trusted
   non-overlappable host chrome. A local/developer trust path must look different
   from signed-publisher trust.
9. Accept `.capsule.sqlite` through command line, picker, drag/drop, and explicit
   **Open with** on every platform. Do not register the broad `.sqlite`
   association. Use the dedicated `.sqlitecapsule` suffix for automatic desktop
   association unless a platform-specific installer can prove an equally narrow
   compound-suffix registration. Hosts continue to identify every input by
   content and manifest, never by suffix alone.
10. File association is a delivery convenience only. `.sqlitecapsule` and
   `.capsule.sqlite` contain the same ordinary SQLite format and neither changes
   `format_id`, `format_version`, or `runtime_protocol`.

## Proof obligations

- metadata-only inspection rejects non-files, symlinks, oversized files,
  non-SQLite headers, wrong application IDs, unsupported format triples,
  malformed manifests, and non-object permission declarations;
- the trusted shell can show a sanitized startup report without returning an
  asset, endpoint SQL, database bytes, or a generic file-read primitive;
- the raw child renderer cannot see Tauri globals or internals, has no registered
  native message handler, and cannot navigate or overlap the trusted shell;
- command-line input is inspected before the Tauri event loop releases capsule
  content;
- automated Windows evidence is followed by native macOS and Linux evidence
  before the ADR is accepted.

## Windows evidence

The checked toolchain is Rust 1.97.1 with Tauri 2.11.5, Wry 0.55.1,
rusqlite 0.40.1, and bundled libsqlite3-sys 0.38.1. Exact direct versions are in
the workspace manifest and all transitive versions are locked in `Cargo.lock`.

The WebView2 run established both the rejected and accepted boundaries:

- a Tauri-managed sandboxed iframe had an opaque origin and blocked parent DOM
  access, but exposed `__TAURI__` and `__TAURI_INTERNALS__`; that path failed
  closed and was removed;
- the raw Wry child reported both Tauri globals absent, ran as a separate view,
  and had network and local-file fetches blocked;
- Wry always installs a `window.ipc` transport object on Windows, but no Rust
  handler is registered, so Wry discards its messages. Future capsule RPC must
  use the separately specified, typed host-neutral protocol instead;
- command-line launch displayed the checked capsule identity, format, runtime,
  application version, and byte size while explicitly retaining `Executable
  assets: Not released`;
- launching the same binary with `README.md` rejected the input as lacking
  a SQLite 3 header before execution.

The Rust workspace passed formatting, build, unit, integration, and Clippy
checks. macOS and Linux platform evidence is absent; the Windows implementation
is the supported native target.

## Consequences

- Rust/Tauri and a bundled SQLite library are explicitly permitted native-host
  dependencies; the bootstrap Python runtime remains standard-library-only.
- The shell can use current platform WebViews and packaging/signing machinery,
  but WebView differences are part of the acceptance matrix rather than assumed
  browser parity.
- A second generic runtime increases parity work. Shared fixtures and explicit
  compatibility reports are mandatory.
- Existing `.capsule.sqlite` files remain valid and openable. Automatic
  double-click association uses `.sqlitecapsule` on platforms where compound
  suffix registration is not narrow enough; this avoids hijacking unrelated
  SQLite files.
- Tauri's capability system protects only the trusted shell. The raw child is
  outside that capability graph; its dedicated protocol must validate
  state, session tokens, exact fields, limits, and trust/grant decisions.

## Primary references

- [Tauri capabilities](https://v2.tauri.app/security/capabilities/)
- [Tauri process model](https://v2.tauri.app/concept/process-model/)
- [Tauri application permissions](https://v2.tauri.app/security/permissions/)
- [Wry child WebViews](https://docs.rs/wry/0.55.1/wry/struct.WebViewBuilder.html#method.build_as_child)
- [Windows file types](https://learn.microsoft.com/windows/win32/shell/fa-file-types)
- [freedesktop shared MIME globs](https://specifications.freedesktop.org/shared-mime-info/latest-single/)
- [Apple document types](https://developer.apple.com/documentation/bundleresources/information-property-list/cfbundledocumenttypes)
