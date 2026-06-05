#!/usr/bin/env bash
# Package the SafetyStrip macOS shell into a runnable menu-bar .app bundle.
#
# Usage:
#   ./package-app.sh           # build + assemble dist/SafetyStrip.app (ad-hoc signed)
#   ./package-app.sh --sandbox # additionally sign with SafetyStrip.entitlements (App Sandbox)
#   ./package-app.sh --run     # build, assemble, then `open` the app
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
for arg in "$@"; do
    case "${arg}" in
        --sandbox) SANDBOX=1 ;;
        --run) RUN=1 ;;
        *) echo "unknown flag '${arg}' (use --sandbox and/or --run)" >&2; exit 2 ;;
    esac
done

APP_NAME="SafetyStrip"
BUNDLE_ID="com.safetystrip.app"
APP="${SCRIPT_DIR}/dist/${APP_NAME}.app"
CONTENTS="${APP}/Contents"

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
    <key>CFBundlePackageType</key>     <string>APPL</string>
    <key>CFBundleShortVersionString</key> <string>0.1.0</string>
    <key>CFBundleVersion</key>         <string>1</string>
    <key>LSMinimumSystemVersion</key>  <string>14.0</string>
    <!-- Menu-bar agent: no Dock icon, no main window. -->
    <key>LSUIElement</key>             <true/>
    <key>NSHighResolutionCapable</key> <true/>
    <key>NSPrincipalClass</key>        <string>NSApplication</string>
</dict>
</plist>
PLIST

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
