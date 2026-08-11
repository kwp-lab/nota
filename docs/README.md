# Nota Engineering Documentation

- Status: Accepted
- Last updated: 2026-08-11
- Owners: Nota maintainers

This directory is the durable engineering context for Nota. It explains the
runtime boundaries, invariants, recovery behavior, and architectural decisions
that are intentionally too detailed for the project README.

## Reading Order

1. [`architecture.md`](architecture.md) — component ownership, trust
   boundaries, concurrency, and system-wide invariants.
2. [`asr-integration.md`](asr-integration.md) — FunASR whole-meeting jobs,
   OpenAI-compatible legacy chunks, state mapping, and recovery rules.
3. [`ai-documents.md`](ai-documents.md) — Markdown-first AI generation,
   templates, versions, providers, and relinking.
4. [`design-system.md`](design-system.md) — product visual language, design
   tokens, typography, color, spacing, and component styling rules.
5. [`data-lifecycle.md`](data-lifecycle.md) — local and remote persistence,
   migrations, retention, and deletion ordering.
6. [`testing.md`](testing.md) — automated checks and hardware acceptance
   scenarios.
7. [`decisions/README.md`](decisions/README.md) — architectural decision
   index, lifecycle, and template.
8. [`speaker-identification.md`](speaker-identification.md) — local
   voiceprint enrollment, matching, confirmation, and lifecycle rules.
9. [`releasing.md`](releasing.md) — version, tag, package, and release process.

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
| New test requirement or supported environment | `testing.md` |
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
