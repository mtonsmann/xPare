#!/usr/bin/env bash
# Package the xPare macOS shell into a runnable menu-bar .app bundle.
#
# Usage:
#   ./package-app.sh                 # build + assemble dist/xPare.app (ad-hoc signed)
#   ./package-app.sh --version 1.2.3 # stamp the bundle version (default: exact tag, else 0.1.0)
#   ./package-app.sh --sandbox       # additionally sign with xPare.entitlements (App Sandbox)
#   ./package-app.sh --run           # build, assemble, then `open` the app
#   ./package-app.sh --require-icon  # fail if the app icon cannot be generated
#                                    # (release.sh dist passes this; official bundles
#                                    # must not ship icon-less)
#
# For release packaging (unsigned preview / Developer ID), use release.sh, which
# wraps this script.
#
# Why a hand-assembled bundle?
#   A SwiftUI `MenuBarExtra` app must run from a bundle with `LSUIElement = true`
#   to behave as a menu-bar-only agent (no Dock icon, no main window). Full Xcode
#   would produce this automatically; with Command-Line-Tools we assemble the
#   minimal bundle by hand. Apple Silicon requires a valid signature, so we ad-hoc
#   sign the finished bundle (`codesign -s -`).
#
#   Ad-hoc signing WITHOUT entitlements runs unsandboxed — the most reliable path
#   for a local functional test. `--sandbox` signs with the checked-in minimal
#   entitlements (App Sandbox); note that a fully sandboxed, distributable build
#   ultimately needs a Developer ID identity + notarization (out of scope here).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

SANDBOX=0
RUN=0
REQUIRE_ICON=0
VERSION=""
while [ "$#" -gt 0 ]; do
    case "${1}" in
        --sandbox) SANDBOX=1 ;;
        --run) RUN=1 ;;
        --require-icon) REQUIRE_ICON=1 ;;
        --version) shift; VERSION="${1:-}"; [ -n "${VERSION}" ] || { echo "--version needs a value" >&2; exit 2; } ;;
        *) echo "unknown flag '${1}' (use --version X.Y.Z, --sandbox, --run, --require-icon)" >&2; exit 2 ;;
    esac
    shift
done

APP_NAME="xPare"
# SwiftPM product (Package.swift `.executable(name: "XPareApp", ...)`). Distinct
# from APP_NAME on purpose: the bundle/display name is branded lowercase-x while
# the Swift product/target keeps the type-style capital — do not derive one from
# the other (deriving "${APP_NAME}App" is what broke the first rc.1 release run).
PRODUCT_NAME="XPareApp"
BUNDLE_ID="com.xpare.app"
APP="${SCRIPT_DIR}/dist/${APP_NAME}.app"
CONTENTS="${APP}/Contents"

# Bundle version metadata. --version wins; else an exact vX.Y.Z tag; else 0.1.0.
# The build number is the commit count (else 1).
if [ -z "${VERSION}" ]; then
    tag="$(git -C "${REPO_ROOT}" describe --tags --exact-match --match 'v[0-9]*' 2>/dev/null || true)"
    VERSION="${tag#v}"
fi
[ -n "${VERSION}" ] || VERSION="0.1.0"
BUILD_VERSION="$(git -C "${REPO_ROOT}" rev-list --count HEAD 2>/dev/null || printf '1')"

# --- 1. Build the Rust core staticlib (release) --------------------------------
echo ">>> Building Rust core (xpare-ffi, release)…"
if [ -f "${HOME}/.cargo/env" ]; then
    # shellcheck disable=SC1091
    source "${HOME}/.cargo/env"
fi
# --locked: a packaged artifact must build against the committed lockfile, never
# a silently floated dependency graph.
( cd "${REPO_ROOT}" && cargo build -p xpare-ffi --release --locked )

# --- 2. Build the Swift app (release) ------------------------------------------
echo ">>> swift build -c release --product ${PRODUCT_NAME}"
cd "${SCRIPT_DIR}"
swift build -c release --product "${PRODUCT_NAME}"
BIN="$(swift build -c release --product "${PRODUCT_NAME}" --show-bin-path)/${PRODUCT_NAME}"
[ -f "${BIN}" ] || { echo "ERROR: built binary not found at ${BIN}" >&2; exit 1; }

