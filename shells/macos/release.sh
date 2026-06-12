#!/usr/bin/env bash
# Release packaging for the xPare macOS shell.
#
# The Swift app STATICALLY links libxpare_ffi.a, so the bundle has no
# embedded dylib to sign or relocate — packaging is just the assembled .app plus
# (for an official release) a Developer ID signature, notarization, and a stapled
# ticket. The heavy assembly lives in package-app.sh; this script wraps it for the
# release surface and stays out of the everyday dev path.
#
# Subcommands:
#   preview          Assemble + zip an explicitly UNSIGNED preview + checksum.
#                    Needs no Apple account; this is the path CI and local testing use.
#   dist             Developer ID sign with the checked App Sandbox entitlements
#                    -> notarize -> staple -> zip -> checksum -> verify.
#                    GATED: requires CERT_NAME, NOTARY_PROFILE, the entitlements
#                    file, and a real vX.Y.Z version. Cannot run without an Apple
#                    Developer ID; never produces an un-notarized official asset.
#   github-release   Verify the zip is stapled, then create a draft release with
#                    the zip, checksums, and staged SBOM via `gh` (prerelease for
#                    hyphenated versions).
#
# Environment:
#   VERSION=X.Y.Z              Release version. `dist` requires it (or an exact vX.Y.Z
#                              git tag); `preview` falls back to a dev label.
#   CERT_NAME="Developer ID Application: Name (TEAMID)"   Required for `dist`.
#   NOTARY_PROFILE=name        `xcrun notarytool store-credentials` profile; required
#                              for `dist` (official assets must be notarized).
#   NOTARY_KEYCHAIN=path       Optional keychain path for NOTARY_PROFILE; official
#                              CI passes the temporary signing keychain so deleting
#                              that keychain removes both signing and notary material.
#   SIGN_ENTITLEMENTS=path     Entitlements for the Developer ID signature. Defaults
#                              to shells/macos/xPare.entitlements; dist rejects
#                              any path that does not resolve to that checked file.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

APP_NAME="xPare"
APP="${SCRIPT_DIR}/dist/${APP_NAME}.app"
EXE="${APP}/Contents/MacOS/${APP_NAME}"
RELEASE_DIR="${REPO_ROOT}/dist/release"
# Asset names carry the build arch. Releases are arm64-only for 1.0 (built on
# Apple Silicon, so this resolves to arm64); x86_64 and universal assets are
# deferred — see docs/release-model.md.
ARCH="$(uname -m)"
DEFAULT_SIGN_ENTITLEMENTS="${SCRIPT_DIR}/xPare.entitlements"

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

# Semver-shaped: X.Y.Z with an optional dotted prerelease (1.0.0-rc.1).
valid_version() { [[ "$1" =~ ^[0-9]+(\.[0-9]+){2}(-[0-9A-Za-z]+(\.[0-9A-Za-z]+)*)?$ ]]; }

canonical_path() {
    local path="$1"
    local dir
    local base
    dir="$(cd "$(dirname "${path}")" && pwd -P)" || return 1
    base="$(basename "${path}")"
    printf '%s/%s' "${dir}" "${base}"
}

minimal_entitlements_error() {
    local target="$1"
    local label="$2"
    local sandbox
    if ! sandbox="$(/usr/libexec/PlistBuddy -c 'Print :com.apple.security.app-sandbox' "${target}" 2>/dev/null)"; then
        echo "${label} is missing com.apple.security.app-sandbox=true."
        return 1
    fi
    if [[ "${sandbox}" != "true" ]]; then
        echo "${label} has com.apple.security.app-sandbox=${sandbox}; expected true."
        return 1
    fi

    # Minimality: delete the one allowed key from a scratch copy and require the
    # remaining dict to print empty. A line-based count of `Print` output would
    # also count the keys of nested dict payloads (and could be fooled by
    # multi-line string values), so the check works structurally instead. The
    # authoritative XML-level scan is `cargo xtask check-entitlements`; this is
    # the in-script fail-closed mirror for the signing path.
    local scratch
    scratch="$(mktemp "${TMPDIR:-/tmp}/xpare-entitlements-scratch.XXXXXX")" || {
        echo "could not create a scratch copy to verify ${label}."
        return 1
    }
    local remainder=""
    if ! cp "${target}" "${scratch}" \
        || ! /usr/libexec/PlistBuddy -c 'Delete :com.apple.security.app-sandbox' "${scratch}" 2>/dev/null \
        || ! remainder="$(/usr/libexec/PlistBuddy -c 'Print' "${scratch}" 2>/dev/null)"; then
        rm -f "${scratch}"
        echo "could not read ${label}."
        return 1
    fi
    rm -f "${scratch}"
    if [[ "${remainder}" != $'Dict {\n}' ]]; then
        echo "${label} must contain only com.apple.security.app-sandbox=true; found extra entitlement content:"
        printf '%s\n' "${remainder}"
        return 1
    fi
}

