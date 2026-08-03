# ADR 0003: Keep Speaker Identity and Voiceprints Local

- Status: Accepted
- Date: 2026-08-02
- Last updated: 2026-08-02
- Decision owners: Nota maintainers

## Context

ASR diarization produces anonymous labels whose scope is one meeting. Users
want to assign real participant names, reuse those assignments in later
meetings, and manage saved voiceprints. Names and voice embeddings are
biometric personal data, while the configured ASR Server may be shared or
remotely operated.

Automatic identity replacement during normal transcription would also make a
probabilistic match look authoritative and would couple a reliable existing
workflow to a new optional feature.

## Decision

Speaker identification is a separate, explicit post-transcription action.
The ASR Server exposes a stateless CAM++ embedding-extraction capability. It
does not receive participant names or persist a people registry.

The Rust backend stores participant names and embeddings, performs compatible
cosine matching, holds extraction results in an opaque in-memory session, and
persists only user-confirmed mappings. React receives candidate metadata and
suggestions, not raw audio samples or embedding vectors.

Raw `speaker_N` values remain immutable transcription evidence. Confirmed
assignments are stored separately by recording and transcription generation,
and all user-facing transcript renderers resolve names at read time.

## Alternatives Considered

- **Store the participant registry on the ASR Server.** Rejected because it
  expands the server trust boundary and makes identity data portable to an
  operator who only needs anonymous audio inference.
- **Run CAM++ in the desktop application.** Rejected for the first version
  because it introduces a second Python/model runtime or a model conversion
  into the lightweight desktop package.
- **Identify speakers during every transcription.** Rejected because it
  changes existing latency and failure semantics and removes deliberate user
  confirmation.
- **Rewrite `segments_json` with names.** Rejected because retranscription,
  participant rename, sample deletion, and auditability become ambiguous.

## Consequences

- The existing transcription contract and offline recording path remain
  independent of voiceprint recognition.
- Identification requires an accessible compatible Nota ASR Server, even for
  recordings transcribed by another provider.
- Local storage gains biometric data and must exclude it from logs, frontend
  payloads, and exports unless a future explicit export design is accepted.
- Source-audio preview can disappear while the stored embedding remains
  usable.
- Matching thresholds can evolve without rewriting raw transcript data.

## Compatibility and Evolution

Server capabilities and embeddings carry a version, model fingerprint, and
dimension. Incompatible samples are retained but excluded from matching.
Introducing server-side identities, automatic confirmation, or a desktop
model runtime requires a new ADR.
