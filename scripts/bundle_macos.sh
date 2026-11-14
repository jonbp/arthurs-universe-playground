#!/bin/bash
# Build "Arthur's Universe Playground.app" - a self-contained macOS bundle with
# MoltenVK (Vulkan -> Metal) embedded, so no Vulkan SDK is needed to run it.
#
# Requirements to *build*: Rust, and MoltenVK (`brew install molten-vk`).
set -euo pipefail
cd "$(dirname "$0")/.."

APP_NAME="Arthur's Universe Playground"
BIN=arthurs-universe-playground
OUT="target/release/bundle"
APP="$OUT/$APP_NAME.app"

MOLTENVK="${MOLTENVK:-}"
for c in "$MOLTENVK" /opt/homebrew/lib/libMoltenVK.dylib /usr/local/lib/libMoltenVK.dylib "${VULKAN_SDK:-/nonexistent}/lib/libMoltenVK.dylib"; do
    if [ -n "$c" ] && [ -f "$c" ]; then MOLTENVK="$c"; break; fi
done
if [ -z "$MOLTENVK" ] || [ ! -f "$MOLTENVK" ]; then
    echo "error: libMoltenVK.dylib not found. Install it with: brew install molten-vk" >&2
    exit 1
fi

echo "==> Building release binary"
cargo build --release

echo "==> Assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Frameworks" "$APP/Contents/Resources"
cp "target/release/$BIN" "$APP/Contents/MacOS/$BIN"
cp -L "$MOLTENVK" "$APP/Contents/Frameworks/libMoltenVK.dylib"
install_name_tool -id "@rpath/libMoltenVK.dylib" "$APP/Contents/Frameworks/libMoltenVK.dylib" 2>/dev/null || true

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundleIdentifier</key><string>com.arthur.universe-playground</string>
    <key>CFBundleExecutable</key><string>$BIN</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>LSApplicationCategoryType</key><string>public.app-category.simulation-games</string>
</dict>
</plist>
PLIST

# App icon: render Earth with the game itself, crop it and build an .icns.
if [ ! -f assets/AppIcon.icns ]; then
    echo "==> Rendering app icon"
    TMP=$(mktemp -d)
    "target/release/$BIN" --preset solar --focus Earth --zoom 5.2 --hide-ui --frames 60 --speed 0.2 --screenshot "$TMP/shot.bmp" >/dev/null 2>&1 || true
    if [ -f "$TMP/shot.bmp" ]; then
        sips -s format png "$TMP/shot.bmp" --out "$TMP/shot.png" >/dev/null
        H=$(sips -g pixelHeight "$TMP/shot.png" | awk '/pixelHeight/{print $2}')
        sips -c "$H" "$H" "$TMP/shot.png" --out "$TMP/square.png" >/dev/null
        mkdir -p "$TMP/AppIcon.iconset" assets
        for s in 16 32 128 256 512; do
            sips -z $s $s "$TMP/square.png" --out "$TMP/AppIcon.iconset/icon_${s}x${s}.png" >/dev/null
            d=$((s * 2))
            sips -z $d $d "$TMP/square.png" --out "$TMP/AppIcon.iconset/icon_${s}x${s}@2x.png" >/dev/null
        done
        iconutil -c icns "$TMP/AppIcon.iconset" -o assets/AppIcon.icns
    fi
    rm -rf "$TMP"
fi
[ -f assets/AppIcon.icns ] && cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

echo "==> Code signing (ad-hoc)"
codesign --force --sign - "$APP/Contents/Frameworks/libMoltenVK.dylib"
codesign --force --sign - "$APP"

echo "==> Done: $APP"
du -sh "$APP"
