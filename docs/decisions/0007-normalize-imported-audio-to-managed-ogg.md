# ADR 0007: Normalize Imported Audio to a Managed Ogg Copy

- Status: Accepted
- Date: 2026-08-11
- Last updated: 2026-08-11
- Decision owners: Nota maintainers

## Context

Users may record an offline meeting on a phone and later want Nota playback,
transcription, speaker management, and AI documents. Those workflows currently
assume a stable local Ogg Opus recording with known timing and decoding
behavior. Referencing arbitrary external formats in place would spread codec
branches across playback, ASR, voiceprint, deletion, and recovery paths.

Shipping FFmpeg would broaden format support, but it introduces a large native
runtime, distribution and license work, PATH/sidecar behavior, and another
failure surface in portable builds. Mutating or moving the source would also
violate user ownership expectations.

## Decision

Nota will treat imported media as a read-only source and create a Nota-owned,
48 kHz mono Ogg Opus copy in the configured recording directory. Rust will use
built-in Symphonia decoding for the explicitly supported MP3, M4A, WAV, and
FLAC formats, the existing Rubato resampling path, and the existing Nota Opus
writer. No FFmpeg executable or runtime DLL is required.

The filesystem and SQLite commit will use a private journal plus a hidden
partial file. Imported rows are distinguished from captured rows and retain
display metadata plus an internal exact-source hash. Deletion affects only the
managed copy. Every downstream feature consumes the managed Ogg through the
existing recording identity.

## Alternatives Considered

- Keep the external file in place and index it directly. Rejected because
  removable/moved files are fragile and every downstream audio consumer would
  need arbitrary-format support and different deletion semantics.
- Copy the original bytes without normalization. Rejected because it preserves
  source ownership but still spreads codec and timestamp differences across
  downstream features.
- Bundle FFmpeg or require a system installation. Rejected for the first phase
  because deployment weight, native sidecar maintenance, licensing review, and
  portable-build complexity are disproportionate to the target formats.
- Convert in React. Rejected because raw audio, filesystem ownership, and long-
  running media work belong in the Rust backend.
- Upload to a conversion service. Rejected because import must remain offline
  and meeting audio is private.

## Consequences

- Imported recordings behave like captured recordings after one local
  conversion and reuse all existing Ogg-based workflows.
- Import requires additional temporary and final disk space and is not
  instantaneous for long recordings.
- Exact duplicates can be detected without exposing audio or hashes to React.
- The original source can be moved or deleted after a successful import.
- Supported formats are deliberately narrower than FFmpeg. HE-AAC,
  DRM/protected M4A, video containers, and unusual codecs receive explicit
  errors.
- Symphonia and its transitive licenses become part of release inventory.

## Compatibility and Evolution

Adding a decoder format is additive only after playback/transcription
acceptance, license review, and documentation. A future optional FFmpeg sidecar
may extend formats, but must preserve the managed-Ogg boundary and original-
source ownership. Changing the canonical managed format or indexing external
files directly requires a superseding ADR and migration plan.
