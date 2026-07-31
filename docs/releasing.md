# Releasing Nota

- Status: Accepted
- Last updated: 2026-07-31
- Owners: Nota release maintainers
- Related files: `CHANGELOG.md`, `package.json`, `src-tauri/Cargo.toml`,
  `src-tauri/tauri.conf.json`, `.github/workflows/release.yml`, `scripts/`

Nota uses tag-driven GitHub Releases. A tag matching the application version builds
the Windows installer and portable archive, verifies them, and creates a draft
release for final review.

## Prepare the version

Start from an up-to-date branch and update every application manifest with one
command:

```powershell
.\scripts\set-version.ps1 0.2.0
.\scripts\verify-version.ps1
```

Review and commit the manifest and lockfile changes. Let CI pass on the pull
request before merging it to `main`.

## Trigger a release

After the version commit is on `main`, create one annotated tag with the exact
same version:

```powershell
git switch main
git pull --ff-only
git tag -a v0.2.0 -m "Nota v0.2.0"
git push origin v0.2.0
```

The release workflow will:

1. verify the tag against all application manifests;
2. run frontend and Rust tests on a Windows runner;
3. build the per-user NSIS installer;
4. create the portable ZIP and SHA-256 checksum file;
5. upload the assets to a draft GitHub Release;
6. generate categorized release notes from merged pull requests.

## Review and publish

Open the draft on the repository's Releases page and verify:

- install, launch, record, stop, and playback with the NSIS build;
- launch, record, stop, and playback with the portable build;
- file names and SHA-256 checksums;
- generated release notes and any migration or known-issue notes;
- the pre-release checkbox for preview builds.

Publish the draft only after those checks pass. Published release assets are
treated as immutable: never move an existing version tag or silently replace a
published binary. Fix a problem with a new patch version instead.

Re-running the workflow may replace assets only while the release remains a
draft. It deliberately fails if that tag already has a published release.
