#!/bin/sh
# Builds BatteryTray.app for macOS 13 and later, signed by the project's certificate (see below), and the zip a release
# carries:
#   macos/build.sh [native|universal]     (default: universal, Apple Silicon and Intel in one app)
# Out: target/macos/BatteryTray.app and target/macos/BatteryTray-macOS.zip. Needs Rust with the
# aarch64-apple-darwin and x86_64-apple-darwin targets, the Xcode command line tools, and cargo-about for the crates'
# licenses in the app (`cargo install cargo-about --locked --features cli`; without it the app is built without them).
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="$ROOT/target/macos"
APP="$OUT/BatteryTray.app"
WORK="$OUT/build"
SOURCES="$ROOT/macos/BatteryTray"
VERSION=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$ROOT/Cargo.toml" | head -n 1)

case "${1:-universal}" in
  native) ARCHS=$(uname -m) ;;
  universal) ARCHS="arm64 x86_64" ;;
  *) echo "usage: macos/build.sh [native|universal]" >&2; exit 2 ;;
esac

rm -rf "$APP" "$WORK"
mkdir -p "$WORK" "$APP/Contents/MacOS" "$APP/Contents/Resources"

BINARIES=""
WIDGETS=""
for ARCH in $ARCHS; do
  case $ARCH in
    arm64) TARGET=aarch64-apple-darwin ;;
    x86_64) TARGET=x86_64-apple-darwin ;;
  esac
  echo "== core ($TARGET)"
  MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release --locked -p batterytray-macos --target "$TARGET" \
    --manifest-path "$ROOT/Cargo.toml"
  echo "== app ($ARCH)"
  # shellcheck disable=SC2046
  swiftc -O -swift-version 5 -target "$ARCH-apple-macos13.0" \
    -import-objc-header "$SOURCES/core.h" $(ls "$SOURCES"/*.swift) \
    -L "$ROOT/target/$TARGET/release" -lbatterytray_macos \
    -framework IOKit -framework CoreFoundation -framework Foundation -framework AppKit -framework SwiftUI \
    -framework Carbon -framework GameController -framework UserNotifications -framework CoreAudio \
    -framework UniformTypeIdentifiers -framework WidgetKit \
    -o "$WORK/BatteryTray-$ARCH"
  BINARIES="$BINARIES $WORK/BatteryTray-$ARCH"
  # The desktop widgets (macOS 14 and later): an app extension, which starts in Foundation's NSExtensionMain
  echo "== widgets ($ARCH)"
  # shellcheck disable=SC2046
  swiftc -O -swift-version 5 -parse-as-library -application-extension -target "$ARCH-apple-macos14.0" \
    $(ls "$ROOT"/macos/Widget/*.swift) -framework WidgetKit -framework SwiftUI -framework AppKit \
    -Xlinker -application_extension -Xlinker -e -Xlinker _NSExtensionMain \
    -o "$WORK/BatteryTrayWidgets-$ARCH"
  WIDGETS="$WIDGETS $WORK/BatteryTrayWidgets-$ARCH"
done
# shellcheck disable=SC2086
lipo -create $BINARIES -output "$APP/Contents/MacOS/BatteryTray"
# The widgets in the app, where macOS finds them
APPEX="$APP/Contents/PlugIns/BatteryTrayWidgets.appex"
mkdir -p "$APPEX/Contents/MacOS" "$APPEX/Contents/Resources"
# shellcheck disable=SC2086
lipo -create $WIDGETS -output "$APPEX/Contents/MacOS/BatteryTrayWidgets"

echo "== bundle $VERSION"
sed "s/@VERSION@/$VERSION/g" "$ROOT/macos/Info.plist" > "$APP/Contents/Info.plist"
swift "$ROOT/macos/make-icon.swift" "$WORK/AppIcon.iconset"
iconutil -c icns -o "$APP/Contents/Resources/AppIcon.icns" "$WORK/AppIcon.iconset"
# The reason macOS shows when it asks for Bluetooth, in each language of the app
for lproj in "$ROOT"/macos/*.lproj; do
  mkdir -p "$APP/Contents/Resources/$(basename "$lproj")"
  cp "$lproj/InfoPlist.strings" "$APP/Contents/Resources/$(basename "$lproj")/"
done
# The widgets' own texts: their names in the widget gallery, and what they say without devices
sed "s/@VERSION@/$VERSION/g" "$ROOT/macos/Widget/Info.plist" > "$APPEX/Contents/Info.plist"
for lproj in "$ROOT"/macos/Widget/*.lproj; do
  cp -R "$lproj" "$APPEX/Contents/Resources/"
done
# The licenses that come with the code: the app's own, its sources', the crates' in it
cp "$ROOT/LICENSE" "$ROOT/THIRD_PARTY_NOTICES.md" "$APP/Contents/Resources/"
if cargo about --version >/dev/null 2>&1; then
  cargo about generate --locked --manifest-path "$ROOT/macos/core/Cargo.toml" -c "$ROOT/macos/about.toml" \
    "$ROOT/macos/about.hbs" -o "$APP/Contents/Resources/THIRD_PARTY_LICENSES.md"
else
  echo "warning: cargo-about isn't installed: the app has no THIRD_PARTY_LICENSES.md, a release must not go out so" >&2
fi
# The signature macOS ties the app's permissions to (Bluetooth, Input Monitoring). The project's own
# certificate, "BatteryTray Code Signing" (in the keychain, or the identity in BATTERYTRAY_SIGNING_IDENTITY), is the
# same for every version, so the permissions stay from one version to the next. It isn't Apple's Developer ID: macOS
# still wants "Open Anyway" once for an app downloaded from the internet. Without it the app is signed ad hoc, and
# macOS sees each build as another app and asks for the permissions again; the app updates only to a version signed
# as it is. So a build a release takes (BATTERYTRAY_REQUIRE_SIGNING=1) fails without the certificate, and with
# BATTERYTRAY_SIGNING_ROOT (its SHA-1) one signed by another certificate fails too.
SIGN="${BATTERYTRAY_SIGNING_IDENTITY:-}"
if [ -z "$SIGN" ]; then
  SIGN=$(security find-identity -p codesigning 2>/dev/null | awk '/"BatteryTray Code Signing"/ { print $2; exit }')
fi
if [ -z "$SIGN" ]; then
  if [ -n "${BATTERYTRAY_REQUIRE_SIGNING:-}" ]; then
    echo "error: no \"BatteryTray Code Signing\" identity, and this build is for a release: it must not be signed ad hoc" >&2
    exit 1
  fi
  echo "warning: no \"BatteryTray Code Signing\" identity: signed ad hoc, a release must not go out so" >&2
  SIGN=-
fi
# Inside out: the widgets first, with the sandbox an extension must have and the one folder it reads
codesign --force --sign "$SIGN" --entitlements "$ROOT/macos/Widget/Widget.entitlements" "$APPEX"
codesign --force --sign "$SIGN" "$APP"
codesign --verify --strict "$APP"
REQUIREMENT=$(codesign -d -r- "$APP" 2>&1 | sed -n 's/^designated => //p')
echo "== signed: ${REQUIREMENT:-ad hoc}"
if [ "$SIGN" != "-" ] && [ -n "${BATTERYTRAY_SIGNING_ROOT:-}" ] \
  && ! printf '%s' "$REQUIREMENT" | grep -qi "certificate root = H\"$BATTERYTRAY_SIGNING_ROOT\""; then
  echo "error: the app isn't signed by the certificate $BATTERYTRAY_SIGNING_ROOT" >&2
  exit 1
fi

rm -f "$OUT/BatteryTray-macOS.zip"
ditto -c -k --keepParent "$APP" "$OUT/BatteryTray-macOS.zip"
echo "== $APP"
echo "== $OUT/BatteryTray-macOS.zip"

# The disk image people download (a release's universal build): the app beside a link to Applications, to be dragged
# there. The zip stays what the app updates itself from.
if [ "${1:-universal}" = "universal" ]; then
  STAGE="$WORK/dmg"
  rm -rf "$STAGE" "$OUT/BatteryTray-macOS.dmg"
  mkdir -p "$STAGE"
  ditto "$APP" "$STAGE/BatteryTray.app"
  ln -s /Applications "$STAGE/Applications"
  hdiutil create -quiet -volname BatteryTray -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$OUT/BatteryTray-macOS.dmg"
  echo "== $OUT/BatteryTray-macOS.dmg"
fi
