# Changelog

All notable changes to Nota are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/kwp-lab/nota/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/kwp-lab/nota/compare/v0.1.1...v0.3.0
[0.2.0]: https://github.com/kwp-lab/nota/compare/v0.1.1...a9091aaca969bf901e30d32ed55affe43f5efe2a
[0.1.1]: https://github.com/kwp-lab/nota/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/kwp-lab/nota/releases/tag/v0.1.0
