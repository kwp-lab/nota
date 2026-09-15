# Testing and Acceptance

- Status: Accepted
- Last updated: 2026-08-27
- Owners: Nota maintainers
- Related configuration: `package.json`, `src-tauri/Cargo.toml`,
  `.github/workflows/ci.yml`, `.github/workflows/frontend.yml`,
  `.github/workflows/native.yml`, `.github/workflows/compliance.yml`,
  `.github/workflows/pr-gate.yml`

## Standard Verification

The repository-level verification entry point is:

```powershell
npm run check
```

`check` verifies synchronized versions, rejects frontend visual constants that
bypass Nota design tokens, builds and tests the frontend, checks Rust
formatting, runs locked Rust tests, and treats every Clippy warning as an
error. It delegates the Windows-specific orchestration to `scripts/check.ps1`.
This local command is the required routine quality gate. Cloud CI is layered
by changed path so ordinary contributions receive relevant feedback without
paying for an unnecessary full Windows build:

Hotword changes additionally require SQLite CRUD/migration and immutable
snapshot tests, Provider limit tests, DashScope vocabulary serialization,
Nota Server capability compatibility, and a privacy review confirming that
hotword bodies never enter logs or errors. Automated tests must not create
real cloud tasks.

Weighted-hotword changes must additionally cover ASCII/full-width delimiters,
zero and invalid normalization, escaped literal colons, conflicting duplicate
weights, the 50-super-hotword cap, legacy nullable-column migration, legacy
snapshot reads, DashScope default/super serialization, and Nota Server weight
removal.

- frontend source and configuration changes run design-token validation,
  frontend tests, and a web-only production build on Linux;
- Rust and Tauri changes run formatting, locked tests, and Clippy on Windows;
- dependency or legal-artifact changes run the locked license policy gate;
- documentation-only changes run only lightweight change detection and the
  required PR gate; they do not start application checks.

For pull requests, `.github/workflows/pr-gate.yml` detects the affected layers,
calls only those reusable workflows, and always reports one `PR Gate` result
that branch rules can require. The layers also run independently for relevant
pushes to `main`, cancel an older run for the same ref, and support manual
runs. `.github/workflows/ci.yml` remains a manual full-verification fallback.
None of these verification workflows calls `npm run build`, produces
`Nota.exe`, or creates an installer. Only the tag-triggered release workflow
builds distribution artifacts.
The dependency-compliance job caches only the pinned `cargo-about` executable;
a cache miss rebuilds that exact version, and the generator verifies its
version before use.

During focused development, the narrower commands remain available:

