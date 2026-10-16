#!/bin/sh
# Builds Koi.app. Notifications only work when Koi runs from the bundle.
# KOI_SIGN_IDENTITY signs for distribution; otherwise the signature is ad-hoc.
#
# --install moves the bundle to /Applications/Koi.app, leaving no copy in
# the build directory that Launchpad would list beside it. A running Koi
# keeps running from the old copy until it is restarted.
#
# Usage: scripts/bundle-macos.sh [--debug] [--install]
set -eu
cd "$(dirname "$0")/.."

profile=release
install=false
for arg in "$@"; do
    case "$arg" in
        --debug) profile=debug ;;
        --install) install=true ;;
        *) echo "Usage: scripts/bundle-macos.sh [--debug] [--install]" >&2; exit 1 ;;
    esac
done
if [ "$profile" = debug ]; then
    cargo build -p koi-app
else
    cargo build --release -p koi-app
fi

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
app="target/$profile/Koi.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
mkdir -p "$app/Contents/Resources"
cp "target/$profile/koi" "$app/Contents/MacOS/koi"
strip -x "$app/Contents/MacOS/koi"
cp packaging/macos/Koi.icns "$app/Contents/Resources/Koi.icns"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
if [ -n "${KOI_SIGN_IDENTITY:-}" ]; then
    codesign --force --options runtime --timestamp --sign "$KOI_SIGN_IDENTITY" "$app"
else
    codesign --force --sign - "$app"
fi
codesign --verify --deep --strict "$app"

if [ "$install" = true ]; then
    lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
    # Removed before copying rather than overwritten, as rewriting the
    # executable of a running Koi in place would crash it.
    rm -rf /Applications/Koi.app
    ditto "$app" /Applications/Koi.app
    "$lsregister" -u "$app" 2>/dev/null || true
    rm -rf "$app"
    "$lsregister" -f /Applications/Koi.app
    app=/Applications/Koi.app
fi
echo "$app"
