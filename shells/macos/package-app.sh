#!/usr/bin/env bash
# Package the SafetyStrip macOS shell into a runnable menu-bar .app bundle.
#
# Usage:
#   ./package-app.sh                 # build + assemble dist/SafetyStrip.app (ad-hoc signed)
#   ./package-app.sh --version 1.2.3 # stamp the bundle version (default: exact tag, else 0.1.0)
#   ./package-app.sh --sandbox       # additionally sign with SafetyStrip.entitlements (App Sandbox)
#   ./package-app.sh --run           # build, assemble, then `open` the app
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
VERSION=""
while [ "$#" -gt 0 ]; do
    case "${1}" in
        --sandbox) SANDBOX=1 ;;
        --run) RUN=1 ;;
        --version) shift; VERSION="${1:-}"; [ -n "${VERSION}" ] || { echo "--version needs a value" >&2; exit 2; } ;;
        *) echo "unknown flag '${1}' (use --version X.Y.Z, --sandbox, --run)" >&2; exit 2 ;;
    esac
    shift
done

APP_NAME="SafetyStrip"
BUNDLE_ID="com.safetystrip.app"
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
echo ">>> Building Rust core (safetystrip-ffi, release)…"
if [ -f "${HOME}/.cargo/env" ]; then
    # shellcheck disable=SC1091
    source "${HOME}/.cargo/env"
fi
( cd "${REPO_ROOT}" && cargo build -p safetystrip-ffi --release )

# --- 2. Build the Swift app (release) ------------------------------------------
echo ">>> swift build -c release --product ${APP_NAME}App"
cd "${SCRIPT_DIR}"
swift build -c release --product "${APP_NAME}App"
BIN="$(swift build -c release --product "${APP_NAME}App" --show-bin-path)/${APP_NAME}App"
[ -f "${BIN}" ] || { echo "ERROR: built binary not found at ${BIN}" >&2; exit 1; }

# --- 3. Assemble the .app bundle -----------------------------------------------
echo ">>> assembling ${APP}"
rm -rf "${APP}"
mkdir -p "${CONTENTS}/MacOS" "${CONTENTS}/Resources"
cp "${BIN}" "${CONTENTS}/MacOS/${APP_NAME}"

cat > "${CONTENTS}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>            <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>     <string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key>      <string>${BUNDLE_ID}</string>
    <key>CFBundleExecutable</key>      <string>${APP_NAME}</string>
    <key>CFBundleIconFile</key>        <string>AppIcon</string>
    <key>CFBundlePackageType</key>     <string>APPL</string>
    <key>CFBundleShortVersionString</key> <string>${VERSION}</string>
    <key>CFBundleVersion</key>         <string>${BUILD_VERSION}</string>
    <key>LSMinimumSystemVersion</key>  <string>14.0</string>
    <!-- Menu-bar agent: no Dock icon, no main window. -->
    <key>LSUIElement</key>             <true/>
    <key>NSHighResolutionCapable</key> <true/>
    <key>NSPrincipalClass</key>        <string>NSApplication</string>
</dict>
</plist>
PLIST

# --- 3b. App icon (best-effort: needs python3 + iconutil) ----------------------
if command -v iconutil >/dev/null 2>&1 && command -v python3 >/dev/null 2>&1; then
    icon_tmp="$(mktemp -d)"
    if python3 "${SCRIPT_DIR}/generate-icon.py" "${icon_tmp}/AppIcon.iconset" >/dev/null 2>&1 \
        && iconutil -c icns "${icon_tmp}/AppIcon.iconset" -o "${CONTENTS}/Resources/AppIcon.icns" >/dev/null 2>&1; then
        echo ">>> generated AppIcon.icns"
    else
        echo ">>> icon generation skipped (generator/iconutil failed)"
    fi
    rm -rf "${icon_tmp}"
else
    echo ">>> icon generation skipped (python3/iconutil not found)"
fi

# --- 4. Sign (Apple Silicon requires a valid signature) ------------------------
if [ "${SANDBOX}" -eq 1 ]; then
    echo ">>> ad-hoc signing WITH App Sandbox entitlements"
    codesign --force --sign - \
        --identifier "${BUNDLE_ID}" \
        --entitlements "${SCRIPT_DIR}/SafetyStrip.entitlements" \
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
