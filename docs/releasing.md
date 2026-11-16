# Releasing Nota

- Status: Accepted
- Last updated: 2026-11-16
- Owners: Nota release maintainers
- Related files: `CHANGELOG.md`, `package.json`, `src-tauri/Cargo.toml`,
  `src-tauri/tauri.conf.json`, `.github/workflows/release.yml`, `scripts/`

Nota uses tag-driven GitHub Releases. A tag matching the application version builds
the Windows installer and portable archive, verifies them, and creates a draft
release for final review.

## Local command layers

`package.json` is the public command entry point for local development and
release preparation:

| Command | Contract |
|---|---|
| `npm run build:exe` | Build the optimized raw executable and skip installer bundling. |
| `npm run build` | Run Tauri's frontend hook, compile the optimized Rust executable, and build the NSIS installer. It does not run the full test suite or create portable/checksum assets. |
| `npm run check` | Run all repository quality gates without creating release assets. |
| `npm run licenses:generate` | Regenerate notices, source map, dependency inventory, and CycloneDX SBOM from locked dependencies. |
| `npm run licenses:check` | Verify policy, manual native checksums, and that committed compliance artifacts are current. |
| `npm run release:windows` | Reinstall locked npm dependencies, run `check`, build NSIS, then create the installer, portable ZIP, MPL source archive, legal assets, SBOM, and SHA-256 file under `release/`. |

The PowerShell files under `scripts/` implement complex Windows workflows but
are not separate contributor-facing entry points. CI may call the narrower
internal commands where its steps need separate names and logs.

## Prepare the version

Start a release branch from an up-to-date `main`. Move the release's entries
from `[Unreleased]` into a dated `CHANGELOG.md` section and update its comparison
links before changing the version. Historical versions without tags use their
version commits as comparison endpoints. Then update every application manifest
with one command and regenerate the version-dependent compliance artifacts:

```powershell
.\scripts\set-version.ps1 -Version 0.12.0
.\scripts\verify-version.ps1
npm run licenses:generate
npm run check
```

Review and commit the changelog, manifest, lockfile, and compliance changes,
then open a pull request to `main`. The PR Gate workflow runs the affected
frontend, native, and dependency-compliance checks automatically. Wait for
those checks before merging. The separate full CI workflow remains available
for manual dispatch.

Before tagging, produce and inspect the local release-grade assets when the
change affects packaging or Windows integration:

```powershell
npm run release:windows
```

## Trigger a release

After the version commit is on `main`, create one annotated tag with the exact
same version:

```powershell
git switch main
git pull --ff-only
.\scripts\verify-version.ps1 -Tag v0.12.0
git tag -a v0.12.0 -m "Nota v0.12.0"
git push origin v0.12.0
```

The release workflow will:

1. verify the tag against all application manifests;
2. run frontend and Rust tests on a Windows runner;
3. build the per-user NSIS installer;
4. verify the locked third-party license policy and committed legal artifacts;
5. create the portable ZIP, MPL source archive, standalone notices, SBOM, and
   SHA-256 checksum file;
6. upload the assets to a draft GitHub Release;
7. generate categorized release notes from merged pull requests.

## Review and publish

Open the draft on the repository's Releases page and verify:

- install, launch, record, stop, and playback with the NSIS build;
- launch, record, stop, and playback with the portable build;
- file names and SHA-256 checksums;
- `LICENSE`, third-party notices, source map, CycloneDX SBOM, and the MPL source
  archive are present and correspond to the tag's lock files;
- generated release notes and any migration or known-issue notes;
- the pre-release checkbox for preview builds.

Publish the draft only after those checks pass. Published release assets are
treated as immutable: never move an existing version tag or silently replace a
published binary. Fix a problem with a new patch version instead.

Re-running the workflow may replace assets only while the release remains a
draft. It deliberately fails if that tag already has a published release.
