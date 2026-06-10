# Release Model

xPare follows the normal open-source macOS pattern:

- **Source is the canonical distribution** for contributors and privacy-sensitive
  users (`make app` / `make run` build and launch a local bundle).
- **Tagged GitHub Releases** (`v*`) are the end-user download surface.
- macOS binary assets are built from a tag, checksummed, and attached to the
  release with notes.
- Broad end-user downloads must be **Developer ID signed and notarized**.

Unlike the upstream FormatStripper lineage, the xPare Swift app **statically
links** `libxpare_ffi.a`, so a release bundle has **no embedded dylib** under
`Contents/Frameworks` to relocate or sign separately — the bundle is the Swift
executable plus its `Info.plist` and icon, and the Developer ID signature covers the
single Mach-O and the bundle.

## Current status

The local product is source-built. From the repo root:

```sh
make app     # build + assemble shells/macos/dist/xPare.app (ad-hoc signed)
make run     # the above, then `open` it
```

For an explicitly **unsigned preview** archive (no Apple account required):

```sh
make preview                 # uses an exact vX.Y.Z tag, or:
make preview VERSION=1.2.3
```

That writes `dist/release/xPare-vX.Y.Z-macos-<arch>-unsigned-preview.zip` and
its `.sha256`. Preview archives are test artifacts, not official binaries: they are
unsigned/ad-hoc and must not be promoted as end-user downloads. There are **no
official downloadable release assets yet** — by design, the bundle is
unsigned/un-notarized until Developer ID credentials exist.

## Target release channels

1. GitHub Releases for users.
2. Build-from-source for contributors and privacy-sensitive users.
3. Homebrew Cask after stable signed/notarized releases exist.

Signed assets are architecture-labeled:

```text
xPare-vX.Y.Z-macos-arm64.zip
xPare-vX.Y.Z-macos-x86_64.zip
```

If the project later ships universal binaries, asset names should make the
architecture choice explicit.

## Release workflow

`.github/workflows/release.yml` runs on `v*` tags:

1. **validate** (always): checkout → require a successful manual Release Fuzz run
   on the exact tag SHA → `cargo run -p xtask -- ci` (the full gate) → build the
   FFI staticlib + `swift build -c release` → `make preview` → upload the unsigned
   preview as a workflow artifact.
2. **publish-official** (gated): only when repo variable
   `XPARE_ENABLE_OFFICIAL_RELEASE == 'true'` **and** the Apple secrets are
   present. It re-checks `shells/macos/xPare.entitlements`, imports the
   Developer ID cert into a temp keychain, stores notary credentials, runs
   `make dist` (Developer ID sign executable and bundle with the checked App
   Sandbox entitlements → verify both signed payloads are minimal → notarize →
   staple → zip → checksum → verify) and `make github-release`, then wipes the
   signing material.

Signing/notary credentials are **never** required for pull-request CI. Absent them,
CI still builds and tests, but must not publish an artifact as an official binary.

## Release fuzz gate

Run the manual Release Fuzz workflow on the release-candidate ref before tagging
or rerunning the final release:

```sh
gh workflow run release-fuzz.yml --ref v1.2.3-rc.1 -f minutes_per_target=30
```

The workflow uses the same `cargo xtask check-fuzz` path as local `make
fuzz-smoke`, but with a release-scale per-target budget and uploaded corpus/crash
artifacts. The release workflow queries GitHub Actions for a successful Release
Fuzz run whose `head_sha` is the exact release tag commit; if the final tag points
at a different SHA, the release fails until fuzz is rerun on that SHA.

## Local official release (after credentials exist)

On an exact `vX.Y.Z` tag, with full Xcode + a Developer ID identity:

```sh
xcrun notarytool store-credentials xpare-notary \
  --apple-id you@example.com --team-id TEAMID --password app-specific-password

make dist VERSION=1.2.3 \
  CERT_NAME="Developer ID Application: Your Name (TEAMID)" \
  NOTARY_PROFILE=xpare-notary

make github-release VERSION=1.2.3
```

`make dist` defaults to `shells/macos/xPare.entitlements` and fails if that
file is unavailable. `SIGN_ENTITLEMENTS` exists so CI can pass that checked file as
an absolute path; `release.sh` rejects any path that does not resolve to the checked
file. The signed payload must contain only
`com.apple.security.app-sandbox = true`. After signing, `release.sh` reads the
executable and bundle signatures back with `codesign -d --entitlements :-` and
fails if either signed payload is not minimal. `cargo xtask check-release-posture`
checks that this fail-closed release path remains wired into the script.

## First public binary prerequisites

- Lock the public bundle identifier (currently `com.xpare.app`).
- Decide universal vs. per-arch assets.
- Add Developer ID signing + notarization secrets (kept **out** of the repo).
- Keep official signing tied to `shells/macos/xPare.entitlements`; a signed
  app without the minimal App Sandbox-only payload is not an official xPare
  release.
- Verify Gatekeeper acceptance on a clean macOS machine.
- Keep the no-network, no-telemetry, no-clipboard-content-logging posture intact —
  enforced by `cargo xtask ci` on every release run.

## References

- GitHub Releases: <https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases>
- Apple Developer ID distribution (signing + notarization):
  <https://developer.apple.com/support/developer-id/>
