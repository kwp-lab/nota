# Architecture Decision Records

- Status: Accepted
- Last updated: 2026-08-09
- Owners: Nota maintainers

ADRs record consequential technical choices that future maintainers should not
have to reconstruct from a code diff. They explain context, the chosen option,
rejected alternatives, and consequences.

## Index

| ADR | Status | Decision |
|---|---|---|
| [0001](0001-whole-meeting-funasr-jobs.md) | Accepted | Use durable whole-meeting jobs for FunASR |
| [0002](0002-keep-recording-core-policy-neutral.md) | Accepted | Keep participant-notification policy out of the core application |
| [0003](0003-local-speaker-identification.md) | Accepted | Keep voiceprints and participant identity in the local Rust backend |
| [0004](0004-clean-voiceprint-sample-selection.md) | Accepted | Enroll voiceprints only from server-filtered clean speaker ranges |
| [0005](0005-markdown-first-ai-meeting-documents.md) | Accepted | Keep generated Markdown authoritative and append a file for every successful AI version |

## Lifecycle

- Use the next four-digit number and a short kebab-case filename.
- Start with `Proposed` when a decision is still under review.
- Change it to `Accepted` only when implementation is authorized.
- Do not rewrite the reasoning of an accepted ADR to make history look
  cleaner.
- When a decision changes, add a new ADR and mark the old one `Superseded by
  ADR NNNN`.
- Small implementation details that do not constrain future architecture
  belong in specifications or code comments, not ADRs.

## Template

```markdown
# ADR NNNN: Decision Title

- Status: Proposed
- Date: YYYY-MM-DD
- Last updated: YYYY-MM-DD
- Decision owners: Team or maintainers

## Context

What problem, constraints, and forces require a decision?

## Decision

What will the project do? State the durable rule explicitly.

## Alternatives Considered

Which credible alternatives were rejected, and why?

## Consequences

What becomes easier, harder, required, or intentionally unsupported?

## Compatibility and Evolution

How can this decision change without silently breaking existing behavior?
```
