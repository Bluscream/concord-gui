#!/usr/bin/env bash
#
# Package concord-gui as a portable AppImage.
#
#   ./scripts/appimage.sh              build release (with demo data) and package
#   ./scripts/appimage.sh --no-test    package a build WITHOUT demo data
#   ./scripts/appimage.sh --no-build   package whatever is already in target/release
#   ./scripts/appimage.sh --out DIR    write the .AppImage to DIR (default: dist/)
#
# Why the build runs in build-box and not on the host
# ---------------------------------------------------
# An AppImage can only run on a machine whose glibc is at least as new as the
# glibc it was linked against, because glibc is backward but not forward
# compatible. build-box is Debian 13 (glibc 2.41) and the host is Bazzite
# (glibc 2.43), so building in the container is what makes the result portable:
# it runs on the host and on anything else with glibc >= 2.41. Building on the
# host instead would stamp GLIBC_2.43 into the binary and lock out every distro
# that has not caught up yet.
#
# opusic-sys also needs cmake, which the immutable host does not have.
set -euo pipefail
IFS=$'\n\t'

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOX="build-box"
APP_ID="concord-gui"
APP_NAME="Concord"
OUT_DIR="$REPO/dist"
FEATURES="fixtures"
BUILD=1

info() { printf '\033[1;34m::\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --no-test)  FEATURES="$(printf '%s' "$FEATURES" | sed 's/fixtures,\{0,1\}//; s/,$//')" ;;
        --no-build) BUILD=0 ;;
        --out)      shift; OUT_DIR="${1:?--out needs a directory}" ;;
        -h|--help)  sed -n '2,9p' "${BASH_SOURCE[0]}" | sed 's/^# \?//'; exit 0 ;;
        *)          die "unknown option: $1 (try --help)" ;;
    esac
    shift
done

command -v appimagetool >/dev/null 2>&1 || die "appimagetool not found on PATH"

# The version comes from crates/gui, not the workspace root. The root package
# and the other five crates are upstream's (chojs23/concord) and carry its
# release number; crates/gui is this fork's own crate, and its version is what
# env!("CARGO_PKG_VERSION") stamps into the About box. Reading the root here
# shipped an AppImage labelled 2.5.9 around a binary that called itself 0.1.0.
MANIFEST="$REPO/crates/gui/Cargo.toml"
VERSION="$(sed -n '/^\[package\]/,/^\[/ s/^version = "\(.*\)"/\1/p' "$MANIFEST" | head -n1)"
[[ -n "$VERSION" ]] || die "could not read version from $MANIFEST"

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
if [[ $BUILD -eq 1 ]]; then
    BOX_LIST="$(distrobox list 2>/dev/null || true)"
    grep -q "| *$BOX *|" <<<"$BOX_LIST" ||
        die "distrobox '$BOX' not found. Create it with: setup-build-box.sh --create"

    # Leave most of the machine usable: a quarter of the cores, lowest priority.
    JOBS=$(( $(nproc) / 5 ))
    if [[ $JOBS -lt 1 ]]; then JOBS=1; fi

    info "Building release in '$BOX'${FEATURES:+ (features: $FEATURES)} on $JOBS cores"
    BUILD_ARGS=(build -p "$APP_ID" --release -j "$JOBS")
    [[ -n "$FEATURES" ]] && BUILD_ARGS+=(--features "$FEATURES")

    # distrobox wants one command string, so the array has to be flattened -
    # but NOT with ${BUILD_ARGS[*]}, which joins on the first character of IFS.
    # IFS is $'\n\t' here, so that produces a newline-separated list; `exec`
    # takes the first line and never returns, and what actually runs is a bare
    # `cargo build` - no package, no --release, no --features - while the line
    # above it prints "Building release (features: fixtures)". Quote each
    # argument and join on a real space.
    CMD="cd $(printf '%q' "$REPO") && exec nice -n 19 cargo"
    for arg in "${BUILD_ARGS[@]}"; do
        CMD="$CMD $(printf '%q' "$arg")"
    done
    distrobox enter "$BOX" -- bash -lc "$CMD" || die "build failed"
fi

BINARY="$REPO/target/release/$APP_ID"
[[ -x "$BINARY" ]] || die "no binary at $BINARY"

# Guard the whole point of building in the container. If this binary wants a
# glibc newer than the container's, something built it somewhere else and the
# AppImage would not be portable.
MAX_GLIBC="$(objdump -T "$BINARY" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -n1)"
info "Binary requires at most ${MAX_GLIBC:-none}"

