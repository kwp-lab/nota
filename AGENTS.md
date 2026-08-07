# AGENTS.md

## Scope

These instructions apply to the entire Nota repository. Add a nested
`AGENTS.md` or `AGENTS.override.md` only when a subdirectory needs more
specific rules.

## Project overview

- Nota is a Windows 11 x64 desktop application for reliable local meeting
  recording and optional user-configured speech transcription.
- The desktop shell is Tauri 2, the backend is Rust, and the frontend is
  React with TypeScript.
- Recording must continue to work fully offline. Network access is allowed
  only for an explicit transcription action or when the user has enabled
  automatic transcription.
- Keep raw audio, filesystem access, SQLite access, and ASR HTTP requests in
  the Rust backend. The frontend receives typed state and events, not PCM
  audio or stored API keys.
- Never include API keys, authorization headers, audio content, or transcript
  content in technical logs.

## Repository map

- `src/`: React and TypeScript user interface.
- `src-tauri/src/`: Rust application, audio engine, storage, and ASR pipeline.
- `src-tauri/capabilities/`: Tauri capability allowlists; keep permissions
  narrowly scoped.
- `scripts/`: versioning, licensing, Windows build, and release packaging.
- `docs/README.md`: engineering documentation index and source-of-truth rules.
- `docs/decisions/`: accepted architectural decision records.
- `docs/assets/`: tracked documentation assets.
- `release/`, `artifacts/`, `dist/`, `node_modules/`, and
  `src-tauri/target/`: generated local output; do not commit them.
- Product research documents are intentionally excluded from source control.

## Engineering rules

- Preserve the selected capture scope. If application loopback fails, report
  the failure instead of silently switching to system-wide capture.
- Treat recording finalization, recovery files, device changes, and partial
  source failures as data-integrity paths. Do not weaken their error handling
  for UI convenience.
- Keep Tauri IPC types synchronized between Rust and TypeScript.
- Keep controls idempotent and preserve the single-active-recording
  invariant.
- Do not add cloud services, telemetry, auto-update behavior, FFmpeg, virtual
  audio drivers, or runtime DLL dependencies without explicit approval.
- Preserve unrelated user changes in a dirty working tree.

## Documentation discipline

- Read `docs/README.md` before changing architecture, ASR behavior, storage,
  migrations, or release behavior.
- Keep the root README focused on product orientation, setup, and contributor
  entry points. Put detailed engineering semantics in `docs/`.
- Update the affected specification in the same change as behavior. Add an ADR
  for a long-lived architectural choice or reversal.
- Treat code, schemas, migrations, and tests as executable truth; use
  specifications for intended semantics and ADRs for rationale.
- When a change introduces complex business logic, cross-component ownership,
  three or more dependent stages, or a non-trivial state transition, add or
  update a Mermaid diagram in the owning specification under `docs/`.
- Keep diagrams version-controlled and update them in the same change as the
  behavior. Use flowcharts for processing flows, sequence diagrams for
  request/response interactions, and state diagrams for lifecycles. A diagram
  complements concise prose and executable tests; it does not replace either.
- Do not duplicate the complete server HTTP schema in this repository. The
  Nota ASR Server OpenAPI schema and API contract remain canonical.

## Verification

Run checks appropriate to the changed area before handing work off:

```powershell
npm run check
```

- Frontend-only changes may use `npm test` and `npm run build:web` while
  iterating, then must pass the checks appropriate to their final scope.
- Rust changes require formatting, tests, and Clippy.
- Cross-layer, version, packaging, or release changes require `npm run check`.
- `npm run build` creates the desktop executable and NSIS installer; it is not
  the frontend-only verification command.
- Add or update tests when behavior changes or a regression is fixed.

## Versioning and changelog

- Nota follows Semantic Versioning, and `CHANGELOG.md` follows Keep a
  Changelog.
- Before changing the version in `package.json`, inspect `CHANGELOG.md` and
  create or update the section for that exact version.
- Before creating or pushing any Git tag, verify that `CHANGELOG.md` contains
  an accurate, dated section for the tag's version and that its comparison
  links are current.
- A version bump or tag is not complete unless the corresponding
  `CHANGELOG.md` entry describes the user-visible additions, changes, and
  fixes included in that release.
- Keep upcoming work under `[Unreleased]`. At release time, move the relevant
  entries into `## [X.Y.Z] - YYYY-MM-DD`; do not leave released changes only
  under `[Unreleased]`.
- Change versions with:

```powershell
.\scripts\set-version.ps1 -Version X.Y.Z
.\scripts\verify-version.ps1
```

- Do not manually update only `package.json`. The version must remain
  synchronized across npm, Cargo, Tauri, lock files, and README badges.
- Git release tags use `vX.Y.Z` and must match the verified application
  version.

## Branch and pull request workflow

- For each new independently deliverable code feature, bug fix, or substantive
  cross-file change, start from an up-to-date `main` branch and create a
  dedicated feature branch before editing. Continue related follow-up work on
  the same branch. Use the `codex/` prefix unless the user requests another
  naming convention.
- Do not create a dedicated branch solely for a trivial, low-risk documentation,
  wording, metadata, or repository-instruction edit unless the user asks for
  one. Keep the branch workflow proportional to the change.
- When changes need to be merged into `main`, prefer a GitHub pull request. If
  the user asks to merge without specifying a method, recommend or create a PR
  instead of pushing directly to `main`. Push directly to `main` only when the
  user explicitly requests it.
- Creating a branch does not authorize committing, pushing, creating a PR, or
  merging it. Those actions still require explicit user authorization.

## Git and release hygiene

- Do not commit, push, create tags, or publish releases unless the user asks.
- Keep commits focused and do not include generated build output or local
  runtime data.
- A tag push starts the Windows release workflow, so run version verification
  and confirm the changelog before tagging.
- Release artifacts are Windows 11 x64 NSIS and portable packages generated
  by the repository scripts; do not hand-edit packaged output.
