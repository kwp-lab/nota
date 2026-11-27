# ADR 0017: Independent AI Documents per Template and No AI Revision

- Status: Accepted
- Date: 2026-11-27
- Last updated: 2026-11-27
- Decision owners: Nota desktop maintainers

## Context

Nota originally allowed at most one AI document for each recording and
template pair, enforced by `UNIQUE(recording_id, template_id)` in
`ai_documents`, a backend check in `create` mode, and a generation dialog that
inferred `create` or `regenerate` from the selected template. Picking a
template the meeting had already used always appended a version to the
existing document. Users who wanted a second, independent summary (for another
audience, say) had to clone the template first, which was hard to discover and
mixed two different concepts: a template describes how to generate, while a
document is a separate result with its own title, requirements, and history.

## Decision

A recording may hold any number of AI documents, and several may come from
the same template. The template is a property of a document, not its
identity. The toolbar has two separate entry points: "new AI document"
(`create`) and "create new version" (`regenerate`) of the selected document.
The selected template never selects the mode. Versions remain scoped to one
document and append-only as described in ADR 0005.

The in-app AI revision action (`revise`) is removed. It overlapped with
"create new version", since both appended a version to the same document,
and its only difference, feeding the previous version's body back to the
model, is not worth a third generation concept. A new version is always
generated from the current transcript and context. The `revise` value remains
readable for historical versions, and new requests with it are rejected.

Existing databases are migrated at startup by rebuilding `ai_documents`
without the unique constraint, preserving every document and version identity.

## Alternatives Considered

### Keep one document per template and promote template cloning

Rejected because cloning a template to obtain a second document is an
indirect workaround, multiplies near-identical templates, and still lets the
dialog silently turn a "new document" intent into a new version.

### Keep AI revision next to "create new version"

Rejected because two actions that both append a version to the same document
were hard to tell apart, and revision added a required free-text field,
source-version validation, and a prompt variant that carried previous AI
errors forward. Manual edits to the Markdown file remain the way to refine a
version in place.

### Keep the constraint but add a per-document variant key

Rejected because it adds a second identity concept the user never sees and
still requires a schema migration.

## Consequences

- Each toolbar entry point has exactly one outcome; neither dialog switches
  between creating a document and appending a version.
- Document titles may repeat across documents of the same template; the
  dialog suggests a de-duplicated default title, but titles are not unique.
- Lookups by `(recording_id, template_id)` are no longer meaningful and were
  removed.
- The generation IPC request no longer carries a source version ID, and
  generated prompts never include an earlier version's Markdown.
- Historical `revise` versions keep their mode and parent link for display.
- Markdown filenames already include a short document identifier, so sibling
  documents from one template cannot collide on disk.

## Compatibility and Evolution

The migration only removes a constraint and adds an index, so an older build
can still open a migrated database and list every document. Its dialog would
treat the first same-template document as the one to append versions to.
Reintroducing uniqueness would require a merge or rename strategy for existing
duplicate documents and a superseding ADR.