# ---------------------------------------------------------------------------
# AppDir
# ---------------------------------------------------------------------------
APPDIR="$(mktemp -d)"
trap 'rm -rf "$APPDIR"' EXIT

mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/scalable/apps"

install -m755 "$BINARY" "$APPDIR/usr/bin/$APP_ID"

# Libraries deliberately NOT bundled. Each of these either loads host-side
# plugins or drivers, or speaks a protocol to a host daemon, and breaks when a
# bundled copy is forced in front of the system one:
#   vulkan, va, va-drm   dlopen the host's GPU drivers
#   asound               loads ALSA plugins from host paths
#   pipewire             wire protocol to the host pipewire daemon
#   dbus-1               talks to the host session bus
#   systemd              talks to the host journald / logind
#   xcb-*, Xau, Xdmcp    host X stack; must match the host libxcb we exclude
#   wayland-*, xcb, X11  talk to the host compositor / display server
#   libstdc++, libgcc_s  the host's are newer than Debian 13's, so its ABI wins
# glibc itself is excluded for the obvious reason: it cannot be swapped under a
# running binary by LD_LIBRARY_PATH.
EXCLUDE='^(ld-linux.*|libc|libm|libdl|libpthread|librt|libgcc_s|libstdc\+\+|libvulkan|libva|libva-drm|libva-x11|libdbus-1|libasound|libpipewire-.*|libsystemd|libwayland-.*|libxcb|libxcb-.*|libX11|libXau|libXdmcp|libGL|libEGL|libdrm)\.so'

info "Bundling libraries"
# IFS is $'\n\t' for this script, which contains no space - so an unscoped
# `read soname path` swallows the whole line into $soname and leaves $path
# empty, silently bundling nothing. Scope a space to this read.
while IFS=' ' read -r soname path; do
    [[ -f "$path" ]] || continue
    [[ "$soname" =~ $EXCLUDE ]] && continue
    install -m644 "$path" "$APPDIR/usr/lib/$soname"
    printf '   bundled %s\n' "$soname"
done < <(ldd "$BINARY" | sed -n 's/^\s*\([^ ]*\) => \([^ ]*\).*/\1 \2/p')

cat > "$APPDIR/$APP_ID.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <rect width="64" height="64" rx="14" fill="#181b1f"/>
  <path d="M20 24h24a4 4 0 0 1 4 4v12a4 4 0 0 1-4 4H28l-8 6V24z"
        fill="none" stroke="#5b8def" stroke-width="3" stroke-linejoin="round"/>
  <circle cx="27" cy="34" r="2.5" fill="#5b8def"/>
  <circle cx="35" cy="34" r="2.5" fill="#5b8def"/>
  <circle cx="43" cy="34" r="2.5" fill="#5b8def"/>
</svg>
SVG
cp "$APPDIR/$APP_ID.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"

# X-AppImage-Version is what Gear Lever and appimageupdate read back.
cat > "$APPDIR/$APP_ID.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$APP_NAME
GenericName=Discord Client
Comment=Native Discord client (GPUI front-end for concord)
Exec=$APP_ID
Icon=$APP_ID
Terminal=false
Categories=Network;InstantMessaging;Chat;
Keywords=discord;chat;voice;concord;
StartupNotify=true
StartupWMClass=$APP_ID
X-AppImage-Version=$VERSION
X-AppImage-Name=$APP_NAME
DESKTOP
cp "$APPDIR/$APP_ID.desktop" "$APPDIR/usr/share/applications/$APP_ID.desktop"

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/bash
HERE="$(dirname "$(readlink -f "${0}")")"
export LD_LIBRARY_PATH="${HERE}/usr/lib${LD_LIBRARY_PATH:+:${LD_LIBRARY_PATH}}"
export PATH="${HERE}/usr/bin:${PATH}"
exec "${HERE}/usr/bin/concord-gui" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

# ---------------------------------------------------------------------------
# Package
# ---------------------------------------------------------------------------
mkdir -p "$OUT_DIR"
TARGET="$OUT_DIR/${APP_NAME}-${VERSION}-x86_64.AppImage"

info "Packaging $TARGET"
ARCH=x86_64 appimagetool "$APPDIR" "$TARGET" || die "appimagetool failed"

chmod +x "$TARGET"
info "Done: $TARGET ($(du -h "$TARGET" | cut -f1))"
