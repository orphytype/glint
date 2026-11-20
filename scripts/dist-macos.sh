#!/usr/bin/env bash
# Builds the macOS download into dist/:
#   Glint-<version>-macos.dmg    Glint.app for Apple silicon and Intel; drag it to Applications
#
#   scripts/dist-macos.sh              build both architectures and package
#   NO_BUILD=1 scripts/dist-macos.sh   package the binaries already in target/<arch>/release
#
# Runs on macOS (it needs lipo, iconutil, codesign and hdiutil, all part of the system or the
# Xcode command line tools) with the aarch64-apple-darwin and x86_64-apple-darwin targets added.
# The app is signed ad hoc, not with a Developer ID, so the first launch goes through
# right-click → Open (or System Settings → Privacy & Security → Open Anyway).
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)

version=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -1)
dist="$root/dist"
mkdir -p "$dist"
targets=(aarch64-apple-darwin x86_64-apple-darwin)

if [ -z "${NO_BUILD:-}" ]; then
    # Both architectures in one cargo run: the build scripts and proc macros, which run on this
    # Mac, are compiled once instead of twice, and the two targets share the cores.
    # Without RUNE's headless tools, which only the developer scenes use.
    MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --release --locked -p glint --no-default-features $(for t in "${targets[@]}"; do echo "--target $t"; done)
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
app="$work/Glint.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

# One binary for both kinds of Mac.
lipo -create -output "$app/Contents/MacOS/glint" $(for t in "${targets[@]}"; do echo "target/$t/release/glint"; done)
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"
iconutil -c icns packaging/macos/glint.iconset -o "$app/Contents/Resources/glint.icns"
cp LICENSE NOTICE.md "$app/Contents/Resources/"

# Ad hoc: enough for Apple silicon to run it, and it seals the bundle.
codesign --force --deep --sign - "$app"
codesign --verify --deep --strict "$app"

# The disk image: the app beside a link to Applications.
stage="$work/dmg"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
out="$dist/Glint-$version-macos.dmg"
rm -f "$out"
hdiutil create -volname "Glint $version" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$out"

ls -la "$dist"
