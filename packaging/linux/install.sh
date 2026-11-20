#!/bin/sh
# Installs Glint from this folder for the current user, into ~/.local (no root needed):
#   ./install.sh             install or update
#   ./install.sh --remove    uninstall (your settings and sign-in in ~/.config/glint stay)
# Run it as root (sudo ./install.sh) to install for everyone into /usr/local instead.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
if [ "$(id -u)" -eq 0 ]; then prefix=/usr/local; else prefix="$HOME/.local"; fi
bin="$prefix/bin"
share="$prefix/share"

if [ "${1:-}" = "--remove" ]; then
    rm -f "$bin/glint" "$share/applications/glint.desktop" "$share/metainfo/glint.metainfo.xml"
    for s in 256 128 64 48 32; do rm -f "$share/icons/hicolor/${s}x${s}/apps/glint.png"; done
    echo "Glint removed from $prefix"
    exit 0
fi

mkdir -p "$bin" "$share/applications" "$share/metainfo"
cp "$here/glint" "$bin/glint" && chmod 755 "$bin/glint"
# The menu entry runs the installed binary by its full path, in case ~/.local/bin is not on PATH.
sed "s|^Exec=glint|Exec=$bin/glint|" "$here/glint.desktop" > "$share/applications/glint.desktop"
cp "$here/glint.metainfo.xml" "$share/metainfo/"
for s in 256 128 64 48 32; do
    mkdir -p "$share/icons/hicolor/${s}x${s}/apps"
    cp "$here/icons/${s}x${s}/apps/glint.png" "$share/icons/hicolor/${s}x${s}/apps/glint.png"
done
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database -q "$share/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -qtf "$share/icons/hicolor" 2>/dev/null || true
echo "Glint installed to $bin/glint; it is in your applications menu."
