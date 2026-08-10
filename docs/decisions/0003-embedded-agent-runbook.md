# ADR 0003: Launch instructions travel inside the database

- Status: Accepted for bootstrap
- Date: 2026-08-07

## Context

External READMEs and setup scripts drift from the exact artefact version. A coding agent should be able to receive a capsule and discover how to inspect and run it.

## Decision

Store ordered runbooks, command templates, prompts, checks, and an obvious `START_HERE` view in every capsule. Embed a compatible standalone host as a fallback asset for agent-assisted extraction.

## Consequences

- A one-sentence prompt is sufficient for a capable local agent.
- Instructions are versioned with the application.
- Embedded instructions are an explicit prompt-injection and supply-chain surface; they are inspected as untrusted data until the file is trusted.
- Installed trusted hosts remain preferable to embedded executable code.
