# Trusted shell UX and accessibility review

Review the implemented Cabinet, Overview, Create copy, Compare, Reconcile and
Upgrade workflows as a document/application lifecycle product, not merely a
security settings screen.

Verify:

- Overview is the first safe page after inspection.
- Application, publisher, capsule instance and file state are distinguishable.
- Operation names explain identity/data effects.
- Inputs-unchanged and create-new semantics are visible at review and completion.
- Disabled actions state the exact compatibility reason.
- Sensitive data requires explicit disclosure.
- Errors map stable native codes to actionable text without losing technical detail.
- Keyboard-only flow, focus restoration, headings/landmarks, labels, scaling,
  high contrast, reduced motion and live-region use are correct.
- Long digests and comparison details remain usable without horizontal traps.
- Dialogs never rely solely on colour, icon or animation.
- Existing Security, Recovery, signing and host-update functions remain reachable.
- Raw renderer cannot invoke or imitate trusted lifecycle dialogs.

Exercise at least v0.2 legacy, v0.3 signed, unsigned, invalid, forked, compared,
conflicted and upgrade-available fixtures. Record screenshots, keyboard order and
remaining manual screen-reader gaps.
