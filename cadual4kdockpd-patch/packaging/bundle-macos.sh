#!/bin/sh
# Builds "CADUAL4KDOCKPD Patch.app" for Apple silicon and Intel Macs into target/bundle/.
# Run on macOS 14 or later with Xcode: packaging/bundle-macos.sh
#
# The app is the SwiftUI window (macos/*.swift) plus the Rust engine it runs (`cadual4kdockpd-patch
# serve`), both in Contents/MacOS.
#
# With DOCK_IMAGE_FILE, DOCK_HUB_L1_FILE and DOCK_HUB_L2_FILE set (absolute paths to the power controller
# image and to i-tec's two hub images) the firmware is built into the app, so the app needs no files
# next to it. Such an app contains vendor firmware: do not publish it.
#
# The app is signed for local use only (ad hoc), not notarized. A Mac that received it by chat, mail
# or browser refuses to open it until the download mark is cleared:
#   xattr -dr com.apple.quarantine "CADUAL4KDOCKPD Patch.app"
set -eu
cd "$(dirname "$0")/.."

NAME="CADUAL4KDOCKPD Patch"
BIN=cadual4kdockpd-patch
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
APP="target/bundle/$NAME.app"
WORK=target/bundle/work

rm -rf "$APP" "$WORK"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$WORK/icon.iconset"

FEATURES=""
if [ -n "${DOCK_IMAGE_FILE:-}" ]; then
    : "${DOCK_HUB_L1_FILE:?set DOCK_HUB_L1_FILE as well}" "${DOCK_HUB_L2_FILE:?set DOCK_HUB_L2_FILE as well}"
    FEATURES="--features embedded-image"
fi
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    # shellcheck disable=SC2086  # FEATURES is two words or none
    cargo build --release --target "$target" $FEATURES
done
lipo -create -output "$APP/Contents/MacOS/$BIN" \
    "target/aarch64-apple-darwin/release/$BIN" "target/x86_64-apple-darwin/release/$BIN"

for arch in arm64 x86_64; do
    xcrun swiftc -O -swift-version 6 -default-isolation MainActor -enable-upcoming-feature NonisolatedNonsendingByDefault \
        -parse-as-library -target "$arch-apple-macos14.0" macos/*.swift -o "$WORK/window-$arch"
done
lipo -create -output "$APP/Contents/MacOS/$NAME" "$WORK/window-arm64" "$WORK/window-x86_64"

swift packaging/icon.swift "$WORK/icon.png"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$WORK/icon.png" --out "$WORK/icon.iconset/icon_${size}x${size}.png" >/dev/null
    sips -z "$((size * 2))" "$((size * 2))" "$WORK/icon.png" --out "$WORK/icon.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$WORK/icon.iconset" -o "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$NAME</string>
    <key>CFBundleDisplayName</key><string>$NAME</string>
    <key>CFBundleIdentifier</key><string>local.cadual4kdockpd-patch</string>
    <key>CFBundleExecutable</key><string>$NAME</string>
    <key>CFBundleIconFile</key><string>icon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
EOF
plutil -lint "$APP/Contents/Info.plist" >/dev/null

codesign --force --sign - "$APP/Contents/MacOS/$BIN"
codesign --force --sign - "$APP"
codesign --verify --strict "$APP"
rm -rf "$WORK"
echo "built $APP ($(lipo -archs "$APP/Contents/MacOS/$NAME")${FEATURES:+, firmware built in})"
