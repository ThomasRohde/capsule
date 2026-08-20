# Trusted shell UX acceptance — <workflow>

## Fixture and build

- Commit/build:
- Capsule fixture:
- Windows/WebView2 version:
- Display scale:
- Input method:

## Flow

| Step | Expected focus/announcement/state | Actual | Evidence |
| --- | --- | --- | --- |
| Open workflow | … | … | … |
| Review | … | … | … |
| Destination/selection | … | … | … |
| Execute | … | … | … |
| Result | … | … | … |

## Checks

- [ ] Full keyboard completion with visible focus.
- [ ] Focus enters and returns from dialogs correctly.
- [ ] Labels and headings expose purpose and state.
- [ ] No state is communicated by colour alone.
- [ ] Reduced-motion preference is respected.
- [ ] 200% scaling does not clip essential text or controls.
- [ ] Input-unchanged/create-new semantics are clear.
- [ ] Publisher and mutable profile are visually distinct.
- [ ] Errors preserve stable code/detail and offer a practical next action.
- [ ] Raw renderer cannot invoke or overlay trusted workflow surfaces.

## Manual gaps

Record screen-reader, high-contrast or platform checks not run. Do not mark them
passed by inference.
