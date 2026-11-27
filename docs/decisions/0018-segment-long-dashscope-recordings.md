# ADR 0018: Segment DashScope Recordings Longer Than Two Hours

- Status: Accepted
- Date: 2026-11-27
- Last updated: 2026-11-27
- Decision owners: Nota maintainers

## Context

ADR 0010 always enables speaker diarization for DashScope file transcription
and rejects recordings longer than two hours, the reliable diarization limit,
before network access. It rejected chunked transcription because independent
chunks lose meeting-wide speaker scope.

Meetings now regularly run longer than two hours, so those recordings cannot
use DashScope at all. The user names speakers manually after transcription
anyway: DashScope generations never use Nota voiceprints, and meeting-local
names are keyed by raw speaker label without a uniqueness constraint. Giving
two labels the same name already merges them in transcripts and AI documents.

## Decision

Recordings longer than two hours use a separate, snapshotted protocol,
`dashscope_filetrans_segmented_v1`. Nota splits the original Ogg Opus file
locally, at page boundaries inside pauses and without re-encoding, into
`ceil(duration / 105 min)` roughly equal segments of at most 115 minutes. The
segments are uploaded and transcribed one after another as independent tasks
and merged into one transcript.

Speaker labels are not aligned across segments. Each segment's labels are
renumbered after the previous segment's, and the user merges a person's labels
by naming them the same. Segments do not overlap.

Each completed segment result is stored durably before the next segment
starts. The duplicate-billing rules of ADR 0010 apply per segment: a saved
task is never resubmitted, and an ambiguous submission fails the generation
instead of retrying. Recordings of at most two hours keep the existing
single-file protocol unchanged.

This ADR supersedes ADR 0010's two-hour rejection and its rejection of
chunked transcription.

## Alternatives Considered

### Align speakers across segments with overlapping audio

Rejected for now. Matching labels inside an overlap window bills the overlap
twice, adds an alignment algorithm whose mistakes are hard to see, and still
cannot match a person who does not speak in the overlap. Manual naming is
needed for those people anyway.

### Disable diarization for long recordings

Rejected because the resulting transcript has no speakers, which breaks speaker
management and speaker-based AI templates. It also depends on an unverified
non-diarized duration limit.

### Submit all segments in parallel

Deferred. Parallel tasks shorten the wait but multiply in-flight remote state,
cancellation cases, and ambiguous-submission cases. Sequential processing
keeps at most one active remote task per generation, as today.

### Split at fixed two-hour boundaries

Rejected because a short final segment, such as one minute after a two-hour
cut, diarizes poorly, and a fixed cut can split a sentence.

## Consequences

- Long meetings can use DashScope with speaker labels.
- Total billed duration equals the recording duration; no audio is sent twice.
- The user may need to name more labels, for example nine labels for three
  people across three segments.
- The desktop gains Ogg page remuxing, a segment planner, checkpoint version 2,
  per-segment result storage in `transcription_chunks`, and two nullable
  progress fields in `TranscriptionSummary`.
- An ambiguous submission on a later segment forces a fresh generation that
  transcribes, and may bill, every segment again.
- Total wall-clock time grows with the number of segments.

## Compatibility and Evolution

The single-file protocol and checkpoint version 1 are untouched, so in-flight
and completed generations keep working. Segmented generations are recognizable
by their protocol value. Cross-segment alignment or parallel submission can be
added later as a new protocol version without changing stored segmented
transcripts.
