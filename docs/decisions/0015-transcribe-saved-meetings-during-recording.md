# ADR 0015: Transcribe Saved Meetings During Recording

- Status: Accepted
- Date: 2026-11-18
- Decision owners: Nota desktop maintainers

## Context

The ASR worker previously waited whenever the recording controller was active,
even if the requested transcript belonged to a different, already saved
meeting. Back-to-back meetings could therefore delay transcription for the
entire next recording. Batch tasks also began with an upload phase before
dispatch, so the UI presented this wait as a stalled upload.

The recorder and ASR worker already operate on separate files. Recording
finalization commits the Ogg before indexing it, and manual transcription
selects an indexed recording. A global capture-state gate does not establish
additional ownership of that saved file.

## Decision

The ASR manager must not depend on recording-controller activity. Explicit
transcription requests and enabled automatic transcription of saved files may
proceed during another recording. This applies to FunASR, DashScope,
OpenAI-compatible chunks, and resumed generations.

Keep the single ASR worker, per-recording duplicate rejection, cancellation,
Provider checkpoints, immutable generation settings, and independent capture
scope. Do not expose active PCM or partial files to the ASR worker.

Local enqueue and resume use queued status with preparing phase. The UI
distinguishes that local wait from Provider upload and remote queue progress.
Resuming preserves checkpoint data and completed chunks.

## Alternatives Considered

- Keep the global wait and explain it in the UI: this still prevents the
  back-to-back meeting workflow despite the input already being saved.
- Permit only cloud Providers during recording: this leaves inconsistent
  behavior across Providers and does not address legacy resume and chunk
  scheduling.
- Add multiple ASR workers: unnecessary for independence from audio capture;
  it would change resource usage, ordering, and concurrency beyond this fix.

## Consequences

Capture and ASR can share CPU, disk, and network resources. ASR stays on one
background worker with existing bounded legacy chunks; this decision adds no
new parallel ASR workers or automatic network actions. Future scheduling
changes should address observed contention without silently blocking saved
meetings for the duration of another recording.

This reverses the former global recording wait in the architecture
specification. It does not change the server HTTP contract or storage schema.
