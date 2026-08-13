# Changelog

All notable changes to Nota are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added a bilingual contribution guide covering privacy-safe fixtures,
  architecture boundaries, focused pull requests, and proportionate checks.
- Added generated third-party notices, a CycloneDX SBOM, an MPL dependency
  source archive, and license files in both Windows distribution formats.

- Added privacy-safe local diagnostic event logs for recording, audio recovery,
  imports, ASR, and LLM generation, with typed metadata, correlation IDs, UTC
  timestamps, bounded rotation, and no automatic upload.
- Added a **Diagnostics and logs** Settings entry that opens Nota's local log
  directory without changing saved settings.
- Added a selected-application reminder after three continuous minutes without
  audible output. The reminder describes prolonged silence rather than
  claiming the meeting ended, pauses its timer with recording, and re-arms only
  after application audio returns.

### Changed

- Split contribution CI into path-scoped frontend, native, and dependency
  compliance checks, while keeping executable and installer builds exclusive
  to tagged releases.
- Standardized locked frontend dependency downloads and CI installs on the
  official npm registry for reproducible public builds.
- Added locked dependency license policy gates for Rust, production npm, and
  the vendored libopus snapshot; release packaging now fails on stale or
  missing compliance artifacts.

- Licensed Nota under the MIT License with copyright held by `kwp-lab`.
- Replaced speculative meeting-ended detection with a capture-health reminder
  shown only when selected-application audio capture cannot be rebuilt for 15
  continuous seconds; successful recovery dismisses it automatically.

### Fixed

- Updated the Vite build chain's transitive PostCSS and nanoid dependencies to
  patched releases, clearing the known npm security advisories.
- Prevented screen sharing, window/PID changes, minimization, and silence from
  being interpreted as evidence that a meeting ended; prolonged silence is
  reported only as an observable audio condition.

## [0.6.0] - 2026-08-12

### Added

- Added per-version AI generation details with request/response JSON tabs,
  collapsible syntax highlighting, copy actions, and normalized input/output
  token usage. New versions retain credential-free request and successful raw
  response snapshots locally; older versions show an explicit unavailable
  state.
- Added a two-tab AI generation dialog with a wrapping, read-only preview of
  the exact Responses or Chat Completions request body, one-click copying, and
  a shared local input-token estimate powered by `tokenx`.
- Added offline, multi-file import for MP3, M4A, WAV, and FLAC phone or
  external recordings, with local progress, cancellation, per-file failures,
  exact-file duplicate detection, and immediate access to playback,
  transcription, speaker management, and AI documents.

### Changed

- Replaced Nota's handwritten input-token character heuristic with
  provider-aware `tokenx` estimation over the model-visible request fields;
  actual usage reported by the configured provider remains authoritative.
- Imported media is decoded locally and normalized to a Nota-managed 48 kHz
  mono Ogg Opus copy without FFmpeg; deleting the Nota recording does not
  modify the originally selected file.

### Fixed

- Reloaded the detail audio player after a recording rename moves its managed
  Ogg path, instead of retaining an unsupported stale source until another
  recording is selected.

## [0.5.0] - 2026-08-11

### Added

- Added Markdown-first AI meeting documents for completed transcripts, with
  meeting summaries, action items, speaker summaries, and speaker-grouped
  stand-up action items.
- Added configurable Responses API and OpenAI-compatible Chat Completions LLM
  providers, configurable input/output budgets, connection testing, and model
  discovery.
- Added custom AI templates, three context layers, immutable generation
  snapshots, and document/version selectors for create, regenerate, and revise
  workflows.
- Added a selected-meeting-target exit reminder: after a ten-second recovery
  window, Nota shows a portable-compatible Tauri decision prompt and highlights
  the tray so the user can explicitly continue recording or stop and save.
- Added session-scoped microphone switching during an active recording,
  including explicit device selection, a switching state, and the option to
  disable only the current recording's microphone without splitting the Ogg.

### Changed

- Standardized the client visual language with Fluent-inspired Nota design
  tokens for typography, semantic colors, spacing, shape, elevation, control
  sizing, and motion, including consistent 12 px tooltips, 14 px menus, and a
  shared compact action-button treatment for dense document toolbars.
- Every successful AI generation now creates a new shareable Markdown file;
  external edits remain authoritative, moved files are reported and can be
  identity-checked for relinking, and recording deletion preserves AI files by
  default.
- AI document commits now use no-replace atomic moves, cancellation is checked
  at the final commit boundary, incomplete model responses are rejected, and
  optional deletion verifies each Markdown file's embedded Nota identity.
- Long AI Markdown previews now scroll inside the document body, version actions
  explicitly state that they create a new version without overwriting, and new
  generations follow the currently configured Markdown root while Explorer
  selects the exact version file.
