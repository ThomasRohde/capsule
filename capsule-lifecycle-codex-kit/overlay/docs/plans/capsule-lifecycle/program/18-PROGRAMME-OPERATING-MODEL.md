# Programme operating model for Codex

## 1. Durable state

Conversation context is not authoritative. The durable state is:

- `PROGRAM_STATUS.json`;
- current milestone `RESULT.md`;
- accepted ADRs;
- repository Git history/diff;
- evidence files and exact command outputs.

A new session must be able to resume from these alone.

## 2. Milestone transaction

Each milestone follows:

```text
inspect → mark in progress → implement slices → test → independent review
       → fix → run gate → record evidence → mark complete → handoff
```

Do not mark complete before the gate. Do not let the next milestone hide
unfinished work from the current one.

## 3. Commit discipline

Recommended:

- one commit for an accepted prerequisite/ADR when it materially changes the plan;
- one or more coherent implementation commits;
- one final test/docs/generated-artifact commit if needed;
- milestone result/status included in the completion commit.

Avoid huge mixed commits and drive-by cleanup. Preserve generated/source
relationships.

## 4. Parallel agents

Good bounded assignments:

- inspect a specific crate and return a path/contract map;
- implement a non-overlapping pure module plus tests;
- review canonical vectors independently;
- attack a single trust boundary;
- exercise accessibility/visual flows;
- verify plugin snapshot parity.

Bad assignments:

- two agents editing the same contract/crate;
- delegating final integration/evidence;
- asking a subagent to "finish the milestone" without the full gate;
- accepting a critic's claim without reproducing it.

The lead agent integrates and reruns tests.

## 5. Evidence quality

Evidence must name:

- exact commit and dirty state;
- exact command/cwd/environment;
- exit code and pass/fail counts;
- fixture identities/digests;
- before/after source hashes for transforms;
- report/output paths;
- known unrun platform checks.

Screenshots support UX evidence but do not prove native isolation. Static
capability inventory and negative raw-window tests are also required.

## 6. Handling changed repository state

When the checkout has moved beyond the package baseline:

1. capture the new baseline;
2. inspect changes affecting assumptions;
3. update path maps/contracts;
4. record an ADR only for material design deviation;
5. continue with current code as source of truth while retaining programme
   outcomes/security invariants.

Do not reset or overwrite newer repository work to match this kit.

## 7. Blocked work

A milestone may be `blocked` only by a concrete missing environment, external
decision or failed invariant. The handoff records:

- exact blocker;
- work completed and tested;
- command/environment needed;
- first remaining action;
- paths and risks.

"Large task" is not a blocker.

## 8. Definition of programme done

M09 completion requires all prior gates, current generated artefacts, plugin
parity, security/UX reviews and platform-specific native qualification. Partial
platform evidence must remain a stated limitation rather than an implied pass.
