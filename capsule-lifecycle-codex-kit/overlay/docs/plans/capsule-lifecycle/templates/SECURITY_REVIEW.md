# Security review — <milestone/release>

- **Reviewer:** <independent agent/person>
- **Date:** YYYY-MM-DD
- **Commit/range:** <sha or range>
- **Threat model version:** <path/digest>
- **Scope:** <components and workflows>

## Trust-boundary inventory

| Boundary | Trusted side | Untrusted side | Native enforcement | Negative test |
| --- | --- | --- | --- | --- |
| … | … | … | … | … |

## Findings

### <ID>: <title>

- **Severity:** critical | high | medium | low
- **Status:** open | fixed | accepted
- **Affected paths:**
- **Invariant violated:**
- **Reproduction:**
- **Impact:**
- **Recommended correction:**
- **Fix and verification:**
- **Residual risk:**

Repeat for every finding.

## Attack matrix

| Attack | Fixture/setup | Expected | Actual | Evidence | Result |
| --- | --- | --- | --- | --- | --- |
| … | … | … | … | … | pass/fail/gap |

## Claims checked

- [ ] Inputs are read-only and unchanged.
- [ ] Outputs are create-new and no-replace.
- [ ] Plans are rebound before execution.
- [ ] Raw renderer has no lifecycle access.
- [ ] Signature compartments behave as specified.
- [ ] Metadata/icon parsing is bounded.
- [ ] Compare and logs do not disclose sensitive values by default.
- [ ] Migration is allowlisted and cannot write application/input compartments.
- [ ] Crash paths do not publish partial outputs.

## Gaps and release recommendation

State untested platform paths and whether release is approved, conditionally
approved or blocked. Unrun tests are gaps, not passes.
