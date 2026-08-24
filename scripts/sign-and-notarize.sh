#!/usr/bin/env bash
# sign-and-notarize.sh — Code-sign and notarize the Dev Cockpit macOS app.
#
# Required environment variables:
#   APPLE_DEVELOPER_ID_APPLICATION  — "Developer ID Application: Name (TEAM_ID)"
#   APPLE_ID                        — Apple ID email for notarization
#   APPLE_APP_PASSWORD              — App-specific password (NOT account password)
#   APPLE_TEAM_ID                   — 10-char team ID
#
# Usage:
#   ./scripts/sign-and-notarize.sh <path-to-app-bundle>
#
# Exit codes:
#   0 — success (signed, notarized, stapled, verified)
#   1 — missing arguments or environment
#   2 — codesign failure
#   3 — notarization failure
#   4 — stapling failure
#   5 — spctl verification failure

set -euo pipefail

APP_PATH="${1:-}"

# ---------------------------------------------------------------------------
# Validation
# ---------------------------------------------------------------------------

if [[ -z "$APP_PATH" ]]; then
  echo "error: usage: $0 <path-to-App.app>"
  exit 1
fi

if [[ ! -d "$APP_PATH" ]]; then
  echo "error: app bundle not found: $APP_PATH"
  exit 1
fi

for var in APPLE_DEVELOPER_ID_APPLICATION APPLE_ID APPLE_APP_PASSWORD APPLE_TEAM_ID; do
  if [[ -z "${!var:-}" ]]; then
    echo "error: required environment variable $var is not set"
    exit 1
  fi
done

echo "=== Dev Cockpit: Sign & Notarize ==="
echo "App: $APP_PATH"
echo "Identity: $APPLE_DEVELOPER_ID_APPLICATION"
echo "Team: $APPLE_TEAM_ID"
echo ""

# ---------------------------------------------------------------------------
# Step 1: Code-sign the app bundle
# ---------------------------------------------------------------------------

echo "→ Signing app bundle..."

# Sign all nested frameworks/dylibs first (deep sign)
codesign --deep --force --options runtime \
  --sign "$APPLE_DEVELOPER_ID_APPLICATION" \
  --timestamp \
  --entitlements "$(dirname "$0")/../src-tauri/Entitlements.plist" \
  "$APP_PATH"

if [[ $? -ne 0 ]]; then
  echo "error: codesign failed"
  exit 2
fi

# Verify the signature
codesign --verify --deep --strict "$APP_PATH"
echo "✓ Code signature verified"

# ---------------------------------------------------------------------------
# Step 2: Create ZIP for notarization
# ---------------------------------------------------------------------------

ZIP_PATH="${APP_PATH%.app}.zip"
echo "→ Creating ZIP for notarization: $ZIP_PATH"
ditto -c -k --keepParent "$APP_PATH" "$ZIP_PATH"

# ---------------------------------------------------------------------------
# Step 3: Submit for notarization
# ---------------------------------------------------------------------------

echo "→ Submitting for Apple notarization..."

xcrun notarytool submit "$ZIP_PATH" \
  --apple-id "$APPLE_ID" \
  --password "$APPLE_APP_PASSWORD" \
  --team-id "$APPLE_TEAM_ID" \
  --wait \
  --timeout 30m

if [[ $? -ne 0 ]]; then
  echo "error: notarization failed"
  echo "Run: xcrun notarytool log <submission-id> --apple-id $APPLE_ID --password \$APPLE_APP_PASSWORD --team-id $APPLE_TEAM_ID"
  exit 3
fi

echo "✓ Notarization accepted"

# ---------------------------------------------------------------------------
# Step 4: Staple the notarization ticket
# ---------------------------------------------------------------------------

echo "→ Stapling notarization ticket..."

xcrun stapler staple "$APP_PATH"

if [[ $? -ne 0 ]]; then
  echo "error: stapling failed"
  exit 4
fi

echo "✓ Ticket stapled"

# ---------------------------------------------------------------------------
# Step 5: Final verification with spctl
# ---------------------------------------------------------------------------

echo "→ Verifying with Gatekeeper (spctl)..."

spctl --assess --type exec --verbose=2 "$APP_PATH"

if [[ $? -ne 0 ]]; then
  echo "error: spctl verification failed — app will trigger Gatekeeper warning"
  exit 5
fi

echo "✓ Gatekeeper verification passed"

# ---------------------------------------------------------------------------
# Cleanup & Summary
# ---------------------------------------------------------------------------

rm -f "$ZIP_PATH"

echo ""
echo "=== SUCCESS ==="
echo "App is signed, notarized, and ready for distribution."
echo "Path: $APP_PATH"
echo ""
