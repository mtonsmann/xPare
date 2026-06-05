#!/usr/bin/env bash
# Release packaging for the SafetyStrip macOS shell.
#
# The Swift app STATICALLY links libsafetystrip_ffi.a, so the bundle has no
# embedded dylib to sign or relocate — packaging is just the assembled .app plus
# (for an official release) a Developer ID signature, notarization, and a stapled
# ticket. The heavy assembly lives in package-app.sh; this script wraps it for the
# release surface and stays out of the everyday dev path.
#
# Subcommands:
#   preview          Assemble + zip an explicitly UNSIGNED preview + checksum.
#                    Needs no Apple account; this is the path CI and local testing use.
#   dist             Developer ID sign -> notarize -> staple -> zip -> checksum -> verify.
#                    GATED: requires CERT_NAME (and NOTARY_PROFILE to notarize) plus a
#                    real vX.Y.Z version. Cannot run without an Apple Developer ID.
#   github-release   Upload the signed release zip + checksum via `gh`.
#
# Environment:
#   VERSION=X.Y.Z              Release version. `dist` requires it (or an exact vX.Y.Z
#                              git tag); `preview` falls back to a dev label.
#   CERT_NAME="Developer ID Application: Name (TEAMID)"   Required for `dist`.
#   NOTARY_PROFILE=name        `xcrun notarytool store-credentials` profile; required to notarize.
#   SIGN_ENTITLEMENTS=path     Optional entitlements for the Developer ID signature.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

APP_NAME="SafetyStrip"
APP="${SCRIPT_DIR}/dist/${APP_NAME}.app"
EXE="${APP}/Contents/MacOS/${APP_NAME}"
RELEASE_DIR="${REPO_ROOT}/dist/release"
ARCH="$(uname -m)"

die() { echo "release.sh: $*" >&2; exit 1; }

# Resolve the version: explicit VERSION wins, else an exact vX.Y.Z tag, else "".
resolve_version() {
    local v="${VERSION:-}"
    if [ -z "${v}" ]; then
        v="$(git -C "${REPO_ROOT}" describe --tags --exact-match --match 'v[0-9]*' 2>/dev/null || true)"
        v="${v#v}"
    fi
    printf '%s' "${v}"
}

valid_version() { [[ "$1" =~ ^[0-9]+(\.[0-9]+){2}([.-][A-Za-z0-9]+)?$ ]]; }

# Assemble the .app via package-app.sh at the requested version (ad-hoc signed).
assemble() {
    echo ">>> assembling ${APP_NAME}.app (version ${1})"
    ( cd "${SCRIPT_DIR}" && ./package-app.sh --version "${1}" )
    [ -d "${APP}" ] || die "expected bundle not found at ${APP}"
}

zip_app() {
    mkdir -p "${RELEASE_DIR}"
    rm -f "$1"
    ditto -c -k --keepParent "${APP}" "$1"
    shasum -a 256 "$1" > "$1.sha256"
    echo ">>> wrote $1 (+ .sha256)"
}

cmd="${1:-}"
case "${cmd}" in
    preview)
        version="$(resolve_version)"
        [ -n "${version}" ] || version="0.0.0-dev"
        assemble "${version}"
        zip_app "${RELEASE_DIR}/${APP_NAME}-v${version}-macos-${ARCH}-unsigned-preview.zip"
        echo ">>> UNSIGNED preview ready. This is not a Developer ID release."
        ;;

    dist)
        version="$(resolve_version)"
        [ -n "${version}" ] || die "dist needs VERSION=X.Y.Z or an exact vX.Y.Z tag."
        valid_version "${version}" || die "VERSION must look like X.Y.Z or X.Y.Z-suffix (got '${version}')."
        [ -n "${CERT_NAME:-}" ] || die "dist needs CERT_NAME='Developer ID Application: ... (TEAMID)'."

        assemble "${version}"

        echo ">>> Developer ID signing (hardened runtime)"
        ent=()
        [ -n "${SIGN_ENTITLEMENTS:-}" ] && ent=(--entitlements "${SIGN_ENTITLEMENTS}")
        # Sign the inner Mach-O first, then the bundle (inside-out).
        codesign --force --options runtime --timestamp "${ent[@]}" --sign "${CERT_NAME}" "${EXE}"
        codesign --force --options runtime --timestamp --sign "${CERT_NAME}" "${APP}"
        codesign --verify --strict --verbose=2 "${EXE}"

        local_zip="${RELEASE_DIR}/${APP_NAME}-v${version}-notary.zip"
        mkdir -p "${RELEASE_DIR}"; rm -f "${local_zip}"
        ditto -c -k --keepParent "${APP}" "${local_zip}"

        if [ -n "${NOTARY_PROFILE:-}" ]; then
            echo ">>> notarizing (xcrun notarytool submit --wait)"
            xcrun notarytool submit "${local_zip}" --keychain-profile "${NOTARY_PROFILE}" --wait
            xcrun stapler staple "${APP}"
            xcrun stapler validate "${APP}"
        else
            echo ">>> NOTARY_PROFILE unset — signed but NOT notarized/stapled (incomplete release)." >&2
        fi

        zip_app "${RELEASE_DIR}/${APP_NAME}-v${version}-macos-${ARCH}.zip"
        echo ">>> verifying Gatekeeper acceptance"
        codesign --verify --deep --strict --verbose=2 "${APP}"
        spctl --assess --type execute --verbose "${APP}" || \
            echo ">>> spctl assessment failed (expected until notarization completes)." >&2
        ;;

    github-release)
        version="$(resolve_version)"
        valid_version "${version}" || die "github-release needs VERSION=X.Y.Z or an exact vX.Y.Z tag."
        command -v gh >/dev/null 2>&1 || die "the gh CLI is required for github-release."
        zip="${RELEASE_DIR}/${APP_NAME}-v${version}-macos-${ARCH}.zip"
        [ -f "${zip}" ] || die "${zip} is missing; run 'release.sh dist' first."
        gh release create "v${version}" "${zip}" "${zip}.sha256" \
            --title "${APP_NAME} ${version}" --generate-notes --verify-tag
        ;;

    ""|-h|--help)
        sed -n '2,30p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
        ;;
    *)
        die "unknown subcommand '${cmd}' (use preview | dist | github-release)"
        ;;
esac