# --- 3. Assemble the .app bundle -----------------------------------------------
echo ">>> assembling ${APP}"
rm -rf "${APP}"
mkdir -p "${CONTENTS}/MacOS" "${CONTENTS}/Resources"
cp "${BIN}" "${CONTENTS}/MacOS/${APP_NAME}"

# --- 3a. App icon (needs python3 + iconutil) ------------------------------------
# Runs before the Info.plist write so the plist declares CFBundleIconFile only
# when AppIcon.icns actually exists — bundle metadata must never point at a
# missing resource. Official packaging (release.sh dist passes --require-icon)
# must not ship icon-less, so failure is fatal there; the ad-hoc/dev and
# unsigned-preview paths stay best-effort.
ICON_GENERATED=0
if command -v iconutil >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
    icon_tmp="$(mktemp -d)"
    if python3 "${SCRIPT_DIR}/generate-icon.py" "${icon_tmp}/AppIcon.iconset" >/dev/null 2>&1 \
        && iconutil -c icns "${icon_tmp}/AppIcon.iconset" -o "${CONTENTS}/Resources/AppIcon.icns" >/dev/null 2>&1; then
        echo ">>> generated AppIcon.icns"
        ICON_GENERATED=1
    fi
    rm -rf "${icon_tmp}"
fi
ICON_PLIST_ENTRY=""
if [ "${ICON_GENERATED}" -eq 1 ]; then
    ICON_PLIST_ENTRY="    <key>CFBundleIconFile</key>        <string>AppIcon</string>"
elif [ "${REQUIRE_ICON}" -eq 1 ]; then
    echo "ERROR: --require-icon set but icon generation failed (needs python3 + iconutil)." >&2
    exit 1
else
    echo ">>> icon generation skipped (python3/iconutil missing or failed; bundle ships without an icon)"
fi

cat > "${CONTENTS}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key> <string>en</string>
    <key>CFBundleInfoDictionaryVersion</key> <string>6.0</string>
    <key>CFBundleName</key>            <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>     <string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key>      <string>${BUNDLE_ID}</string>
    <key>CFBundleExecutable</key>      <string>${APP_NAME}</string>
${ICON_PLIST_ENTRY}
    <key>CFBundlePackageType</key>     <string>APPL</string>
    <key>CFBundleShortVersionString</key> <string>${VERSION}</string>
    <key>CFBundleVersion</key>         <string>${BUILD_VERSION}</string>
    <key>LSApplicationCategoryType</key> <string>public.app-category.utilities</string>
    <key>LSMinimumSystemVersion</key>  <string>14.0</string>
    <!-- Menu-bar agent: no Dock icon, no main window. -->
    <key>LSUIElement</key>             <true/>
    <key>NSHighResolutionCapable</key> <true/>
    <key>NSHumanReadableCopyright</key> <string>Copyright © 2026 Marcus Tonsmann</string>
    <key>NSPrincipalClass</key>        <string>NSApplication</string>
</dict>
</plist>
PLIST

# --- 4. Sign (Apple Silicon requires a valid signature) ------------------------
if [ "${SANDBOX}" -eq 1 ]; then
    echo ">>> ad-hoc signing WITH App Sandbox entitlements"
    codesign --force --sign - \
        --identifier "${BUNDLE_ID}" \
        --entitlements "${SCRIPT_DIR}/xPare.entitlements" \
        --options runtime \
        "${APP}"
else
    echo ">>> ad-hoc signing (unsandboxed — reliable for local testing)"
    codesign --force --sign - --identifier "${BUNDLE_ID}" "${APP}"
fi

codesign --verify --verbose=2 "${APP}"
echo ">>> Built: ${APP}"

# --- 5. Optionally launch ------------------------------------------------------
if [ "${RUN}" -eq 1 ]; then
    echo ">>> open ${APP}  (look for the ✂ scissors icon in your menu bar)"
    open "${APP}"
else
    echo ">>> To run it:  open '${APP}'"
    echo "    Then look for the ✂ scissors icon in the menu bar."
fi
