# Lifecycle operation test matrix — <operation>

## Fixture identities

| Role | Path | File SHA-256 | Capsule ID | Revision ID | App digest | Data schema |
| --- | --- | --- | --- | --- | --- | --- |
| source | … | … | … | … | … | … |
| target/release/base | … | … | … | … | … | … |

## Functional cases

| Case | Plan decision | Expected output identity/data | Expected code | Result/evidence |
| --- | --- | --- | --- | --- |
| happy path | … | … | success | … |
| sensitive default | … | … | … | … |
| stale source | … | no output | `stale_plan` | … |
| destination exists | … | no overwrite | `destination_exists` | … |
| limit exceeded | … | no output | `limit_exceeded` | … |
| validation fails | … | no published output | `verification_failed` | … |

## Immutability

Record SHA-256 and relevant filesystem identity before and after each test for
every input. A changed source is a test failure.

## Crash stages

| Stage | Injection | Expected durable state | Actual |
| --- | --- | --- | --- |
| private temp created | kill | input intact, no published output | … |
| rows written | kill | input intact, private temp recoverable/removable | … |
| validation | kill | input intact, no accepted output | … |
| before publish | kill | destination absent | … |
| after publish | kill | destination complete and reopen-verifiable | … |
