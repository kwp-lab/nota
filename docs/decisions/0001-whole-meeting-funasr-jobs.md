# ADR 0001: Use Durable Whole-Meeting Jobs for FunASR

- Status: Accepted
- Date: 2026-07-30
- Last updated: 2026-07-31
- Decision owners: Nota desktop and Nota ASR Server maintainers

## Context

Nota originally made long transcription resumable by decoding the recording
locally and sending independent ten-minute WAV requests. That is suitable for a
generic OpenAI-compatible endpoint, but each request creates an independent
speaker-diarization scope.

For a meeting that crosses multiple chunks, the same participant can therefore
receive different speaker labels in different chunks. Client-side text and
timestamp merging cannot reliably repair that identity mismatch.

Uploading an entire multi-hour recording as one synchronous request would
restore one speaker scope, but would lose durable upload progress, cancellation,
server restart recovery, and bounded processing memory.

## Decision

FunASR providers use Nota batch protocol version 1:

- the client uploads the original Ogg recording through resumable byte ranges;
- the server persists task state and processing-window checkpoints;
- server inference uses bounded internal windows;
- speaker centroids from every completed window are clustered once at final
  meeting scope;
- the client commits the final existing transcript schema locally before
  acknowledging remote deletion.

The client requires capability negotiation before starting a FunASR
transcription. An old server is rejected rather than silently using independent
requests.

OpenAI-compatible providers retain the existing local WAV chunk protocol
because their generic API does not promise Nota-specific durable jobs or
meeting-wide speaker reconciliation.

## Alternatives Considered

### Keep independent FunASR chunks

Rejected because resumability would be preserved at the cost of incorrect
meeting-wide speaker semantics.

### Send one synchronous full recording

Rejected because multi-hour requests are difficult to resume, cancel, recover
after restart, and process with bounded resources.

### Reconcile only local speaker labels in the client

Rejected because anonymous labels do not contain enough information to decide
whether speakers from different requests are the same person. Speaker
embeddings remain a server-side model concern.

### Build realtime streaming now

Rejected for this phase. Realtime sessions require a separate protocol for
partial results, sequence numbers, reconnects, backpressure, and evolving
speaker state.

## Consequences

Positive:

- one final meeting has one anonymous speaker-label scope;
- upload and inference progress survive client and server interruptions;
- the server can keep audio memory bounded by its processing window;
- the existing final transcript schema remains stable;
- the generic OpenAI-compatible workflow remains available.

Costs:

- FunASR now requires a compatible Nota ASR Server;
- client and server share a versioned private protocol;
- both sides persist additional task identity and progress state;
- remote data requires explicit acknowledgement and retention cleanup;
- cancellation is cooperative at server window boundaries.

## Compatibility and Evolution

Batch protocol version 1 semantics must not change silently. An incompatible
protocol requires a new advertised version and a corresponding client decision.

Fun-ASR-Nano and OpenVINO are not part of this decision. They may be introduced
behind the same server-side window adapter and final response contract after
separate model and deployment evaluation.
