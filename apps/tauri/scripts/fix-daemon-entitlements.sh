#!/bin/bash
set -e

# This script fixes the daemon entitlements in the bundled macOS app
# It removes the app-sandbox entitlement which causes the daemon to crash

# Only run on macOS
if [ "$(uname)" != "Darwin" ]; then
    echo "Skipping daemon entitlement fix (macOS only)"
    exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BUNDLE_PATH="$1"

if [ -z "$BUNDLE_PATH" ]; then
    echo "Usage: $0 <path-to-app-bundle>"
    exit 1
fi

MACOS_DIR="$BUNDLE_PATH/Contents/MacOS"
ENTITLEMENTS_PATH="$SCRIPT_DIR/../src-tauri/DaemonEntitlements.plist"

if [ -f "$MACOS_DIR/sd-daemon" ]; then
    DAEMON_PATH="$MACOS_DIR/sd-daemon"
else
    DAEMON_PATHS=("$MACOS_DIR"/sd-daemon-*)
    if [ ${#DAEMON_PATHS[@]} -eq 1 ] && [ -f "${DAEMON_PATHS[0]}" ]; then
        DAEMON_PATH="${DAEMON_PATHS[0]}"
    else
        echo "Error: Daemon not found in $MACOS_DIR"
        exit 1
    fi
fi

if [ ! -f "$DAEMON_PATH" ]; then
    echo "Error: Daemon not found at $DAEMON_PATH"
    exit 1
fi

if [ ! -f "$ENTITLEMENTS_PATH" ]; then
    echo "Error: DaemonEntitlements.plist not found at $ENTITLEMENTS_PATH"
    exit 1
fi

echo "Re-signing daemon with correct entitlements..."
codesign --force --sign - \
    --entitlements "$ENTITLEMENTS_PATH" \
    --options runtime \
    "$DAEMON_PATH"

echo "Re-signing app bundle after daemon entitlement fix..."
codesign --force --sign - \
    --entitlements "$SCRIPT_DIR/../src-tauri/Entitlements.plist" \
    --options runtime \
    "$BUNDLE_PATH"

echo "✓ Daemon and app bundle re-signed successfully"
