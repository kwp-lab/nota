# ADR 0002: Keep the Recording Core Policy-Neutral

- Status: Accepted
- Date: 2026-08-02
- Last updated: 2026-08-02
- Decision owners: Nota maintainers

## Context

Nota previously displayed a one-time participant-notification prompt before
recording. It also copied a notification template into a per-session
`consents` table. The local rows had no participant identity, signature,
tamper protection, export workflow, retention policy, or durable relationship
to finalized recordings. They therefore added product and storage complexity
without constituting a reliable compliance record.

Notification and consent requirements vary by jurisdiction, organization, and
deployment. A generic open-source recording core cannot accurately encode all
of those policies.

## Decision

The upstream Nota application will not display a participant-notification or
consent-acknowledgement prompt, require an acknowledgement field before
recording, or persist acknowledgement records or templates.

The current schema does not create the legacy `consents` table or its related
setting rows. The application does not access or migrate those objects if they
remain in a database created by an older build. Recording remains an explicit
user action; automatic recording remains unsupported.

Downstream distributors may add a policy-specific workflow when required, but
that workflow is outside the upstream recording contract and must define its
own user experience, storage semantics, retention policy, and tests.

## Alternatives Considered

- **Keep the existing one-time prompt.** Rejected because it represents a
  single policy assumption and the acknowledgement is not meaningful evidence.
- **Keep only the per-session ledger.** Rejected because invisible, mutable
  local rows create privacy and maintenance cost without a supported user
  workflow.
- **Add compatibility cleanup for legacy databases.** Rejected to keep the
  current code and schema independent of a removed feature. Any old rows remain
  inert and are outside the supported data model.

## Consequences

- Starting a recording has no notification modal or acknowledgement gate.
- The Tauri request and settings contracts are smaller.
- Existing notification records and settings may remain in an old database,
  but Nota neither reads nor changes them.
- The upstream project makes no promise that a participant-notification policy
  has been executed or audited.
- Integrators with additional policy requirements carry the implementation and
  documentation responsibility.

## Compatibility and Evolution

No forward-compatibility migration is provided for the removed feature. New
installs contain no notification schema, while databases from older builds may
retain unused objects without affecting recording startup.

Reintroducing an upstream workflow requires a new ADR. It must not reuse the
legacy `consents` design without defining evidence quality, lifecycle,
retention, and migration behavior.
