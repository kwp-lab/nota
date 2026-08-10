# Testing and Acceptance

- Status: Accepted
- Last updated: 2026-08-08
- Owners: Nota maintainers
- Related configuration: `package.json`, `src-tauri/Cargo.toml`,
  `.github/workflows/ci.yml`

## Standard Verification

The repository-level verification entry point is:

```powershell
npm run check
```

`check` verifies synchronized versions, builds and tests the frontend, checks
Rust formatting, runs locked Rust tests, and treats every Clippy warning as an
error. It delegates the Windows-specific orchestration to `scripts/check.ps1`.
This local command is the required routine quality gate. `.github/workflows/ci.yml`
exposes the same clean-Windows verification as a manual `workflow_dispatch`;
pull requests and pushes do not start cloud CI automatically. The tag-triggered
release workflow remains independent and automatic.

During focused development, the narrower commands remain available:

```powershell
npm test
npm run test:watch
npm run build:web
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

Frontend changes require `npm test` and `npm run build:web`. Rust changes
require formatting, tests, and Clippy. Cross-layer, migration, packaging, or
release changes require `npm run check`. `npm run build` is a desktop packaging
command, not a substitute for the verification suite.

Hardware-dependent Rust tests are ignored by default and document their
required devices or environment variables in the test name and ignore message.

## Test Ownership

| Area | Primary tests |
|---|---|
| React behavior and typed rendering | `src/*.test.tsx`, `src/components/*.test.tsx` |
| IPC serialization | `src-tauri/src/models.rs` tests |
| SQLite schema and migrations | `src-tauri/src/storage.rs` tests |
| ASR parsing, capability checks, request construction, chunk merging | `src-tauri/src/asr.rs` tests |
| Voiceprint candidate planning, clean-range mapping, local matching, and confirmation sessions | `src-tauri/src/voiceprints.rs` tests |
| AI prompt boundaries, provider response parsing, Markdown metadata, version semantics, and SQLite snapshots | `src-tauri/src/ai.rs` and `src-tauri/src/storage.rs` tests |
| Ogg encoding, decoding, and recovery | `src-tauri/src/audio/` tests |
| Recording state and idempotent controls | `src-tauri/src/state_machine.rs` tests |
| Tray and controller behavior | `src-tauri/src/controller.rs` tests |
| Release consistency | `scripts/verify-version.ps1` and CI |

The Nota ASR Server repository owns protocol endpoint, authentication,
server-restart, window-recovery, diarization, and final response contract
tests. Client and server tests complement each other; neither repository should
duplicate the other's internal implementation fixtures.

## ASR Regression Matrix

Automated client coverage must include:

- recording-list context menus exposing **Open containing folder** only for
  list items while preserving the three-action detail overflow menu;
- one sticky recording-detail control region containing the audio player and
  transcription actions, with the transcript body outside that region;
- transcript segment timestamps rendered as zero-padded `HH:MM:SS`, including
  meetings longer than one hour;
- valid batch protocol v1 capability parsing;
- an old FunASR server producing a visible upgrade warning;
- authenticated batch request construction with a stable idempotency key;
- migration of old transcription rows to `legacy_chunks`;
- persistence of remote job identity and generic progress;
- FunASR manual speaker-count defaults, 1/64 boundaries, invalid input,
  cancellation, retranscription, and recovery from the persisted snapshot;
- automatic transcription always using automatic speaker detection and the
  OpenAI-compatible path remaining a one-click flow;
- UI rendering for byte upload, server queue, windows, diarization, and
  finalization;
- speaker-sample-analysis capability discovery, repeated multipart upload,
  clean-range validation, and strict response compatibility;
- local matching threshold plus runner-up margin behavior;
- speaker management opening without an analysis request, explicit analysis
  start only with a selected provider, disabled configuration guidance without
  one, late-session cleanup after close, and no overwrite of a dirty or
  persisted selection;
- stable participant-id prefill, unresolved-first ordering, representative
  utterance playback, active preview/pause feedback, range-end cleanup, and
  meeting-local edits from transcript speaker labels;
- name-only saves that never enroll embeddings by default, plus explicit
  optional enrollment for named candidates with reusable embeddings;
- incremental assignment upsert and explicit single-speaker clear preserving
  every omitted current-generation assignment;
- raw transcript preservation, generation-scoped assignments, participant
  rename/delete, and voiceprint sample deletion;
- unchanged OpenAI-compatible multipart and response-format fallback behavior;
- Ogg Opus decode coverage for the legacy path.
- selected-target exit grace timing, window-bound versus audio-session-only
  liveness, resident-process window disappearance, one reminder per continuous
  absence, reset after recovery, session-scoped stale-action rejection, and
  explicit continue/stop prompt actions without capture-scope mutation;
- active-recording microphone selection, visible switching state, session-only
  disable behavior, failed-switch rollback, authoritative snapshot recovery,
  source-epoch filtering, microphone-only buffer reset, AEC reconvergence, and
  pause-state inheritance without changing the meeting-audio scope;

Server-side automated coverage must include:

- idempotent creation and authentication isolation;
- sequential offset and SHA-256 validation;
- upload, processing-window, and final-result restart recovery;
- cancel and resume, including cancellation during a window;
- meeting-wide speaker clustering and overlap midpoint deduplication;
- explicit `diarization_failed` behavior and valid silent output;
- duration, upload-size, disk-space, and retention limits;
- exact `verbose_json 1.0` result compatibility.

## AI Document Regression Matrix

Automated coverage for AI documents must include:

- four deterministic built-in templates and immutable built-in behavior;
- custom-template clone, revision increment, and archive behavior;
- LLM provider API-key masking plus keep, replace, clear, and delete behavior;
- configurable Responses API root/full-endpoint request and response parsing,
  official-OpenAI authentication requirements, and compatible Chat Completions
  response parsing, including rejection of incomplete or
  token-truncated output, without placing prompt content in logs;
- one document per recording/template and monotonically increasing version
  numbers, including failed and cancelled attempts;
- `regenerate` without parent output and `revise` with a validated parent;
- speaker-template rejection when the transcript has no speaker labels;
- local input-budget rejection before network access;
- collision-safe create-new paths, YAML identities, atomic file writes, and no
  overwrite of an existing or concurrently created path;
- `ready`, `modified`, and `missing` file states plus identity-checked relink;
- safe Markdown preview with raw HTML disabled and remote images not loaded;
- document-first and version-second UI selection, with newest successful
  version selected by default;
- three context lifetimes and per-version prompt/provider/template snapshots;
- recording deletion preserving Markdown by default and deleting exact linked
  paths only after explicit opt-in, completed-status filtering, and an immediate
  YAML identity check;
- stale asynchronous preview responses never replacing the currently selected
  version, and official OpenAI providers without a key staying out of generation
  UI while unauthenticated third-party Responses providers remain available;
- application restart converting queued or generating rows to `interrupted`.

Provider integration acceptance uses synthetic text only. It must not place a
real meeting transcript or generated business document in committed fixtures or
ordinary technical logs.

## Manual Hardware Acceptance

Before a release that changes audio capture or ASR behavior, exercise relevant
scenarios on Windows 11:

1. Record and play back a normal meeting with application capture and a
   microphone.
2. Start a FunASR transcription, interrupt upload, and confirm it resumes from
   the server offset.
3. Exit Nota while the server is processing, relaunch, resume, and confirm the
   existing remote job is used.
4. Cancel during server inference, wait for the current server window to stop,
   then resume.
5. Complete a result locally and confirm remote cleanup failure does not remove
   the local transcript.
6. Test a legacy OpenAI-compatible provider to ensure WAV chunk behavior has
   not regressed.
7. Use a controlled long recording with at least two speakers whose turns cross
   server window boundaries; verify one speaker keeps one label throughout the
   final meeting.
8. For the four-hour limit, confirm working memory remains bounded by the
   server window and verify disk-full errors are explicit.
9. With Paraformer, retranscribe a controlled rapid-turn recording once with a
   known speaker-count target and once with automatic detection. Confirm the
   target reaches whole-meeting clustering, weakly similar speakers are not
   merged merely to reach it, sentence timestamps remain ordered, and automatic
   mode does not change the response schema.
10. With SenseVoice and Fun-ASR-Nano, retranscribe a rapid-turn region that has
    little silence. Confirm a multi-speaker VAD may become finer segments,
    concatenated text is unchanged, speaker timestamps remain ordered, and the
    client needs no protocol or local-database migration.
11. Use a meeting that produces at least 20 local window centroids. Confirm
    whole-meeting clustering is deterministic in automatic and specified-count
    modes, weakly similar people are not merged at a window boundary, a target
    may return additional safe clusters, and multiple raw labels can still be
    resolved to one local participant name.
12. In recording management, right-click a list item and open its containing
    folder. Then scroll a long transcript and confirm the player and
    transcription actions span the full scroll-viewport width, retain internal
    spacing around the player, and cast a shadow only below the pinned region.
    Confirm no transcript content leaks above or beside it and every timestamp
    keeps the `HH:MM:SS` form.
13. Open speaker management on a meeting with at least three raw speakers and
    confirm no server request starts. Play a clean preview and several
    representative utterances; verify hover, playing color, pause icon,
    switching, and range-end cleanup. Reduce the Nota window height and confirm
    the compact action footer and save button remain visible while both speaker
    panes scroll independently. Save one name with voiceprint persistence left
    off, reopen, and confirm the assignment is preselected and no sample was
    added. Then explicitly run analysis, opt in to saving a reusable sample,
    and verify only named, eligible speakers are enrolled. Finally assign the
    last speaker without a configured server and confirm clearing one mapping
    leaves every omitted mapping untouched.
14. Record an Enterprise WeChat meeting by selecting its meeting window, leave
    the meeting, and confirm the `wwmapp.exe` warm-up process may remain while
    the selected window disappears. Confirm no prompt appears during the
    ten-second recovery window, then confirm the prompt and warning tray state
    appear. **Continue recording** must leave the current recording and
    microphone behavior unchanged, and no second prompt may appear while the
    target remains absent. Reopen and close the same target to confirm a new
    reminder is allowed; finally choose **Stop and save** and verify normal Ogg
    finalization, library refresh, and optional automatic transcription. Repeat
    at 100%, 150%, and 200% display scaling and with enlarged system text;
    confirm the prompt follows its content within the bounded height, remains
    anchored above the taskbar, and keeps both decisions reachable.
15. During one recording, switch between two physical microphones, disable the
    microphone, enable it again, and repeat a switch while paused. Confirm the
    same Ogg file remains active, meeting audio is not interrupted by stale
    microphone packets, the UI reflects the backend-selected device, AEC shows
    convergence after each enabled switch, and a deliberately unavailable
    device leaves the previous microphone selected. Repeat once with a
    Bluetooth headset to cover A2DP/HFP mode changes.

Real-model and hardware acceptance results should record software versions,
model id, device type, audio duration, and pass/fail observations. They must not
include the recording or transcript in ordinary logs or committed artifacts.

## Documentation Verification

Documentation-only changes do not require rebuilding the application unless
they reveal or accompany a code change. Before merging them:

- ensure all relative links resolve;
- verify state names and field names against Rust and TypeScript types;
- verify protocol semantics against the server OpenAPI schema and contract
  tests;
- confirm planned behavior is marked draft;
- check that README, specification, ADR, and changelog serve distinct purposes
  rather than copying the same text.

## Definition of Done

A behavior change is complete when:

- implementation and migrations are present;
- appropriate automated tests pass;
- user-visible changes are in `CHANGELOG.md`;
- README entry-point material is current;
- affected specifications are updated;
- an ADR records any long-lived architectural decision;
- privacy and logging constraints remain intact.

Transcript output changes must additionally verify that clipboard and TXT
serialization remain identical, speaker labels are preserved without invented
identities, plain-text fallback is unchanged, and post-export file navigation
does not expose transcript content in logs.