- Icon-only controls now use a consistent accessible Radix tooltip on hover and
  keyboard focus instead of the browser's unstyled native title popup.
- Meeting-window disappearance is detected even when an application keeps its
  meeting engine resident for warm-up. Continuing preserves the existing
  recording and capture state and suppresses repeat prompts until that target
  recovers and disappears again. The decision prompt now sizes itself to its
  rendered content across DPI and accessibility-text settings while remaining
  anchored above the taskbar.
- Microphone handoff now starts the replacement before committing it, rejects
  late packets from the previous device, preserves the meeting-audio buffer,
  inherits pause state, and reconverges echo cancellation after each switch.

### Fixed

- Fixed a microphone selected while recording was paused remaining inaudible
  after resume by initializing the replacement in a validated paused state and
  resetting the mixer before accepting resumed audio packets.

## [0.4.0] - 2026-08-06

### Added

- Added per-transcription FunASR speaker-count controls: manual jobs default to
  automatic detection or accept a known 1–64 count, while automatic jobs remain
  fully automatic and OpenAI-compatible providers keep their one-click flow.
- Added a custom recording-list context menu with rename, recycle-bin, and
  permanent-delete actions matching the recording detail menu.
- Added **Open containing folder** to the recording-list context menu while
  keeping the recording-detail overflow menu focused on its existing actions.
- Added explicit post-transcription speaker identification with bounded
  CAM++ sample extraction, conservative local voiceprint matching, a preview
  and name-confirmation dialog, and generation-scoped meeting assignments.
- Added a local **Voiceprints** workspace for choosing an independent Nota ASR
  Server, replaying available source segments, renaming participants, and
  deleting participants or individual samples.
- Added continuous speaker management from clickable transcript labels, with
  stable assignment prefill and multiple representative utterances per speaker.

### Changed

- Preserved conservative deterministic speaker clusters returned by updated
  Nota ASR Servers, allowing multiple anonymous labels to resolve to one local
  participant instead of requiring unsafe cross-window speaker merges.
- Clarified that a manually supplied speaker count is a safety target and may
  return additional anonymous speakers rather than force weakly similar people
  into one label, using the same server-side safety line as automatic mode.
- Accepted finer meeting-wide speaker turns returned by updated Nota ASR
  Servers for SenseVoice and Fun-ASR-Nano while keeping the batch protocol,
  local transcript schema, and existing completed recordings unchanged.
- Persisted the selected FunASR speaker count with each transcription
  generation so cancellation, restart, resume, and expired remote-job
  recreation preserve the original clustering safety target.
- Made `package.json` the unified contributor command entry point, separated
  frontend-only hooks from full Tauri development and build commands, and
  added explicit project-check, raw executable, and Windows release commands.
- Changed hosted Windows CI to manual dispatch so routine pull requests and
  main-branch pushes rely on the required local `npm run check`; tagged release
  builds remain automatic.
- Changed transcript copying and TXT export to share one formatter that
  includes provider speaker labels as `speaker_N：text` lines when available.
- Extended the successful TXT export notification with an eight-second
  **Open folder** action.
- Resolved confirmed participant names consistently in transcript details,
  clipboard copying, and TXT export while preserving raw `speaker_N` values.
- Changed voiceprint enrollment and preview to use CAM++ multi-candidate purity
  analysis, ignore short or mixed-speaker turns, and keep speakers without a
  sufficiently clean sample available for preview and meeting-local naming
  without saving a polluted reusable voiceprint.
- Kept the recording-detail player and transcription action bar visible while
  the transcript scrolls, and standardized segment timestamps as zero-padded
  `HH:MM:SS` values.
- Changed speaker management to open without a server request, start CAM++
  analysis only after an explicit action with a selected extraction service,
  prioritize unresolved speakers, and preserve user edits when late suggestions
  arrive.
- Separated meeting-name saves from biometric enrollment: voiceprint storage is
  now an optional, default-off choice after successful analysis.

### Fixed

- Made speaker-assignment saves incremental so a later partial confirmation no
  longer erases names confirmed in an earlier pass.
- Added hover, active color, pause icons, and range-end cleanup to clean and
  representative speaker previews so the playing sample is always visible.
- Simplified the speaker-manager header, promoted analysis actions to visible
  secondary buttons, added close-button and backdrop dismissal behavior, and
  kept a compact action footer visible when the application window is short.
- Closed recording action menus before destructive operations and stopped
  successful deletion of the selected recording from automatically opening
  the next recording; Nota now shows an explicit success notification and an
  empty detail state instead.

### Removed

- Removed the built-in pre-recording participant-notification prompt, its IPC
  fields and backend guard, and all acknowledgement storage from the current
  schema and runtime code. Legacy database rows, if present, remain inert.

## [0.3.0] - 2026-07-31

### Added

- Added resumable whole-meeting FunASR jobs with server-side window recovery,
  meeting-wide speaker reconciliation, cancellation, resume, and staged
  progress reporting.

