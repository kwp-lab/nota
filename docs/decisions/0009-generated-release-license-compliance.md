# ADR 0009: Generate release license compliance from locked dependencies

- Status: Accepted
- Date: 2026-08-13
- Last updated: 2026-08-13
- Decision owners: Nota maintainers

## Context

Nota distributes a native Rust executable, bundled web assets, statically
compiled libopus code, and MPL-2.0 media crates. A hand-maintained package list
cannot reliably preserve license texts, detect dependency drift, or prove which
source corresponds to a binary release.

## Decision

Use pinned local open-source generators against `Cargo.lock` and
`package-lock.json`, enforce an allowlist with exact reviewed exceptions, and
commit deterministic notices, source mapping, and a CycloneDX SBOM. Every
release package carries those files. The release workflow additionally builds
an exact source archive for all locked MPL-2.0 Rust crates and verifies the
vendored libopus license with a pinned checksum.

## Alternatives Considered

- **Maintain only a Markdown dependency table.** Rejected because it omits
  full notice text and silently drifts.
- **Remove all reciprocal dependencies.** Rejected because MPL-2.0 is
  compatible with Nota's distribution model when file-level source obligations
  are met.
- **Commit every third-party source tree.** Rejected because it creates large,
  duplicated, easily stale repository content.
- **Use a hosted compliance service.** Rejected because deterministic local and
  CI tooling is sufficient and avoids a new external dependency.

## Consequences

Dependency changes now require regeneration and review. Release jobs install a
pinned compliance tool and publish additional legal assets. MPL source archives
increase release size, but create a direct version-to-source link. Manual native
components remain exceptional and checksum guarded.

## Compatibility and Evolution

Tool versions and policy may be upgraded in a normal reviewed change. A new
license family or manually embedded component requires an explicit policy entry
and documentation update; it must not be hidden behind a permissive wildcard.