require_minimal_entitlements() {
    local target="$1"
    local label="$2"
    local err
    if ! err="$(minimal_entitlements_error "${target}" "${label}")"; then
        die "${err}"
    fi
}

resolve_sign_entitlements() {
    local path="${SIGN_ENTITLEMENTS:-${DEFAULT_SIGN_ENTITLEMENTS}}"
    if [[ "${path}" != /* ]]; then
        path="${PWD}/${path}"
    fi
    [ -f "${path}" ] || die "dist needs signing entitlements at ${path} (default: ${DEFAULT_SIGN_ENTITLEMENTS})."

    local resolved
    local default_resolved
    resolved="$(canonical_path "${path}")" || die "could not resolve signing entitlements path ${path}."
    default_resolved="$(canonical_path "${DEFAULT_SIGN_ENTITLEMENTS}")" || die "could not resolve default signing entitlements path ${DEFAULT_SIGN_ENTITLEMENTS}."
    [[ "${resolved}" == "${default_resolved}" ]] || die "dist must sign with the checked entitlements at ${default_resolved}; refusing SIGN_ENTITLEMENTS=${resolved}."

    require_minimal_entitlements "${resolved}" "signing entitlements ${resolved}"
    printf '%s' "${resolved}"
}

verify_signed_entitlements() {
    local target="$1"
    local actual
    actual="$(mktemp "${TMPDIR:-/tmp}/xpare-entitlements.XXXXXX")"
    # `--entitlements - --xml` requests the XML plist form PlistBuddy can parse
    # (the legacy `:-` destination is deprecated, and a bare `-` emits DER on
    # current toolchains).
    if ! codesign -d --entitlements - --xml "${target}" > "${actual}" 2>/dev/null; then
        rm -f "${actual}"
        die "could not read signed entitlements from ${target}."
    fi
    local err
    if ! err="$(minimal_entitlements_error "${actual}" "signed entitlements for ${target}")"; then
        echo "release.sh: signed entitlements for ${target}:" >&2
        sed 's/^/release.sh:   /' "${actual}" >&2
        rm -f "${actual}"
        die "${err}"
    fi
    rm -f "${actual}"
    echo ">>> verified minimal App Sandbox entitlement on ${target}"
}

# The hardened runtime is a posture requirement and a notarization prerequisite;
# `--options runtime` at sign time must be visible in the finished signature's
# CodeDirectory flags, not just requested.
verify_hardened_runtime() {
    local target="$1"
    local flags_line
    flags_line="$(codesign -d --verbose "${target}" 2>&1 | grep '^CodeDirectory' || true)"
    if [[ "${flags_line}" != *"runtime"* ]]; then
        die "hardened runtime flag missing on ${target} (CodeDirectory: ${flags_line:-unreadable})."
    fi
    echo ">>> verified hardened runtime flag on ${target}"
}

# Submit the signed bundle for notarization and staple the ticket. Trusting the
# notarytool exit code alone is not enough — a completed-but-Invalid submission
# can still exit 0 — so this parses the machine-readable verdict and requires
# status Accepted, printing the notarization log on any other outcome.
notarize_and_staple() {
    local notary_zip="${RELEASE_DIR}/${APP_NAME}-notary-submission.zip"
    mkdir -p "${RELEASE_DIR}"
    rm -f "${notary_zip}"
    ditto -c -k --keepParent "${APP}" "${notary_zip}"

    echo ">>> notarizing (xcrun notarytool submit --wait)"
    local submit_json submit_ok=1
    submit_json="$(mktemp "${TMPDIR:-/tmp}/xpare-notary.XXXXXX")"
    local -a notary_keychain_args=()
    if [ -n "${NOTARY_KEYCHAIN:-}" ]; then
        [ -f "${NOTARY_KEYCHAIN}" ] || die "NOTARY_KEYCHAIN does not exist: ${NOTARY_KEYCHAIN}"
        notary_keychain_args=(--keychain "${NOTARY_KEYCHAIN}")
    fi
    if ! xcrun notarytool submit "${notary_zip}" --keychain-profile "${NOTARY_PROFILE}" \
        "${notary_keychain_args[@]}" --wait --output-format json > "${submit_json}"; then
        submit_ok=0
    fi

    local status submission_id
    status="$(plutil -extract status raw -o - "${submit_json}" 2>/dev/null || true)"
    submission_id="$(plutil -extract id raw -o - "${submit_json}" 2>/dev/null || true)"
    rm -f "${submit_json}"
    if [ "${submit_ok}" -ne 1 ] || [ "${status}" != "Accepted" ]; then
        echo "release.sh: notarization status '${status:-<none>}' (submission id: ${submission_id:-<none>})." >&2
        if [ -n "${submission_id}" ]; then
            echo "release.sh: notarization log for ${submission_id}:" >&2
            xcrun notarytool log "${submission_id}" --keychain-profile "${NOTARY_PROFILE}" \
                "${notary_keychain_args[@]}" >&2 \
                || echo "release.sh: could not fetch the notarization log." >&2
        fi
        rm -f "${notary_zip}"
        die "notarization did not return status Accepted."
    fi

    xcrun stapler staple "${APP}"
    xcrun stapler validate "${APP}"
    # Remove the pre-staple submission zip: it holds the un-stapled bundle and
    # must never sit in the release dir where it could be mistaken for (or
    # checksummed alongside) the real asset.
    rm -f "${notary_zip}"
    echo ">>> notarized (submission ${submission_id}) and stapled"
}

# The uploadable zip must contain the stapled app — catches a zip produced
# before stapling or by a tampered dist run.
verify_zip_stapled() {
    local zip="$1"
    local unpack
    unpack="$(mktemp -d "${TMPDIR:-/tmp}/xpare-staple-check.XXXXXX")"
    if ! ditto -x -k "${zip}" "${unpack}"; then
        rm -rf "${unpack}"
        die "could not unpack ${zip} to verify stapling."
    fi
    if ! xcrun stapler validate "${unpack}/${APP_NAME}.app"; then
        rm -rf "${unpack}"
        die "${zip} contains an un-stapled app; rerun 'release.sh dist' with NOTARY_PROFILE."
    fi
    rm -rf "${unpack}"
    echo ">>> verified stapled notarization ticket inside ${zip}"
}

# Assemble the .app via package-app.sh at the requested version (ad-hoc signed).
# Extra args are forwarded to package-app.sh (dist passes --require-icon).
assemble() {
    local version="$1"
    shift
    echo ">>> assembling ${APP_NAME}.app (version ${version})"
    ( cd "${SCRIPT_DIR}" && ./package-app.sh --version "${version}" "$@" )
    [ -d "${APP}" ] || die "expected bundle not found at ${APP}"
}

zip_app() {
    local zip="$1"
    local dir base
    dir="$(dirname "${zip}")"
    base="$(basename "${zip}")"
    mkdir -p "${dir}"
    rm -f "${zip}"
    ditto -c -k --keepParent "${APP}" "${zip}"
    # Hash the basename from inside the directory so the .sha256 file verifies
    # anywhere with `shasum -c` — an absolute path baked into the file would
    # only ever verify on the machine that built it.
    ( cd "${dir}" && shasum -a 256 "${base}" > "${base}.sha256" )
    # Aggregate manifest over every zip currently in the release dir, for
    # one-shot `shasum -c SHA256SUMS` verification of a download set.
    ( cd "${RELEASE_DIR}" && shasum -a 256 -- *.zip > SHA256SUMS )
    echo ">>> wrote ${zip} (+ ${base}.sha256, SHA256SUMS)"
}

cmd="${1:-}"
case "${cmd}" in
    preview)
        version="$(resolve_version)"
        # Preview archives are explicitly-unofficial test artifacts whose names
        # say so, so the version is a label rather than a contract: untagged
        # builds fall back to a dev marker (which still matches valid_version's
        # prerelease shape) instead of failing strict validation.
        [ -n "${version}" ] || version="0.0.0-dev"
        assemble "${version}"
        zip_app "${RELEASE_DIR}/${APP_NAME}-v${version}-macos-${ARCH}-unsigned-preview.zip"
        echo ">>> UNSIGNED preview ready. This is not a Developer ID release."
        ;;

    dist)
        version="$(resolve_version)"
        [ -n "${version}" ] || die "dist needs VERSION=X.Y.Z or an exact vX.Y.Z tag."
        valid_version "${version}" || die "VERSION must look like X.Y.Z with an optional dotted prerelease, e.g. 1.0.0-rc.1 (got '${version}')."
        sign_entitlements="$(resolve_sign_entitlements)"
        [ -n "${CERT_NAME:-}" ] || die "dist needs CERT_NAME='Developer ID Application: ... (TEAMID)'."
        # Fail closed: a signed-but-un-notarized zip under the official asset
        # name is indistinguishable from a real release downstream. `preview`
        # is the path that needs no Apple credentials.
        [ -n "${NOTARY_PROFILE:-}" ] || die "dist needs NOTARY_PROFILE (see 'xcrun notarytool store-credentials'); official assets must be notarized — use 'preview' for unsigned archives."

        # Official bundles must carry the app icon, so its generation is fatal here.
        assemble "${version}" --require-icon

        echo ">>> Developer ID signing (hardened runtime + App Sandbox entitlements)"
        # Sign the inner Mach-O first, then the bundle (inside-out).
        codesign --force --options runtime --timestamp \
            --entitlements "${sign_entitlements}" --sign "${CERT_NAME}" "${EXE}"
        codesign --force --options runtime --timestamp \
            --entitlements "${sign_entitlements}" --sign "${CERT_NAME}" "${APP}"
        codesign --verify --strict --verbose=2 "${EXE}"
        codesign --verify --strict --verbose=2 "${APP}"
        verify_signed_entitlements "${EXE}"
        verify_signed_entitlements "${APP}"
        verify_hardened_runtime "${EXE}"
        verify_hardened_runtime "${APP}"

        notarize_and_staple

        zip_app "${RELEASE_DIR}/${APP_NAME}-v${version}-macos-${ARCH}.zip"
        echo ">>> verifying Gatekeeper acceptance"
        codesign --verify --deep --strict --verbose=2 "${APP}"
        # Notarization is mandatory above, so Gatekeeper acceptance is a hard
        # requirement — a failure here means the release is not shippable.
        spctl --assess --type execute --verbose "${APP}"
        ;;

    github-release)
        version="$(resolve_version)"
        valid_version "${version}" || die "github-release needs VERSION=X.Y.Z or an exact vX.Y.Z tag."
        command -v gh >/dev/null 2>&1 || die "the gh CLI is required for github-release."
        zip="${RELEASE_DIR}/${APP_NAME}-v${version}-macos-${ARCH}.zip"
        [ -f "${zip}" ] || die "${zip} is missing; run 'release.sh dist' first."
        sums="${RELEASE_DIR}/SHA256SUMS"
        [ -f "${sums}" ] || die "${sums} is missing; run 'release.sh dist' first."
        sbom="${RELEASE_DIR}/${APP_NAME}-v${version}-sbom.spdx.json"
        [ -f "${sbom}" ] || die "${sbom} is missing; generate the release SBOM before github-release."
        verify_zip_stapled "${zip}"

        # Draft first so a human reviews and publishes; hyphenated versions
        # (1.0.0-rc.1) are marked prerelease. This command creates a draft once
        # and never mutates release assets after that. A draft-status check before
        # upload would be racy with a maintainer publishing the draft, so existing
        # releases are a hard stop rather than a clobber target.
        # The aggregate SHA256SUMS rides along with the per-file checksum, and
        # the SBOM is included in the same create command. CI's provenance
        # attestation covers SHA256SUMS*, so the published asset set and the
        # attested subject set stay in sync.
        if gh release view "v${version}" >/dev/null 2>&1; then
            die "release v${version} already exists; refusing to replace release assets. Delete the draft release before rerunning, or create a new tag for a corrected public release."
        else
            create_flags=(--draft)
            case "${version}" in
                *-*) create_flags+=(--prerelease) ;;
            esac
            gh release create "v${version}" "${zip}" "${zip}.sha256" "${sums}" "${sbom}" \
                --title "${APP_NAME} ${version}" --generate-notes --verify-tag \
                "${create_flags[@]}"
        fi
        ;;

    ""|-h|--help)
        # Print the header comment block (everything up to the first non-# line)
        # so the usage text cannot drift out of sync with a fixed line range.
        awk 'NR > 1 && !/^#/ { exit } NR > 1 { sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"
        ;;
    *)
        die "unknown subcommand '${cmd}' (use preview | dist | github-release)"
        ;;
esac
