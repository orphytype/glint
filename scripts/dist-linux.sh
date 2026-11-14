#!/usr/bin/env bash
# Builds the Linux downloads into dist/:
#   Glint-<version>-x86_64.AppImage        runs as is on most distributions
#   glint_<version>-1_amd64.deb            Debian, Ubuntu, Mint, Pop!_OS...
#   glint-<version>-1.x86_64.rpm           Fedora, openSUSE, RHEL...
#   glint-<version>-linux-x86_64.tar.gz    anything else: unpack and run ./install.sh
#
#   scripts/dist-linux.sh                  all four
#   scripts/dist-linux.sh tar deb          only those (any of: appimage deb rpm tar)
#   NO_BUILD=1 scripts/dist-linux.sh       package the binary already in target/release
#
# Needs: a C compiler, pkg-config, and the ALSA, fontconfig and Wayland headers to build
# (Debian: build-essential pkg-config libasound2-dev libfontconfig1-dev libwayland-dev);
# cargo-deb and cargo-generate-rpm for those two; curl for appimagetool, fetched on first use.
# Build on the oldest system you want to support: the binary needs that glibc or newer.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)

version=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' Cargo.toml | head -1)
dist="$root/dist"
bin="$root/target/release/glint"
mkdir -p "$dist"

kinds=("$@")
[ ${#kinds[@]} -eq 0 ] && kinds=(appimage deb rpm tar)
want() { local k; for k in "${kinds[@]}"; do [ "$k" = "$1" ] && return 0; done; return 1; }

if [ -z "${NO_BUILD:-}" ]; then
    # Without RUNE's headless tools, which only the developer scenes use.
    cargo build --release --locked -p glint --no-default-features
fi
[ -x "$bin" ] || { echo "no $bin; build it first" >&2; exit 1; }

icons() { # $1: the hicolor root to fill
    for s in 256 128 64 48 32; do
        install -Dm644 "packaging/linux/icons/$s.png" "$1/${s}x${s}/apps/glint.png"
    done
}

if want tar; then
    stage="$dist/glint-$version"
    rm -rf "$stage"
    install -Dm755 "$bin" "$stage/glint"
    install -Dm644 packaging/linux/glint.desktop "$stage/glint.desktop"
    install -Dm644 packaging/linux/glint.metainfo.xml "$stage/glint.metainfo.xml"
    icons "$stage/icons"
    install -Dm755 packaging/linux/install.sh "$stage/install.sh"
    install -Dm644 LICENSE "$stage/LICENSE"
    install -Dm644 README.md "$stage/README.md"
    install -Dm644 NOTICE.md "$stage/NOTICE.md"
    tar -C "$dist" -czf "$dist/glint-$version-linux-x86_64.tar.gz" "glint-$version"
    rm -rf "$stage"
    echo "tar: $dist/glint-$version-linux-x86_64.tar.gz"
fi

if want deb; then
    cargo deb -p glint --no-build --no-strip --output "$dist/"
fi

if want rpm; then
    # RPM versions cannot hold a hyphen: 0.1.0-alpha becomes 0.1.0~alpha, which sorts before 0.1.0.
    rpm_version=$(printf '%s' "$version" | sed 's/-/~/g')
    cargo generate-rpm -p crates/glint -o "$dist/" -s "version = \"$rpm_version\""
fi

if want appimage; then
    tool="$root/target/appimagetool-x86_64.AppImage"
    if [ ! -x "$tool" ]; then
        curl -fsSL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
        chmod +x "$tool"
    fi
    app="$root/target/Glint.AppDir"
    rm -rf "$app"
    install -Dm755 "$bin" "$app/usr/bin/glint"
    install -Dm644 packaging/linux/glint.desktop "$app/usr/share/applications/glint.desktop"
    install -Dm644 packaging/linux/glint.metainfo.xml "$app/usr/share/metainfo/glint.appdata.xml"
    icons "$app/usr/share/icons/hicolor"
    cp packaging/linux/glint.desktop "$app/glint.desktop"
    cp packaging/linux/icons/256.png "$app/glint.png"
    ln -s glint.png "$app/.DirIcon"
    ln -s usr/bin/glint "$app/AppRun"
    # Without FUSE (a container, CI) the tool unpacks itself instead of mounting.
    ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$app" "$dist/Glint-$version-x86_64.AppImage"
    rm -rf "$app"
fi

ls -la "$dist"
