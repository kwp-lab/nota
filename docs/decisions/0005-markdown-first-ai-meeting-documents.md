# ADR 0005: Markdown-First AI Meeting Documents

- Status: Accepted
- Date: 2026-08-09
- Last updated: 2026-08-09
- Decision owners: Nota desktop maintainers

## Context

Meeting summaries and action lists are meant to leave the application and be
shared with a team. Nota is a single-user desktop application, so external
renames, moves, and edits are expected user behavior rather than collaborative
write conflicts. At the same time, users need repeatable generation, visible
history, provider and prompt snapshots, and a way to detect a missing file.

Storing only generated bodies in SQLite would make ordinary sharing and editing
an export workflow. Storing both a database body and an editable file would
create two competing sources of truth.

## Decision

Generated Markdown files are the authoritative AI document content. SQLite is
the local index and immutable generation ledger.

Every successful create, regenerate, or revise operation writes a new Markdown
file and appends a version row. Nota never overwrites an earlier version. A
revision reads the selected file's current disk content, including intentional
external edits, while regeneration starts from the current transcript and
context without feeding a previous model output back into the model.

Each generated file carries small Nota-owned YAML identity metadata. Missing or
moved files are reported explicitly and may be relinked only when those
identities match. Recording deletion preserves Markdown by default and offers a
separate opt-in to remove currently linked files.

## Alternatives Considered

### Store generated content only in SQLite

Rejected because sharing would always require export, common editors could not
work on the primary document, and the database would become the owner of content
whose main purpose is distribution.

### Store identical authoritative copies in SQLite and Markdown

Rejected because an external edit immediately makes one copy stale and requires
conflict resolution that provides little value in a single-user local client.

### Overwrite one Markdown file on regeneration

Rejected because it destroys a useful previous result, makes comparison and
rollback impossible, and creates avoidable ambiguity when the user has edited
the file externally.

### Put all scenarios and attempts in one flat version list

Rejected because scenario selection and generation history are different axes.
The UI therefore selects an AI document/template first and a version second.

## Consequences

- Markdown is immediately shareable and editable with ordinary tools.
- SQLite can show provider, template, context, lineage, and status history
  without duplicating document bodies.
- File availability and external modification must be checked at read time.
- Atomic create-new writes and collision-resistant names are required.
- Relinking requires stable embedded identity metadata.
- Database backup alone is not a complete backup of AI document content.
- A future cloud-sync feature must treat Markdown files as user documents, not
  reconstruct them silently from SQLite.

## Compatibility and Evolution

New metadata fields may be added under the `nota` YAML object, but existing
identity keys must remain readable. If a future feature introduces in-app
editing or synchronization, it must either preserve Markdown as the authority
or supersede this ADR explicitly with a migration and conflict model.
