#!/bin/bash
# Package the Linux release: binary + .desktop entry + app icon + installer.
# Usage: scripts/package_linux.sh <output-dir>   (after `cargo build --release`)
set -euo pipefail
cd "$(dirname "$0")/.."

BIN=arthurs-universe-playground
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
ARCH=$(uname -m)
NAME="ArthursUniversePlayground-$VERSION-linux-$ARCH"
OUT="${1:-target/release/dist}"
STAGE="$OUT/$NAME"

# build.rs renders the icon into the newest OUT_DIR for this crate.
ICON_DIR=$(ls -td target/release/build/$BIN-*/out 2>/dev/null | while read -r d; do [ -f "$d/AppIcon-256.png" ] && echo "$d" && break; done)
[ -n "$ICON_DIR" ] || { echo "error: icon not found; run cargo build --release first" >&2; exit 1; }

rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "target/release/$BIN" "$STAGE/"
cp "assets/linux/$BIN.desktop" "$STAGE/"
cp "$ICON_DIR/AppIcon-256.png" "$STAGE/$BIN.png"
cp README.md CHANGELOG.md "$STAGE/"
cat > "$STAGE/install.sh" <<'INSTALL'
#!/bin/bash
# Installs for the current user into ~/.local (binary, menu entry, icon).
set -euo pipefail
cd "$(dirname "$0")"
BIN=arthurs-universe-playground
install -Dm755 "$BIN" "$HOME/.local/bin/$BIN"
install -Dm644 "$BIN.png" "$HOME/.local/share/icons/hicolor/256x256/apps/$BIN.png"
install -Dm644 "$BIN.desktop" "$HOME/.local/share/applications/$BIN.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$HOME/.local/share/applications" || true
echo "Installed. Launch 'Arthur's Universe Playground' from your app menu (or run $BIN)."
INSTALL
chmod +x "$STAGE/install.sh" "$STAGE/$BIN"
tar -C "$OUT" -czf "$OUT/$NAME.tar.gz" "$NAME"
echo "$OUT/$NAME.tar.gz"
