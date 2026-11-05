#!/usr/bin/env bash
# Construit "Ressource Utility.app" (+ zip). Options : --universal, --install
set -euo pipefail

cd "$(dirname "$0")/.."
UNIVERSAL=0
INSTALL=0
for arg in "$@"; do
    case "$arg" in
        --universal) UNIVERSAL=1 ;;
        --install) INSTALL=1 ;;
        *) echo "Option inconnue : $arg" >&2; exit 2 ;;
    esac
done

BIN=ressource-utility
APP_NAME="Ressource Utility"
VERSION=$(awk -F'"' '/^version *=/ {print $2; exit}' Cargo.toml)
OUT=target/bundle
APP="$OUT/$APP_NAME.app"

if [ "$UNIVERSAL" = 1 ]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    mkdir -p target/universal
    lipo -create -output target/universal/$BIN \
        target/aarch64-apple-darwin/release/$BIN \
        target/x86_64-apple-darwin/release/$BIN
    BIN_PATH=target/universal/$BIN
else
    cargo build --release
    BIN_PATH=target/release/$BIN
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN_PATH" "$APP/Contents/MacOS/$BIN"
sed "s/@VERSION@/$VERSION/g" macos/Info.plist > "$APP/Contents/Info.plist"

codesign --force --deep -s - "$APP"

ZIP="$OUT/Ressource-Utility-$VERSION-macos.zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

echo "App : $APP"
echo "Zip : $ZIP"

if [ "$INSTALL" = 1 ]; then
    mkdir -p "$HOME/Applications"
    rm -rf "$HOME/Applications/$APP_NAME.app"
    ditto "$APP" "$HOME/Applications/$APP_NAME.app"
    echo "Installé : $HOME/Applications/$APP_NAME.app"
fi
