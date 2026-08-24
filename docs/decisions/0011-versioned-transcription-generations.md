# ADR 0011: Preserve Versioned Transcription Generations

- Status: Accepted
- Date: 2026-08-22
- Last updated: 2026-08-22
- Decision owners: Nota maintainers

## Context

Nota originally kept one `transcriptions` row for each recording. A new
transcription incremented its generation number but eventually replaced the
stored text and segments. The Provider snapshot could explain the newest
result, but users could not compare a FunASR result with a later DashScope
result or return to a preferred older transcript.

Provider capability differences make replacement especially misleading.
Speaker assignments, voiceprint availability, speaker-count settings, exports,
and AI document provenance all need to remain attached to the exact result
that supplied them.

## Decision

`transcriptions` uses `(recording_id, generation)` as its primary key and keeps
every transcription attempt until the recording is deleted. A new attempt
inserts a row and never updates an older generation.

`recordings.current_transcription_generation` selects one completed generation
as the recording's product-level transcript. A successful result commit writes
the generation and advances this pointer in one SQLite transaction. The user
may select any older completed generation; selection changes only the pointer.

Read, copy, export, speaker management, voiceprint capability checks, and new
AI document generation resolve through the current pointer. ASR execution,
cancellation, recovery, checkpoints, and status events always carry an
explicit generation so changing the current completed version cannot redirect
an active cloud task.

The main selector shows completed generations only. Failed, cancelled, and
interrupted rows remain available to the recovery path but are not presented
as completed transcript versions. Nota does not yet expose deletion of an
individual generation.

## Alternatives Considered

- Keep only the latest result and retain Provider metadata. This cannot compare
  transcript quality and leaves historical AI provenance without a readable
  source transcript.
- Copy old results into a separate archive table. This creates two transcript
  schemas and two read paths with avoidable migration and consistency risk.
- Treat the UI selection as temporary frontend state. Copy, export, speaker
  management, and AI generation could then use a different generation from the
  one displayed after navigation or restart.

## Consequences

- Retranscription consumes additional local SQLite space, dominated by text
  and segment JSON rather than audio.
- Generation-scoped speaker assignments and voiceprint capabilities remain
  correct when users switch versions.
- Provider configuration may be deleted without destroying historical display
  metadata; resuming an unfinished generation still requires its credentials.
- Every backend operation must distinguish current product selection from the
  explicit generation of an active ASR job.
- Recording deletion cascades every transcript generation. Individual-version
  retention controls require a future decision because voiceprints and AI
  versions may reference a generation.

## Compatibility and Evolution

Existing databases are rebuilt locally from the former recording-keyed table
to the composite key. Their sole row becomes the selected generation, preferring
the newest completed row when available. Existing protocol and Provider-kind
backfills run before the history migration.

Future comparison, diff, pinning, or per-version deletion interfaces may build
on this model without changing stored generation identities. They must preserve
AI document provenance and define dependent voiceprint behavior explicitly.
