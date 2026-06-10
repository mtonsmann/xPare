# Exec Plan 0003 — macOS release plumbing

Status: **active** · Started: 2026-06-05

## Goal

Add the repository plumbing for xPare's open-source macOS release model
**without** claiming official downloadable binaries before Developer ID signing and
notarization credentials exist. Ported and adapted from the upstream FormatStripper
release track onto the xPare tree.

## Key adaptation vs. upstream

The upstream shell loaded a **dynamic** `libformatstripper_ffi.dylib` and embedded
it under `Contents/Frameworks`, with a Makefile `abi-symbols` check over the dylib's
exports. The xPare shell **statically links** `libxpare_ffi.a` into the
Swift executable, so:

- There is **no embedded dylib** to copy, relocate, or sign separately.
- The bundle is the Swift Mach-O + `Info.plist` + `AppIcon.icns`; one Developer ID
  signature covers the executable and the bundle.
- The `nm`-over-dylib ABI-symbol check does not apply; ABI drift is already caught by
  `cargo xtask check-abi` (cbindgen regenerates the header from source).

## Scope

- Generate an `AppIcon.icns` at build time from a dependency-free Python helper
  (`shells/macos/generate-icon.py`) so no binary artwork is committed.
- Version-stamp the bundle `Info.plist` (`--version` flag on `package-app.sh`).
- Add `shells/macos/release.sh` with `preview` / `dist` / `github-release`
  subcommands, and thin `make` targets that delegate to it (Makefile stays
  ergonomic; the script owns the how).
- Add `.github/workflows/release.yml`: tag-triggered `validate` + unsigned-preview
  artifact, plus a `publish-official` job gated on a repo variable and Apple secrets.
- Keep Apple account / certificate / notary / GitHub secret setup **out** of the
  repo; ignore `dist/release/`.

## Out of scope

- Paying for Apple Developer Program membership; storing Developer ID credentials in
  the repo; publishing an unsigned artifact as an official binary.
- Telemetry, network features, paste simulation, Windows/Linux shells, distribution
  channels beyond GitHub Releases + build-from-source.

## Work plan

1. `generate-icon.py` (pure stdlib PNG → `iconutil`). **Done.**
2. `package-app.sh --version X.Y.Z`, stamps `Info.plist` and best-effort generates
   the icon (skips gracefully if `iconutil`/`python3` absent). **Done.**
3. `release.sh preview` → unsigned/ad-hoc zip + `.sha256` under `dist/release/`.
   **Done.**
4. `release.sh dist` → Developer ID sign (hardened runtime + checked App Sandbox
   entitlements) → verify the signed payload is minimal → notarize → staple → zip
   → checksum → verify. Gated; refuses without `CERT_NAME`, the checked entitlements
   file, and a real `vX.Y.Z`; alternate entitlement paths are rejected. **Done
   (gated; full signing untested here).**
5. `release.sh github-release` → `gh release create` with the signed zip + checksum.
   **Done.**
6. `make preview` / `dist` / `github-release` / `check-version` / `clean-release`
   delegating targets. **Done.**
7. `.github/workflows/release.yml` (validate + gated publish-official). **Done.**
8. `docs/release-model.md` distinguishing source builds, unsigned previews, and
   future Developer ID releases. **Done.**

## Decision log

- 2026-06-05: Adapt the upstream pipeline to **static linking** — drop the embedded
  dylib + `abi-symbols` steps; the single Mach-O is signed in place.
- 2026-06-05: Keep official publication gated behind
  `XPARE_ENABLE_OFFICIAL_RELEASE=true` + Apple secrets. Tag runs always build
  an unsigned preview artifact; they never publish it as an official download.
- 2026-06-05: Put the release logic in `release.sh` (not the Makefile) to preserve
  the local "Makefile delegates; xtask/scripts are authoritative" convention.
- 2026-06-06: Security fix: official Developer ID signing must use the checked-in
  App Sandbox entitlements and verify the signed payload remains minimal. Local dev
  preview bundling stays unsigned/ad-hoc and is not an official release channel.

## Acceptance criteria

- `make app` builds `shells/macos/dist/xPare.app`. **Verifiable locally.**
- `make preview` writes an explicitly unsigned preview zip + checksum under
  `dist/release/`. **Verifiable locally (ad-hoc signing; no Apple account).**
- `make dist` refuses without an exact `vX.Y.Z` (or `VERSION=`) and `CERT_NAME`.
  It also refuses if the checked signing entitlements file is unavailable or if an
  alternate entitlements path is supplied, and verifies the signed payload is minimal
  after codesign. **Gates verifiable; full sign/notarize requires Developer ID + full
  Xcode (not available in the current Command-Line-Tools-only environment).**
- Pull-request CI requires no Apple credentials.
- `cargo xtask ci` and `swift build` stay green.

## Follow-ups (not blocking)

- Verify Gatekeeper acceptance on a clean machine once a Developer ID exists.
- Consider a universal (arm64 + x86_64) build vs. per-arch assets.
- Add a Homebrew Cask once signed/notarized releases are published.
