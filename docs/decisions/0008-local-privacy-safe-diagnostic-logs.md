# ADR 0008: Keep Diagnostic Logs Local and Privacy-Safe

- Status: Accepted
- Date: 2026-08-12
- Last updated: 2026-08-12
- Decision owners: Nota desktop maintainers

## Context

Intermittent Windows audio, process-window, ASR, and LLM failures are difficult
to reconstruct from a user report alone. Nota already had a rolling `nota.log`,
but free-form messages were inconsistent and could accidentally include local
paths or unconstrained dependency errors. The existing SQLite schema also
contains an unused `events` table, but persisting diagnostics there would add
migration, retention, query, and privacy obligations without improving the
current support workflow.

Meeting recordings, transcripts, generated documents, credentials, and local
paths are private. A useful diagnostic facility must therefore make unsafe
data difficult to log by construction and must not create telemetry.

## Decision

Nota will keep a bounded, local, rolling UTF-8 text log. Diagnostic records use
a single-line UTC format and a typed field allowlist. The allowlist permits
opaque identifiers, state enums, counters, timings, HTTP status codes, token
usage, and static error codes. It excludes content, credentials, URLs, paths,
request/response bodies, titles, and raw Provider or dependency errors.

The application will retain one 10 MiB active file and two archives. The
Settings UI may open the directory through a Rust command. Nota will not use
SQLite for diagnostics, activate the legacy `events` table, show an in-app log
viewer, export a bundle, or upload logs automatically.

## Alternatives Considered

- **SQLite events:** rejected because diagnostics are append-only support data,
  while database storage would require migrations, cleanup semantics, and a
  query UI. It also risks joining technical events with private product data.
- **Windows Event Log:** rejected because registration and permissions make
  portable and NSIS behavior less predictable, and ordinary users have a
  harder time locating the result.
- **A full tracing/telemetry backend:** rejected because the current need is
  local incident reconstruction. Remote collection would expand the privacy,
  consent, security, and operational scope.
- **Unstructured logger messages:** rejected because free-form interpolation
  cannot reliably prevent paths, Provider bodies, or multiline content.

## Consequences

- Logs remain easy to inspect with ordinary text tools and have a fixed size.
- Correlation IDs can connect recording, ASR, import, and AI state transitions.
- Contributors must add durable field roles to the typed allowlist and test
  them before use.
- Detailed content-level debugging cannot rely on the technical log; explicit
  product-owned views such as AI generation details remain separate.
- Users must deliberately share a log file if they want support to inspect it.

## Compatibility and Evolution

The existing filename and rotation capacity remain compatible. Event names and
fields may be added, but privacy exclusions are invariant. A future viewer,
diagnostic export, or upload workflow requires a new decision covering user
consent, redaction, retention, and failure behavior.
