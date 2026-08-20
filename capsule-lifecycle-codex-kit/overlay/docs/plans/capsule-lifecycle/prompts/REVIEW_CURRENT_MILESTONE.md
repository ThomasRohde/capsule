# Independent review of the current Capsule lifecycle milestone

Act as an independent, sceptical reviewer. Do not implement new scope until the
review is complete.

Read root policy, the programme architecture/security model, current milestone
plan and acceptance gate, its result, and the complete diff from the milestone
start commit.

Review at five levels:

1. **Contract correctness** — Does the implementation satisfy every acceptance
   clause without redefining it?
2. **Architecture** — Are generic/application boundaries, format versioning,
   signed/mutable compartments and trusted/raw window boundaries intact?
3. **Security** — Can an untrusted capsule exploit parsing, metadata, plans,
   output paths, comparison, migration, or Tauri command exposure?
4. **Failure semantics** — Are stale plans, races, crashes, limits and verification
   failures create-new and fail-closed?
5. **Evidence** — Are tests relevant, independently reproducible and free from
   unsupported claims?

Use repository tools and tests to verify findings. Categorise findings as
critical/high/medium/low. Give each a concrete reproduction or code reference.
Do not mark a clause passed merely because code exists.

Write the review to the milestone evidence directory and update `RESULT.md` with
findings and resolutions. Critical or high unresolved findings block completion.
