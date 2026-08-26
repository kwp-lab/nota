# ADR 0012: Keep Cross-Provider Hotword Lists Local and Snapshot Them per Generation

- Status: Accepted
- Date: 2026-08-25
- Last updated: 2026-08-26
- Decision owners: Nota desktop and Nota ASR Server maintainers

## Context

Users need different terminology for different meetings, while ASR Providers
expose incompatible hotword mechanisms. Cloud-owned vocabulary resources would
couple the UI and lifecycle to one vendor and would not preserve reproducible
historical settings.

## Decision

The Client SQLite database is the sole source for reusable lists. A new
transcription generation copies zero or one normalized list into immutable
snapshot columns. Adapters translate that snapshot only after declaring model
capability. Unsupported combinations are rejected before upload. Full
snapshots remain backend-only.

The text-area syntax may include a final `:weight`, but Rust parses and
normalizes it before storage. SQLite stores text and nullable weight as separate
fields, and generation snapshots use a versioned structured representation.
Adapters never parse display syntax: DashScope consumes supported weights,
while Nota Server receives only entry text because its current models do not
expose equivalent per-entry weight semantics. Provider capability metadata
drives disclosure of applied, defaulted, or ignored weights.

## Alternatives Considered

- Cloud precompiled vocabulary tables were rejected because they add vendor
  synchronization, credentials, and orphan cleanup.
- Passing a mutable list by reference was rejected because resume and history
  would change after edits.
- Silently dropping unsupported entries or weights was rejected because it misrepresents
  recognition behavior and may create an unwanted paid task.

## Consequences

Adapters validate first against local generic limits and then against the
selected Provider/model. The Server batch protocol persists the optional list
with idempotent jobs. Snapshots consume some SQLite space but make recovery and
historical attribution stable.

## Compatibility and Evolution

The batch field is optional. Old clients work with a new Server; new clients
use an old Server only without hotwords. Version 2 snapshots add optional
weights while preserving legacy string snapshots. Arbitrary context
enhancement, languages, multi-list merge, import/export, and cloud vocabulary
ids remain separate future capabilities.