```powershell
npm test
npm run test:watch
npm run check:design
npm run build:web
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

Frontend visual changes also require `npm run check:design`; the repository
check runs it automatically. Other frontend changes require `npm test` and
`npm run build:web`. Rust changes require formatting, tests, and Clippy.
Cross-layer, migration, packaging, or release changes require `npm run check`.
`npm run build` is a desktop packaging command, not a substitute for the
verification suite.

Hardware-dependent Rust tests are ignored by default and document their
required devices or environment variables in the test name and ignore message.

## Test Ownership

| Area | Primary tests |
|---|---|
| React behavior and typed rendering | `src/*.test.tsx`, `src/components/*.test.tsx` |
| Settings navigation, autosave, Provider/template editors, and first run | `src/components/settings/*.test.tsx` and `src/App.test.tsx` |
| IPC serialization | `src-tauri/src/models.rs` tests |
| SQLite schema and migrations | `src-tauri/src/storage.rs` tests |
| ASR parsing, capability checks, request construction, chunk merging | `src-tauri/src/asr.rs` tests |
| DashScope policy/upload/task validation and result normalization | `src-tauri/src/asr/providers/dashscope.rs` tests plus ignored manual example |
| Voiceprint candidate planning, clean-range mapping, local matching, and confirmation sessions | `src-tauri/src/voiceprints.rs` tests |
| AI prompt boundaries, provider response parsing, Markdown metadata, version semantics, and SQLite snapshots | `src-tauri/src/ai.rs` and `src-tauri/src/storage.rs` tests |
| Ogg encoding, decoding, and recovery | `src-tauri/src/audio/` tests |
| Audio import decode/resample, journal commit, deduplication, and cleanup | `src-tauri/src/importer.rs` and `src-tauri/src/storage.rs` tests |
| Recording state and idempotent controls | `src-tauri/src/state_machine.rs` tests |
| Tray and controller behavior | `src-tauri/src/controller.rs` tests |
| Release consistency | `scripts/verify-version.ps1` and CI |
| Visual token discipline | `scripts/check-design-tokens.mjs` |

The Nota ASR Server repository owns protocol endpoint, authentication,
server-restart, window-recovery, diarization, and final response contract
tests. Client and server tests complement each other; neither repository should
duplicate the other's internal implementation fixtures.

## Application Capture Target Regression Matrix

Automated coverage must include:

- local PNG icon data crossing Rust/TypeScript IPC without exposing native
  window handles;
- application-icon extraction failure degrading to a neutral UI fallback
  without failing target enumeration or entering the recording-start path;
- selected-target icons, two-line title/executable options, long-text
  truncation, and the retained “未运行” preference state;
- Enter/Space opening, arrow-key navigation, typeahead, Enter selection, Escape
  dismissal, focus restoration, and an accessible name for every option;
- PID rebinding by executable path and no silent switch to another application.

## Audio Import Regression Matrix

Automated coverage must include:

- supported-extension validation for MP3, M4A, WAV, and FLAC plus a clear
  unsupported-format error;
- stereo downmix and non-48-kHz resampling into a readable Nota Ogg Opus file;
- imported recording origin and source-display metadata across Rust/TypeScript
  IPC;
- exact source-hash deduplication without changing or deleting the original;
- journal ordering, final recording insertion, and journal removal;
- cancellation and per-item failure without discarding earlier completed
  imports;
- recording/import mutual exclusion and visible batch progress/actions;
- imported-recording deletion copy that distinguishes the managed Ogg from the
  original source.

## ASR Regression Matrix

Automated client coverage must include:

- recording-list context menus exposing **Open containing folder** only for
  list items while preserving the three-action detail overflow menu;
- compact recording-detail controls above an independently scrolling transcript,
  with a stable native audio player below the body;
- transcript segment timestamps rendered as zero-padded `HH:MM:SS`, including
  meetings longer than one hour;
- valid batch protocol v1 capability parsing;
- an old FunASR server producing a visible upgrade warning;
- authenticated batch request construction with a stable idempotency key;
- migration of old transcription rows to `legacy_chunks`;
- migration from one-row-per-recording transcription storage to the composite
  generation key, preservation of completed results, current-version pointer
  updates, and completed-version selection;
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
- DashScope fixed endpoint/model, `2–100` count, two-hour preflight, trusted
  HTTPS host validation, sentence/speaker normalization, generation snapshot,
  and completed-checkpoint cleanup;
- DashScope UI fixed fields, no model discovery, temporary-cloud disclosure,
  automatic-upload wording, and generation-bound voiceprint degradation;
- recording-detail switching between FunASR and DashScope transcript versions,
  including generation-specific text, speaker count, manual assignments, and
  voiceprint availability;
- no real DashScope request in routine automated tests. The opt-in
  `examples/dashscope-filetrans/verify_hotword_effect.py` probe performs a
  paid A/B transcription only when invoked manually; its ignored sample and
  local `.env` stay outside version control;
- selected-application capture failure using a 15-second continuous grace,
  transient-recovery reset, automatic prompt dismissal after successful
  recovery, one alert per uninterrupted failure, session-scoped stale-action
  rejection, and explicit continue/stop actions without capture-scope mutation;
- selected-application silence using the `0.001` peak threshold and a continuous
  three-minute delay, pause/rebuild timer reset, audible-audio re-arming,
  one reminder per silent period, and reason-specific prompt rendering;
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

## Settings Center Regression Matrix

Automated coverage for Settings must include:

- category navigation, parent-category highlighting on management routes,
  management-page back navigation, session-only route restoration, and
  first-run routing precedence;
- full-height settings mode retaining the application sidebar while omitting
  the normal global header/footer, including the active-recording notice;
- serialized ordinary-settings autosave, newest-snapshot coalescing, success
  feedback, persistence rollback, directory-picker cancellation, specialized
  default Provider commands, and no leave prompt for confirmed ordinary
  settings;
- paired shortcut draft submission on blur or Enter, whitespace trimming,
  conflict feedback without replacing the effective pair, disabled shortcuts,
  and no shortcut re-registration for unrelated settings writes;
- ASR and LLM Provider API-key keep/replace/clear, unsaved-draft connection
  testing, model discovery and manual fallback, explicit save/delete, default
  cleanup and first-valid default selection;
- DashScope fixed fields, no model discovery, and temporary upload disclosure;
- built-in template read-only behavior and copying, custom-template create,
  save, revision update, archive, load failures, and dirty-editor navigation
  protection;
- the first-run checklist allowing non-blocking category and Provider
  navigation, Provider round trips, and only **Finish** or **Set up later**
  persisting completion for the next startup.

Manual visual acceptance covers `1280×800` and `980×640`, Windows `125%`
scaling, Chinese long text, keyboard navigation and focus, plus disabled,
loading, empty-list, and error states. The category navigation must remain
visible and the workspace must not scroll horizontally.

## Diagnostic Logging Regression Matrix

Automated coverage for local diagnostics must include:

- UTC RFC3339 timestamps, one-line records, JSON escaping, and bounded text;
- the typed field allowlist rejecting sensitive field names;
- one active log plus two archives after rotation;
- Settings rendering and the open-directory command path without enqueueing an
  unrelated settings save or changing complex-editor dirty state;
- lifecycle metadata for recording, import, ASR, and LLM work, including
  correlation IDs, coarse phases, HTTP status, elapsed time, estimates, and
  actual token usage where available;
- no audio, transcript or document content, titles, user paths, Base URLs,
  credentials, authorization values, request/response bodies, or raw Provider
  errors in emitted diagnostic records.

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
- credential-free request JSON matching the submitted body, successful raw
  response JSON and normalized usage persistence, nullable-column migration
  for historical databases, and on-demand IPC reads;
- a keyboard-operable Generation details drawer and Request/Response tabs,
  collapsible syntax-highlighted JSON, copy actions, long-value wrapping, and
  explicit unavailable states for historical versions;
- recording deletion preserving Markdown by default and deleting exact linked
  paths only after explicit opt-in, completed-status filtering, and an immediate
  YAML identity check;
- stale asynchronous preview responses never replacing the currently selected
  version, and official OpenAI providers without a key staying out of generation
  UI while unauthenticated third-party Responses providers remain available;
- icon-only actions exposing an accessible custom tooltip on keyboard focus
  without retaining a duplicate native `title` tooltip;
- application restart converting queued or generating rows to `interrupted`.

Provider integration acceptance uses synthetic text only. It must not place a
real meeting transcript or generated business document in committed fixtures or
ordinary technical logs.

## Manual Hardware Acceptance

Before a release that changes audio capture or ASR behavior, exercise relevant
scenarios on Windows 11:

1. Open the application selector with Zoom, Tencent Meeting, Feishu, Chrome or
   Edge, and one ordinary Win32 application running. Confirm software icons,
   the fallback icon, long-title truncation, keyboard selection, app exit and
   PID restart at 100%, 125%, and 150% display scaling. Then record and play
   back a normal meeting with application capture and a microphone.
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
14. With Enterprise WeChat `5.0.9.6065` and Tencent Meeting, keep the meeting
    connected while switching layouts, sharing the screen for more than 30
    seconds, minimizing, restoring, and changing child processes. Confirm none
    of those window/process transitions triggers a reminder. Keep the selected
    application silent: no reminder may appear before three minutes, and
    **Application has had no sound for a while** must appear at approximately
    three minutes without claiming that the meeting ended. **Continue
    recording** must suppress repeats until audible application audio returns;
    pause time and capture-rebuild time must not count toward the three minutes.
    Then force a repeatable process-loopback capture failure: a short failure
    that recovers before 15 seconds must not prompt; a continuous failure must
    show **Application audio capture interrupted** after about 15 seconds.
    Recovery must dismiss that prompt. Both continue actions must retain the
    current scope, microphone, pause state, session, and Ogg file. Finally
    choose **Stop and save** and verify normal finalization. Repeat prompt layout
    checks at 100%, 150%, and 200% display scaling with enlarged system text.
15. During one recording, switch between two physical microphones, disable the
    microphone, enable it again, and repeat a switch while paused. Confirm the
    same Ogg file remains active, meeting audio is not interrupted by stale
    microphone packets, the UI reflects the backend-selected device, AEC shows
    convergence after each enabled switch, and a deliberately unavailable
    device leaves the previous microphone selected. Repeat once with a
    Bluetooth headset to cover A2DP/HFP mode changes.
16. Import one file each in MP3, AAC-LC M4A, ALAC M4A, WAV, and FLAC from a
    removable or phone-synchronized directory. Confirm duration, playback,
    seeking, transcription, and AI-document actions use the managed Ogg; the
    original files remain byte-identical. Import an exact duplicate, a corrupt
    file, an HE-AAC/protected M4A, and a batch with one bad middle item. Confirm
    duplicate/unsupported failures are explicit, later items continue, cancel
    preserves completed items, and no `.partial.ogg` remains. Repeat with low
    disk space and by terminating the app at the prepared-file commit boundary,
    then relaunch and verify startup reconciliation.

Real-model and hardware acceptance results should record software versions,
model id, device type, audio duration, and pass/fail observations. They must not
include the recording or transcript in ordinary logs or committed artifacts.

## Recording Detail Layout Acceptance

### Application Icon Acceptance

- Confirm the sidebar microphone artwork matches the installed Start menu and
  window icon. It must not render a replacement Lucide action glyph.
- Check the Windows tray on light/dark taskbars at 100%, 125%, 150% and 200%
  scaling: idle uses the app mark only; preparing/recording/finalizing add a red
  lower-right badge. Pause uses amber; capture reminders/errors use warning
  amber. Check tooltip/menu distinctions and pending-decision priority.
- Finish recording and confirm the unmodified idle icon returns. Tray show,
  pause/resume, stop/save and capture-prompt re-entry must still work.
- Pixel tests verify the badge leaves the rest of the icon unchanged and that
  idle states exactly match the embedded asset. Browser/pixel tests do not
  replace this Windows system-tray check.

### Layout and Dialogs

- Verify the full-height recording workspace at 1280×800, 980×640 and Windows
  125% scaling: persistent app navigation, independent list/body scrolling,
  bottom playback controls, and no horizontal page overflow.
- Check focus mode, keyboard tab switching, overflow actions, drawer Escape and
  focus restoration. Tab/focus changes must not replace audio or reset playback
  and the current record's reading positions. Recording-active warnings and the
  return-to-recorder action remain available.
- Compare create, regenerate and revise dialogs with the pre-layout baseline:
  all fields, settings/request tabs, Provider/model linkage, token estimates,
  budget errors and submit/cancel states must remain unchanged. Assert complete
  generation payloads, including the revise source-version ID.
- Regress generation progress/cancel, missing/externally modified Markdown,
  file relinking, stale responses, and on-demand request/response inspection.
  Retain all existing playback, speaker, export and transcription tests.
- Verify generation dialogs and the details drawer close on backdrop clicks,
  but not inside clicks, cancelled pointers, secondary buttons, or text-selection
  drags ending outside. Submission blocks close/cancel/backdrop dismissal. Check
  drawer focus restoration and JSON keyboard expansion after the Lucide icon
  adaptation. At both viewport sizes, scroll the complete form and confirm
  header/footer remain visible and scrollbars never cover the shell's corners.
- Visual fixtures must be synthetic and must not call real model services.
  Browser viewport checks do not replace real Windows/WebView2 scaling and
  audio-device acceptance.

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
