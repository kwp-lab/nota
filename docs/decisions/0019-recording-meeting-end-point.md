# ADR 0019: Apply a Recording-Level Meeting End Point When Reading Transcripts

- Status: Accepted
- Date: 2026-11-28
- Last updated: 2026-11-28
- Decision owners: Nota maintainers

## Context

Recordings sometimes run past the end of the meeting. The tail's transcript
then reaches the transcript view, copy and export, speaker management, and AI
documents. Users need to exclude it without losing data, and the exclusion
should survive retranscription.

Every transcript consumer already reads through `Storage::transcript` or
`Storage::transcript_for_generation`.

## Decision

Store an optional `meeting_end_ms` on the recording. When a transcript is read,
sentences that start at or after the end point are excluded and the plain text
is rebuilt from the kept sentences. Stored transcripts, speaker mappings, and
audio are never modified, so clearing the end point restores the full
transcript.

## Alternatives Considered

### Save a trimmed copy as a new transcription generation

Rejected because it duplicates provider, hotword, and speaker-mapping
snapshots, introduces a derived-generation concept into version history, and
must be repeated after every retranscription.

### Trim the recording audio

Rejected as the primary mechanism because it is irreversible and treats the
recording file, a data-integrity path, as editable. Transcribing only the audio
before the end point may be added later without changing this decision.

### Filter only in the AI generation path

Rejected because copy, export, and speaker management would still include the
unrelated tail, and each consumer would need its own rule.

## Consequences

- One filter covers every consumer, and the behavior is reversible.
- The end point applies to every generation of the recording, including later
  ones.
- Transcripts without sentence timestamps cannot be trimmed.
- Plain text of a trimmed transcript is rebuilt from sentences and may differ
  in whitespace from the provider's original full text.
- Readers that must see the full stored transcript, such as raw-label
  validation, read stored sentences directly.

## Compatibility and Evolution

The column is nullable, so existing recordings behave as before. A meeting
start point or excluded ranges could later extend this into a list of kept
ranges with the same read-time rule.
