# M03 trusted-shell visual review

Date: 2026-08-12  
Platform: Windows 11, native Tauri/WebView2 host  
Capture source: real desktop process controlled through the Windows WebView2
debug port; no browser mock or static HTML substitute.

## Reviewed states

| Screenshot | Observed state | Review |
| --- | --- | --- |
| `screenshots/cabinet-empty.png` | No selected capsule | Clear empty Cabinet, prominent host-owned open action, disabled lifecycle previews, no application renderer content. |
| `screenshots/overview-legacy-v02.png` | Verified legacy v0.2, unsigned | Legacy profile is explicit; application and mutable capsule profile are visibly separate; publisher state is not styled as trusted. |
| `screenshots/overview-signed-v03.png` | Valid v0.3 signature, unknown publisher | Signature validity and unknown local publisher trust are separate labels; the mutable instance card does not inherit trust styling. |
| `screenshots/overview-unsigned-v03.png` | Structurally valid unsigned v0.3 | Unsigned is shown as a warning state without calling the application release signed or trusted. |
| `screenshots/overview-invalid-v03.png` | Invalid v0.3 signature | Invalid signature is blocked and visually distinct; no executable application asset is released. |
| `screenshots/overview-remembered-ready.png` | Persisted exact-release decision after restart | Overview remains first, raw assets remain locked, and the explicit host-owned Open action is available without repeating the capability prompt. |
| `screenshots/host-shell-light.png` | Trusted shell, light theme | Eight required top-level destinations remain reachable; hierarchy, focus, spacing and status chips are readable at the tested desktop size. |
| `screenshots/host-shell-dark.png` | Trusted shell, dark theme | Contrast and hierarchy remain readable without changing trust-state semantics. |
| `screenshots/publisher-signing.png` | Host-owned signing review | Sensitive action remains within the trusted shell and is visually separated from capsule-controlled content. |
| `screenshots/application-window.png` | Explicitly authorized raw application | Application content appears only in its separate native window after an explicit trusted-shell decision. |

## Boundary observations

- Overview is rendered before activation while the raw application window is
  hidden and remains at `/__host/locked`.
- Capsule-controlled text is inserted as text and does not control badge class,
  HTML, links, CSS, or action labels.
- Application, signature/publisher, mutable instance, and file state have
  separate host-owned visual groups.
- The real WebView2 Overview gate applies a 200% CDP page scale. Its measured
  `VisualViewport` is 590 by 450 CSS pixels at scale 2; heading text and the
  primary Cabinet action are both wholly inside that visual viewport, and the
  document has no horizontal overflow.
- Static accessibility tests enforce exactly one document-level `h1`, no
  skipped heading levels, non-empty heading text, and a heading relationship
  for every host page panel.
- The v0.3 compatibility fixture contains a deliberately tiny dark icon; the
  host safely renders its re-encoded derivative. This is fixture aesthetics,
  not a decoder or boundary failure.

## Limitations

- Automated keyboard, 200% scaling, semantic-heading, forced-color, and
  reduced-motion checks are retained,
  but M03 does not claim formal accessibility certification or a human
  screen-reader audit.
- Captures cover the supported Windows native target; macOS/Linux native hosts
  remain unsupported.
