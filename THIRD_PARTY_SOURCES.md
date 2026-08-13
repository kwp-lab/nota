# Third-party source availability

Nota's own source is MIT licensed. Some unmodified dependencies use reciprocal
licenses. The release workflow publishes an exact source archive for the
MPL-2.0 Rust crates shipped in the Windows binary. That archive corresponds to
the versions in `src-tauri/Cargo.lock` and is named
`Nota-<version>-mpl-sources.zip`.

## MPL-2.0 Rust crates included in the Windows release

| Crate | Version | Upstream source |
|---|---:|---|
| cssparser-macros | 0.6.1 | https://github.com/servo/rust-cssparser |
| cssparser | 0.36.0 | https://github.com/servo/rust-cssparser |
| dtoa-short | 0.3.5 | https://github.com/upsuper/dtoa-short |
| option-ext | 0.2.0 | https://github.com/soc/option-ext.git |
| selectors | 0.36.1 | https://github.com/servo/stylo |
| symphonia-bundle-flac | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-bundle-mp3 | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-codec-aac | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-codec-adpcm | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-codec-alac | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-codec-pcm | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-common | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-core | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-format-isomp4 | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-format-riff | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia-metadata | 0.6.0 | https://github.com/pdeljanov/Symphonia |
| symphonia | 0.6.0 | https://github.com/pdeljanov/Symphonia |

## MPL-2.0 production npm packages

| Package | Upstream source |
|---|---|
| None | - |

## Manually audited embedded native code

| Component | Exact source | Why it is tracked manually |
|---|---|---|
| libopus | https://github.com/xiph/opus/tree/7b05f44f4baadf34d8d1073f4ff69f1806d5cdb4 | audiopus_sys statically compiles its vendored libopus source; Cargo metadata only describes the wrapper crate |

The source archive is provided for license compliance and debugging; Nota does
not claim ownership of third-party code. If a dependency version or vendored
native snapshot changes, regenerate and review all compliance artifacts.
