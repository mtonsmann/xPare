#!/usr/bin/env bash
# Build (and optionally test) the SafetyStrip macOS shell.
#
# Usage:
#   ./build.sh            # build the Rust core (release) + swift build
#   ./build.sh test       # the above, then swift test
#   ./build.sh release    # build core + swift build -c release
#
# Why this script exists
# ----------------------
# 1. The Swift package links a *prebuilt* Rust staticlib. This script builds it
#    first into target/release so the linker path in Package.swift resolves.
# 2. In a Command-Line-Tools-only environment (no full Xcode), `swift test` needs
#    to find swift-testing's Testing.framework and its companion
#    lib_TestingInterop.dylib, which are not on the default runtime search path.
#    We pass the right -F / -rpath flags so the test bundle can load them.
#    With full Xcode installed these flags are harmless (the paths just won't be
#    used), so the script works in both environments.
set -euo pipefail

# Resolve repo root from this script's location (shells/macos -> ../../).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

CMD="${1:-build}"

# --- 1. Build the Rust core staticlib (release) --------------------------------
echo ">>> Building Rust core (safetystrip-ffi, release)…"
if [ -f "${HOME}/.cargo/env" ]; then
    # shellcheck disable=SC1091
    source "${HOME}/.cargo/env"
fi
( cd "${REPO_ROOT}" && cargo build -p safetystrip-ffi --release )

STATICLIB="${REPO_ROOT}/target/release/libsafetystrip_ffi.a"
if [ ! -f "${STATICLIB}" ]; then
    echo "ERROR: expected staticlib not found at ${STATICLIB}" >&2
    exit 1
fi
echo ">>> Staticlib ready: ${STATICLIB}"

# --- 2. swift-testing runtime path discovery (CLT-only environments) -----------
# Frameworks dir (Testing.framework) and the interop dylib dir, if present.
CLT_FW_DIR="/Library/Developer/CommandLineTools/Library/Developer/Frameworks"
CLT_INTEROP_DIR="/Library/Developer/CommandLineTools/Library/Developer/usr/lib"

TEST_FLAGS=()
if [ -d "${CLT_FW_DIR}" ]; then
    TEST_FLAGS+=( -Xswiftc -F -Xswiftc "${CLT_FW_DIR}" )
    TEST_FLAGS+=( -Xlinker -rpath -Xlinker "${CLT_FW_DIR}" )
fi
if [ -d "${CLT_INTEROP_DIR}" ]; then
    TEST_FLAGS+=( -Xlinker -rpath -Xlinker "${CLT_INTEROP_DIR}" )
fi

# --- 3. Dispatch --------------------------------------------------------------
cd "${SCRIPT_DIR}"
case "${CMD}" in
    build)
        echo ">>> swift build"
        swift build
        ;;
    release)
        echo ">>> swift build -c release"
        swift build -c release
        ;;
    test)
        echo ">>> swift build"
        swift build
        echo ">>> swift test"
        swift test "${TEST_FLAGS[@]}"
        ;;
    *)
        echo "Unknown command '${CMD}'. Use: build | release | test" >&2
        exit 2
        ;;
esac
echo ">>> Done."
