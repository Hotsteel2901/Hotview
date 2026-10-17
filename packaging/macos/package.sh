#!/bin/sh
# Build Hotview.app and a drag-to-Applications DMG on macOS.
#
#   packaging/macos/package.sh [arm64|x86_64]
#
# FFmpeg (and any other Homebrew libraries the binary links against) is copied
# into Contents/Frameworks, so the app runs on machines without Homebrew.
# The bundle is ad-hoc signed; set HOTVIEW_CODESIGN_IDENTITY to use a Developer
# ID certificate instead. Notarization is intentionally left to the release
# owner (see README).
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
version=${HOTVIEW_VERSION:-$(sh "$root/packaging/version.sh")}
binary=${HOTVIEW_BINARY:-"$root/target/release/hotview"}
outdir=${HOTVIEW_OUTDIR:-"$root/dist"}
arch=${1:-$(uname -m)}

case "$arch" in
    arm64 | aarch64) arch=arm64 ;;
    x86_64 | amd64) arch=x86_64 ;;
    *)
        echo "error: unsupported macOS architecture: $arch" >&2
        exit 2
        ;;
esac

if [ ! -f "$binary" ]; then
    echo "error: binary not found: $binary" >&2
    exit 1
fi
command -v dylibbundler >/dev/null 2>&1 || {
    echo "error: dylibbundler is required (brew install dylibbundler)" >&2
    exit 1
}
command -v iconutil >/dev/null 2>&1 || {
    echo "error: iconutil is required (Xcode command line tools)" >&2
    exit 1
}

mkdir -p "$outdir"
outdir=$(CDPATH= cd -- "$outdir" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/hotview-macos.XXXXXX")
trap 'rm -rf "$tmp"' EXIT INT TERM

app="$tmp/Hotview.app"
contents="$app/Contents"
mkdir -p "$contents/MacOS" "$contents/Resources" "$contents/Frameworks"

install -m755 "$binary" "$contents/MacOS/hotview"

printf 'APPL????' >"$contents/PkgInfo"

install -m644 "$root/LICENSE" "$contents/Resources/LICENSE"
install -m644 "$root/README.md" "$contents/Resources/README.md"

# App icon: build an .icns from the 1024px master PNG.
iconset="$tmp/Hotview.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$root/packaging/icons/hotview-1024.png" \
        --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$root/packaging/icons/hotview-1024.png" \
        --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$contents/Resources/hotview.icns"

# Bundle FFmpeg and every non-system dylib next to the executable.
search_args=""
for prefix in "$(brew --prefix 2>/dev/null || true)" /opt/homebrew /usr/local; do
    if [ -n "$prefix" ] && [ -d "$prefix/lib" ]; then
        search_args="$search_args -s $prefix/lib"
    fi
done
# shellcheck disable=SC2086 # search_args is a deliberately split list
dylibbundler -od -b \
    -x "$contents/MacOS/hotview" \
    -d "$contents/Frameworks" \
    -p "@executable_path/../Frameworks/" \
    $search_args >/dev/null

# Advertise the real minimum macOS version: take the highest deployment
# target required by the executable and every bundled dylib. Homebrew bottles
# often need a newer macOS than the Rust binary itself.
min_os=11.0
if command -v otool >/dev/null 2>&1; then
    detected=$( {
        otool -l "$contents/MacOS/hotview"
        for lib in "$contents/Frameworks/"*.dylib; do
            if [ -f "$lib" ]; then otool -l "$lib"; fi
        done
    } 2>/dev/null | awk '/[[:space:]]minos[[:space:]]/ {print $2}' |
        awk -F. 'NF {v = $1 * 1000 + $2; if (v > max) {max = v; out = $0}}
                 END {if (out != "") print out}')
    if [ -n "$detected" ]; then min_os="$detected"; fi
fi
sed -e "s/__VERSION__/$version/g" -e "s/__MINOS__/$min_os/g" \
    "$root/packaging/macos/Info.plist" >"$contents/Info.plist"

xattr -cr "$app"
identity=${HOTVIEW_CODESIGN_IDENTITY:--}
for dylib in "$contents/Frameworks/"*.dylib; do
    [ -f "$dylib" ] || continue
    codesign --force --sign "$identity" "$dylib" >/dev/null 2>&1 ||
        codesign --force --sign - "$dylib" >/dev/null
done
codesign --force --sign "$identity" "$app" >/dev/null 2>&1 ||
    codesign --force --sign - --deep "$app" >/dev/null

# Keep the binary architecture honest even when a caller passes a wrong label.
binary_arch=$(lipo -archs "$contents/MacOS/hotview" 2>/dev/null || true)
case "$binary_arch" in
    *"$arch"*) ;;
    *) echo "warning: binary reports '$binary_arch', packaging as $arch" >&2 ;;
esac

dmgroot="$tmp/dmg"
mkdir -p "$dmgroot"
cp -R "$app" "$dmgroot/Hotview.app"
ln -s /Applications "$dmgroot/Applications"
install -m644 "$root/packaging/macos/INSTALL.txt" "$dmgroot/INSTALL.txt"
install -m644 "$root/LICENSE" "$dmgroot/LICENSE"

out="$outdir/Hotview-${version}-${arch}.dmg"
rm -f "$out"
hdiutil create -quiet -volname "Hotview ${version}" -srcfolder "$dmgroot" \
    -ov -format UDZO "$out"

printf '%s\n' "$out"
