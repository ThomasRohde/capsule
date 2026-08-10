# ADR 0020: Standalone native application window

## Status

Accepted for the current Windows native host on 2026-08-10. Equivalent native
macOS and Linux window evidence remains part of the existing platform acceptance
gap.

## Context

ADR 0014 placed the raw Wry renderer below the trusted controls in one native
window. That topology kept the trust seam visibly adjacent to capsule content,
but left the application with only the small area below a tall review and
administration surface. Diagramming and other workspace-style capsules need the
available monitor work area after the user has authorised execution.

Moving the application into a Tauri WebviewWindow would increase its privilege:
the Windows spike already showed that Tauri initialization can be present in
renderer contexts even when DOM isolation appears effective. The standalone
window therefore must not introduce a second Tauri WebView or a Wry IPC handler.

## Decision

1. Keep `main` as the only bundled Tauri WebView and the only target of the
   compile-time host capability.
2. Create a separate host-owned native `Window` with no Tauri WebView. Attach the
   raw Wry application renderer directly to that window and keep the existing
   custom-protocol, CSP, navigation, popup, clipboard, incognito, and no-IPC
   restrictions.
3. Create the application window hidden and unfocused. Show, maximize, and focus
   it only after launch policy has released the verified entry asset.
4. Hide the application window and focus the trusted shell whenever the renderer
   is locked for a new source, rejection, conflict, or trust reset.
5. Treat a close request from either native window as an application close. Run
   the same final verified checkpoint first; if it fails, keep the application
   open and report the failure through the trusted shell.
6. Let the user restore or resize the maximized application window. The raw
   renderer fills the complete client area and follows native resize events.

The pinned Tauri `unstable` feature is enabled solely to construct a native
window without a bundled WebView. Direct dependency versions remain exact, and
the raw renderer remains outside Tauri's capability graph.

## Consequences

- Authorised capsules start with substantially more screen space and behave like
  ordinary standalone desktop applications.
- Trust and administration remain in a separate independently focusable window.
  Capsule content is absent while those pre-execution decisions are unresolved.
- The application window can overlap other operating-system windows after
  authorisation, as any normal desktop window can, but capsule code cannot draw
  outside its client area or invoke host window controls.
- Window lifecycle tests must cover both close entry points and verify that a
  failed final checkpoint still prevents application shutdown.

## Primary references

- [ADR 0014](0014-native-host-and-trust.md)
- [Native host contract](../native-host-contract.md)
- [Wry standalone WebView builder](https://docs.rs/wry/0.55.1/wry/struct.WebViewBuilder.html#method.build)
