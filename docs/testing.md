# Testing and Acceptance

- Status: Accepted
- Last updated: 2026-08-01
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

- valid batch protocol v1 capability parsing;
- an old FunASR server producing a visible upgrade warning;
- authenticated batch request construction with a stable idempotency key;
- migration of old transcription rows to `legacy_chunks`;
- persistence of remote job identity and generic progress;
- UI rendering for byte upload, server queue, windows, diarization, and
  finalization;
- unchanged OpenAI-compatible multipart and response-format fallback behavior;
- Ogg Opus decode coverage for the legacy path.

Server-side automated coverage must include:

- idempotent creation and authentication isolation;
- sequential offset and SHA-256 validation;
- upload, processing-window, and final-result restart recovery;
- cancel and resume, including cancellation during a window;
- meeting-wide speaker clustering and overlap midpoint deduplication;
- explicit `diarization_failed` behavior and valid silent output;
- duration, upload-size, disk-space, and retention limits;
- exact `verbose_json 1.0` result compatibility.

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
