#!/bin/sh
# Builds a drag-to-Applications DMG, or a PKG installer with --pkg.
# Set signing identities and KOI_NOTARY_PROFILE for Gatekeeper-ready releases.
set -eu
cd "$(dirname "$0")/.."

case "${1:---dmg}" in
    --pkg) format=pkg ;;
    --dmg) format=dmg ;;
    *) echo "Usage: scripts/package-macos.sh [--pkg|--dmg]" >&2; exit 1 ;;
esac

if [ -n "${KOI_NOTARY_PROFILE:-}" ] && [ -z "${KOI_SIGN_IDENTITY:-}" ]; then
    echo "KOI_NOTARY_PROFILE requires KOI_SIGN_IDENTITY" >&2
    exit 1
fi
if [ "$format" = pkg ] && [ -n "${KOI_SIGN_IDENTITY:-}" ] && [ -z "${KOI_INSTALLER_IDENTITY:-}" ]; then
    echo "Signed PKGs also require KOI_INSTALLER_IDENTITY" >&2
    exit 1
fi
if [ "$format" = pkg ] && [ -n "${KOI_INSTALLER_IDENTITY:-}" ] && [ -z "${KOI_SIGN_IDENTITY:-}" ]; then
    echo "Signed PKGs also require KOI_SIGN_IDENTITY" >&2
    exit 1
fi

scripts/bundle-macos.sh
app=target/release/Koi.app

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
arch=$(uname -m)
package="target/release/Koi-${version}-${arch}.${format}"
volume="Koi ${version}-${arch}"

if [ "$format" = dmg ]; then
    scratch=$(mktemp -d "${TMPDIR:-/tmp}/koi-dmg.XXXXXX")
    staging="$scratch/staging"
    mkdir "$staging"
    trap 'rm -rf "$scratch"' EXIT HUP INT TERM
    ditto "$app" "$staging/Koi.app"
    ln -s /Applications "$staging/Applications"
    python3 scripts/finder_layout.py "$staging/.DS_Store"
    hdiutil makehybrid -o "$scratch/image" -hfs -iso \
        -hfs-volume-name "$volume" -iso-volume-name KOI "$staging"
    hdiutil convert "$scratch/image.iso" -format UDZO -ov -o "$package"
    if [ -n "${KOI_SIGN_IDENTITY:-}" ]; then
        codesign --force --timestamp --sign "$KOI_SIGN_IDENTITY" "$package"
        codesign --verify "$package"
    fi
    hdiutil verify "$package"
elif [ -n "${KOI_INSTALLER_IDENTITY:-}" ]; then
    pkgbuild --component "$app" --install-location /Applications \
        --identifier dev.koi.Koi.pkg --version "$version" \
        --sign "$KOI_INSTALLER_IDENTITY" "$package"
    pkgutil --check-signature "$package"
else
    pkgbuild --component "$app" --install-location /Applications \
        --identifier dev.koi.Koi.pkg --version "$version" "$package"
fi

if [ -n "${KOI_NOTARY_PROFILE:-}" ]; then
    xcrun notarytool submit "$package" --keychain-profile "$KOI_NOTARY_PROFILE" --wait
    xcrun stapler staple "$package"
    xcrun stapler validate "$package"
fi

echo "$package"