### Changed

- FunASR now uploads the original Ogg recording through Nota ASR Server batch
  protocol v1; OpenAI-compatible providers retain the existing local WAV chunk
  workflow.
- Changed the tray icon so a single left-click restores and focuses Nota,
  while right-click continues to open the context menu.
- Redesigned global notifications as accessible, tone-aware light cards with
  restrained colors, icons, and motion.
- Updated the development and CI toolchain to Node.js 24.18.0 LTS and npm
  11.16.0, with the Node version shared through `.nvmrc`.

### Fixed

- Made successful and informational notifications dismiss automatically
  after three seconds while keeping warnings and errors visible until closed.
- Surfaced recording faults received after startup and deduplicated repeated
  snapshots of the same fault.
- Fixed local Windows release builds so they use the npm version pinned in
  `package.json` instead of depending on the globally installed npm version.

## [0.2.0] - 2026-07-29

### Added

- Added a dedicated recording library with search, playback, file management, and transcription status.
- Added a transcript workspace with timestamp seeking, provider-supplied speaker labels, full-text copying, and TXT export.
- Added optional speech-to-text through configurable FunASR and OpenAI-compatible providers.
- Added provider connection testing, model discovery, visible loading states, and inline success, warning, and error feedback.
- Added resumable transcription for long recordings using temporary 16 kHz mono WAV chunks with overlap-aware result merging.
- Added persistent transcription jobs, completed chunks, provider snapshots, and results to the local SQLite database.
- Added automatic transcription as an explicit opt-in setting.
- Added an About section showing the current application version and platform information.

### Changed

- Reworked the main interface into three consistent workspaces: Recorder, Recordings, and Settings.
- Moved Settings from a modal dialog into a full-height, independently scrollable workspace.
- Moved recent recordings out of the recorder screen and into the recording library.
- Made first-run setup non-blocking and kept recording controls available while browsing other workspaces.
- Changed the participant-notification reminder to appear only before the first recording.
- Changed saved API keys to use a standard masked password field; clearing the field and saving now removes the stored key.
- Added executable names to application capture targets, for example `[chrome.exe]: Google Meet`.
- Added the resolved Windows device name beside “Follow default device” selections.
- Changed tray behavior so a double left-click opens the window and a right-click opens the menu.
- Added separate tray actions for starting application recording and starting system recording.

### Fixed

- Fixed the recording history list so large libraries scroll independently from the transcript pane.
- Fixed application capture regressions that could produce microphone-only recordings.
- Fixed system capture and microphone instability when both streams use Bluetooth audio devices.
- Improved Bluetooth microphone jitter buffering and concealment of discontinuous packets.
- Improved microphone gain and reduced speech distortion caused by buffer underruns.
- Fixed recording finalization paths that could hang when stopping a meeting recording.
- Fixed provider tests and model discovery using stale saved values instead of the current unsaved form.
- Fixed connection-test and model-discovery feedback being hidden behind the former Settings modal.

## [0.1.1] - 2026-07-25

### Added

- Published the first downloadable GitHub release with an NSIS installer, portable ZIP, and SHA-256 checksums.

### Fixed

- Pinned the npm toolchain used by release builds to make GitHub Actions packaging reproducible.

## [0.1.0] - 2026-07-25

### Added

- Added selected-application recording through Windows process-loopback capture.
- Added system-wide recording through WASAPI endpoint loopback.
- Added independent microphone capture with device selection and default-communications-device following.
- Added local echo cancellation, clock alignment, drift correction, mixing, and peak limiting.
- Added compact 48 kHz mono Ogg Opus output without FFmpeg or a virtual audio device.
- Added crash-safe partial recording files, Ogg page recovery, and cross-volume finalization.
- Added recovery for device changes, USB disconnections, Bluetooth mode switches, and temporary process interruptions.
- Added pause, resume, stop-and-save, tray controls, and configurable global shortcuts.
- Added recording playback, rename, reveal-in-folder, recycle-bin deletion, and permanent deletion.
- Added local SQLite settings and recording indexes, rotating technical logs, and disk-space safeguards.
- Added Windows 11 x64 NSIS and portable packaging scripts.
- Added GitHub Actions workflows for continuous integration and tagged Windows releases.
- Added English and Simplified Chinese README documentation.

[Unreleased]: https://github.com/kwp-lab/nota/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/kwp-lab/nota/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/kwp-lab/nota/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/kwp-lab/nota/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/kwp-lab/nota/compare/v0.1.1...v0.3.0
[0.2.0]: https://github.com/kwp-lab/nota/compare/v0.1.1...a9091aaca969bf901e30d32ed55affe43f5efe2a
[0.1.1]: https://github.com/kwp-lab/nota/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/kwp-lab/nota/releases/tag/v0.1.0
