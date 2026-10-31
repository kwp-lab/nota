# ADR 0014: Separate Meeting-Local Names from Reusable People

- Status: Accepted
- Date: 2026-10-30
- Decision owners: Nota maintainers
- Refines: ADR 0003

## Context

One-off customers need readable speaker labels without becoming reusable people.
Previously every new manual name created a global participant, even without a
voiceprint. Voiceprints lists all such profiles, so routine name editing polluted
the library. Hiding zero-sample profiles alone would leave the ownership problem.

## Decision

Store typed labels in an additive `recording_speaker_names` table keyed by
recording, transcript generation and raw speaker. Keep existing participant links
in `recording_speaker_assignments`; a transactional writer maintains at most one
mapping kind per speaker. Return nullable participant ids through the existing
typed assignment map. All transcript consumers use the same resolved name map.

Default saves never create global people. Linking an existing person is explicit.
Creating a profile with a voiceprint requires an eligible server-held sample and
a default-false `saveVoiceprint` opt-in on that individual speaker. Mixed batches
commit local names, explicit links and selected enrollments atomically. Names and
embeddings remain local; the server never receives names. No new service or
dependency is introduced.

## Alternatives Considered

- Hide zero-sample profiles: insufficient; still creates unwanted global data.
- Make legacy participant foreign keys nullable by rebuilding their table:
  unnecessary migration risk for existing global identity/delete semantics.
- A dialog-wide enrollment checkbox: cannot express customers and reusable
  colleagues in one meeting safely.
- Session-only labels: loses useful names after restart.

## Compatibility and Evolution

Existing people, zero-sample profiles, voiceprints and historical links are not
deleted or reclassified. Their rename/delete behavior is unchanged. Local labels
are independent, persist across restarts and cascade with recording deletion.
Same-name text is not evidence of identity. Future legacy cleanup must be an
explicit migration preserving historical labels. Historical AI Markdown and name
snapshots remain immutable; only new generations use updated labels.
