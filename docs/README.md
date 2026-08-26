# Nota Engineering Documentation

- Status: Accepted
- Last updated: 2026-08-25
- Owners: Nota maintainers

This directory is the durable engineering context for Nota. It explains the
runtime boundaries, invariants, recovery behavior, and architectural decisions
that are intentionally too detailed for the project README.

Contributors should start with the repository-level
[`CONTRIBUTING.md`](../CONTRIBUTING.md) for workflow and privacy rules, then
use this index to find the specification that owns the behavior being changed.

## Reading Order

1. [`architecture.md`](architecture.md) — component ownership, trust
   boundaries, concurrency, and system-wide invariants.
2. [`asr-integration.md`](asr-integration.md) — FunASR whole-meeting jobs,
   OpenAI-compatible legacy chunks, state mapping, and recovery rules.
3. [`audio-import.md`](audio-import.md) — local phone-recording import,
   normalization, task state, duplicate handling, and crash cleanup.
4. [`dashscope-asr-provider.md`](dashscope-asr-provider.md) — direct cloud
   file transcription, durable task recovery, provider capabilities, and the
   checklist for adding another third-party ASR provider.
5. [`hotword-library.md`](hotword-library.md) — local reusable lists,
   generation snapshots, Provider validation, and privacy boundaries.
6. [`hotword-provider-ab-report.md`](hotword-provider-ab-report.md) — manual
   cross-Provider A/B results for real hotword transcription effects.
7. [`ai-documents.md`](ai-documents.md) — Markdown-first AI generation,
   templates, versions, providers, and relinking.
8. [`design-system.md`](design-system.md) — product visual language, design
   tokens, typography, color, spacing, and component styling rules.
9. [`diagnostic-logging.md`](diagnostic-logging.md) — local event format,
   privacy boundary, correlation, and rotation.
10. [`data-lifecycle.md`](data-lifecycle.md) — local and remote persistence,
   migrations, retention, and deletion ordering.
11. [`testing.md`](testing.md) — automated checks and hardware acceptance
   scenarios.
12. [`decisions/README.md`](decisions/README.md) — architectural decision
   index, lifecycle, and template.
13. [`speaker-identification.md`](speaker-identification.md) — local
   voiceprint enrollment, matching, confirmation, and lifecycle rules.
14. [`releasing.md`](releasing.md) — version, tag, package, and release process.
15. [`open-source-compliance.md`](open-source-compliance.md) — dependency
   license policy, generated notices, SBOM, and source-delivery rules.

## Sources of Truth

Documentation has different kinds of authority. Keep them distinct:

1. Rust and TypeScript types, SQLite migrations, HTTP schemas, and tests are
   the executable source of truth.
2. Specifications in this directory define intended semantics, invariants,
   lifecycle ordering, and compatibility guarantees.
3. ADRs record why an architectural choice was made and which alternatives
   were rejected.
4. The root README is the product and contributor entry point.
5. `CHANGELOG.md` records user-visible changes by release.

Do not maintain a second copy of every HTTP field in the client repository.
The Nota ASR Server OpenAPI schemas and its `docs/api-contract.md` remain
canonical for `/v1/nota`. This repository documents how the desktop client
uses that contract and what it promises locally.

## Documentation Change Matrix

| Change | Required documentation |
|---|---|
| User-visible behavior | Root README when material, plus `CHANGELOG.md` |
| Component boundary or invariant | `architecture.md` |
| ASR request, state, retry, or compatibility behavior | `asr-integration.md` |
| SQLite fields, migration, retention, or deletion ordering | `data-lifecycle.md` |
| Audio import formats, normalization, task state, or recovery | `audio-import.md` |
| New test requirement or supported environment | `testing.md` |
| Hotword storage, selection, or Provider mapping | `hotword-library.md` |
| Diagnostic event, field, privacy, or rotation behavior | `diagnostic-logging.md` |
| Visual token, shared component styling, or durable interaction treatment | `design-system.md` |
| Long-lived architectural choice or reversed decision | New ADR |
| Versioning or packaging workflow | `releasing.md` |
| Product-owned policy or compliance workflow | New ADR plus affected specification |
| Complex business flow or non-trivial state transition | Mermaid diagram in the owning specification |

## Writing Rules

- Prefer explicit state names, field names, and invariants over general prose.
- Use **must**, **must not**, and **may** only for behavior that can be tested.
- Link to the owning code and tests instead of copying implementation details
  that change frequently.
- Mark planned behavior as draft; do not write future work as if it already
  exists.
- Update code, tests, documentation, and ADRs in the same change when their
  shared behavior changes.
- Never place API keys, authorization headers, audio, transcripts, or local
  user paths in documentation examples or test output.
