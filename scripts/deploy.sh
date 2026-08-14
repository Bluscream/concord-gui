#!/usr/bin/env bash
#
# Build, install and launch concord-gui.
#
# Everything is user-scoped: no root, no system paths. Undo with --uninstall.
#
#   ./scripts/deploy.sh              build release, install, launch
#   ./scripts/deploy.sh --test       launch with offline fixture data (token "test")
#   ./scripts/deploy.sh --debug      build the debug profile (much faster to compile)
#   ./scripts/deploy.sh --no-run     install without launching
#   ./scripts/deploy.sh --run-only   skip the build, just launch what is installed
#   ./scripts/deploy.sh --uninstall  remove binary, shortcuts and icon
#
# The build runs inside the Arch distrobox because the host (Bazzite) is an
# immutable OS without cmake, which opusic-sys requires. The resulting binary
# runs fine on the host, so the shortcuts launch it directly.

set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BOX="arch"
APP_ID="concord-gui"
APP_NAME="Concord"

BIN_DIR="$HOME/.local/bin"
ICON_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"
MENU_DIR="$HOME/.local/share/applications"
DESKTOP_DIR="$(xdg-user-dir DESKTOP 2>/dev/null || echo "$HOME/Desktop")"

PROFILE="release"
FEATURES="fixtures"
RUN=1
BUILD=1
TEST_MODE=0

info()  { printf '\033[1;34m::\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m!!\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

uninstall() {
    info "Removing concord-gui"
    rm -fv "$BIN_DIR/$APP_ID" \
           "$MENU_DIR/$APP_ID.desktop" \
           "$DESKTOP_DIR/$APP_ID.desktop" \
           "$ICON_DIR/$APP_ID.svg" 2>/dev/null || true
    command -v update-desktop-database >/dev/null 2>&1 &&
        update-desktop-database "$MENU_DIR" 2>/dev/null || true
    info "Done. Config and credentials in ~/.config/concord were left alone."
    exit 0
}

for arg in "$@"; do
    case "$arg" in
        --test)      TEST_MODE=1 ;;
        --debug)     PROFILE="debug" ;;
        --no-run)    RUN=0 ;;
        --run-only)  BUILD=0 ;;
        --uninstall) uninstall ;;
        -h|--help)   sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \?//'; exit 0 ;;
        *)           die "unknown option: $arg (try --help)" ;;
    esac
done

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------

if [[ $BUILD -eq 1 ]]; then
    command -v distrobox >/dev/null 2>&1 || die "distrobox not found"
    # Captured rather than piped into grep: `grep -q` exits on first match and
    # closes the pipe, so `distrobox list` dies with SIGPIPE and `pipefail`
    # reports the whole pipeline as failed - turning a successful match into a
    # false "not found".
    BOX_LIST="$(distrobox list 2>/dev/null || true)"
    grep -q "| *$BOX *|" <<<"$BOX_LIST" ||
        die "distrobox '$BOX' not found. Create it with: distrobox create --name $BOX --image archlinux:latest"

    # A duplicate [build] table here breaks every cargo invocation, and this
    # file has been rewritten by other tooling before. Fail loudly rather than
    # emitting a confusing manifest-parse error.
    CARGO_CONFIG="$HOME/.cargo/config.toml"
    if [[ -f "$CARGO_CONFIG" ]] && [[ $(grep -c '^\[build\]' "$CARGO_CONFIG") -gt 1 ]]; then
        die "$CARGO_CONFIG has duplicate [build] tables (invalid TOML). Fix it, then re-run."
    fi

    BUILD_ARGS=(build -p concord-gui --features "$FEATURES")
    [[ "$PROFILE" == "release" ]] && BUILD_ARGS+=(--release)

    info "Building ($PROFILE, features: $FEATURES) in distrobox '$BOX'"
    distrobox enter "$BOX" -- bash -lc "cd '$REPO' && cargo ${BUILD_ARGS[*]}" ||
        die "build failed"
fi

BINARY="$REPO/target/$PROFILE/$APP_ID"
[[ -x "$BINARY" ]] || die "binary not found at $BINARY (run without --run-only first)"

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------

mkdir -p "$BIN_DIR" "$ICON_DIR" "$MENU_DIR" "$DESKTOP_DIR"

info "Installing binary to $BIN_DIR/$APP_ID"
install -m755 "$BINARY" "$BIN_DIR/$APP_ID"

# Self-contained icon so the shortcut is never a blank square.
cat > "$ICON_DIR/$APP_ID.svg" <<'SVG'
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <rect width="64" height="64" rx="14" fill="#181b1f"/>
  <path d="M20 24h24a4 4 0 0 1 4 4v12a4 4 0 0 1-4 4H28l-8 6V24z"
        fill="none" stroke="#5b8def" stroke-width="3" stroke-linejoin="round"/>
  <circle cx="27" cy="34" r="2.5" fill="#5b8def"/>
  <circle cx="35" cy="34" r="2.5" fill="#5b8def"/>
  <circle cx="43" cy="34" r="2.5" fill="#5b8def"/>
</svg>
SVG

write_desktop_entry() {
    local target="$1"
    cat > "$target" <<DESKTOP
[Desktop Entry]
Type=Application
Name=$APP_NAME
GenericName=Discord Client
Comment=Native Discord client (GPUI front-end for concord)
Exec=$BIN_DIR/$APP_ID
Icon=$APP_ID
Terminal=false
Categories=Network;InstantMessaging;Chat;
Keywords=discord;chat;voice;concord;
StartupNotify=true
StartupWMClass=$APP_ID

Actions=TestMode;

[Desktop Action TestMode]
Name=Launch with test data (offline)
Exec=env CONCORD_TOKEN=test $BIN_DIR/$APP_ID
DESKTOP
    chmod +x "$target"
}

info "Adding application-menu entry"
write_desktop_entry "$MENU_DIR/$APP_ID.desktop"

info "Adding desktop shortcut"
write_desktop_entry "$DESKTOP_DIR/$APP_ID.desktop"
# GNOME and KDE both require the launcher to be marked trusted, or it shows as
# an unlaunchable text file.
command -v gio >/dev/null 2>&1 &&
    gio set "$DESKTOP_DIR/$APP_ID.desktop" metadata::trusted true 2>/dev/null || true

command -v update-desktop-database >/dev/null 2>&1 &&
    update-desktop-database "$MENU_DIR" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 &&
    gtk-update-icon-cache -qtf "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

if ! printf '%s' ":$PATH:" | grep -q ":$BIN_DIR:"; then
    warn "$BIN_DIR is not on PATH; the shortcuts still work (they use a full path)."
fi

# ---------------------------------------------------------------------------
# Launch
# ---------------------------------------------------------------------------

info "Installed:"
printf '   binary   %s\n'  "$BIN_DIR/$APP_ID"
printf '   menu     %s\n'  "$MENU_DIR/$APP_ID.desktop"
printf '   desktop  %s\n'  "$DESKTOP_DIR/$APP_ID.desktop"

if [[ $RUN -eq 1 ]]; then
    if [[ $TEST_MODE -eq 1 ]]; then
        info "Launching with offline fixture data (no account, no network)"
        exec env CONCORD_TOKEN=test "$BIN_DIR/$APP_ID"
    else
        info "Launching. No credential? Paste a token on the login screen,"
        info "or re-run with --test for offline sample data."
        exec "$BIN_DIR/$APP_ID"
    fi
fi
