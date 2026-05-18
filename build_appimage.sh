#!/bin/bash
set -e

APP_NAME="Game Progress Tracker"
BINARY_NAME="game-progress-tracker"
DESKTOP_ID="com.lobinux.game-progress-tracker"
ICON_SOURCE="src-tauri/icons/256x256.png"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

TOOLS_DIR="$ROOT_DIR/.tools"
DIST_DIR="$ROOT_DIR/dist"

# Determine architecture
ARCH=$(uname -m)
case $ARCH in
    x86_64)  APPIMAGE_ARCH="x86_64" ;;
    aarch64) APPIMAGE_ARCH="aarch64" ;;
    *)
        echo -e "${RED}[ERROR]${NC} Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

# ============================================
# [1/5] Build release binary
# ============================================

echo -e "${YELLOW}[1/5]${NC} Building release binary..."
# NO_STRIP=1: Tauri's internal linuxdeploy invocation calls `strip` on every
# bundled `.so` without honouring host capabilities. The bundled binutils
# strip (from the linuxdeploy AppImage) does not parse RELR relocations
# (Fedora 40+, Bazzite). Without NO_STRIP, every system .so fails strip and
# Tauri aborts the bundle with `failed to run linuxdeploy`, leaving no
# AppDir / AppImage to consume.
NO_STRIP=1 cargo tauri build 2>&1 | tail -5

# Tauri's bundle output sits at the workspace root, not at src-tauri/target,
# since this is a multi-crate workspace (`cheat-runtime`, `ce-launcher`, …).
TAURI_APPDIR="target/release/bundle/appimage/${APP_NAME}.AppDir"

if [ ! -d "$TAURI_APPDIR" ]; then
    echo -e "${RED}[ERROR]${NC} Tauri AppDir not found at: $TAURI_APPDIR"
    exit 1
fi

# ============================================
# [2/5] Download tools
# ============================================

echo -e "${YELLOW}[2/5]${NC} Downloading tools..."

mkdir -p "$TOOLS_DIR"

APPIMAGETOOL="$TOOLS_DIR/appimagetool-$APPIMAGE_ARCH.AppImage"
if [ ! -f "$APPIMAGETOOL" ]; then
    echo "  Downloading appimagetool..."
    curl -sL -o "$APPIMAGETOOL" \
        "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-$APPIMAGE_ARCH.AppImage"
    chmod +x "$APPIMAGETOOL"
    echo -e "  ${GREEN}Downloaded${NC}"
else
    echo -e "  appimagetool: ${GREEN}cached${NC}"
fi

# linuxdeploy intentionally NOT used here. Its rpath-patching step (patchelf
# under the hood) corrupts ELF DT_INIT pointers on every bundled .so: DT_INIT
# is left pointing to the old vaddr while the actual `.init` section is moved
# to a new file offset. ld.so then jumps into stale data at dl_init time and
# the process SIGSEGVs before main runs (#65, e.g. `libXau.so.6 + 0x2cc` and
# `libmp3lame.so.0 + 0x294` on Bazzite F44).
#
# Tauri's own bundle (`cargo tauri build`) produces an uncorrupted AppDir with
# 144 libs already resolved — we use that as the base and only supplement
# what Tauri doesn't ship (GIO modules, GLib schemas, GStreamer plugins,
# WebKit binary path patch, custom AppRun).

# ============================================
# [3/5] Setup AppDir
# ============================================

echo -e "${YELLOW}[3/5]${NC} Setting up AppDir..."

APPDIR="$DIST_DIR/appimage/${DESKTOP_ID}.AppDir"
rm -rf "$APPDIR"
mkdir -p "$DIST_DIR/appimage"
# Copy Tauri's complete AppDir verbatim — libs are already correctly bundled.
cp -a "$TAURI_APPDIR" "$APPDIR"

# Tauri ships AppRun + .desktop + icon at the AppDir root. We overwrite the
# AppRun with our own at step [4/5] (adds --install/--uninstall + env setup);
# the root .desktop/.png it leaves are fine.

# ============================================
# [4/5] Bundle libraries
# ============================================

echo -e "${YELLOW}[4/5]${NC} Bundling libraries..."

# WebKit binary path patching is handled by Tauri's bundle phase (via
# `linuxdeploy-plugin-gtk`), which rewrites the hardcoded helper path in
# `libwebkit2gtk-4.1.so.0` from `/usr/libexec/webkit2gtk-4.1` to
# `././/libexec/webkit2gtk-4.1` and bundles the helper binaries at
# `usr/libexec/webkit2gtk-4.1/`. Re-running the same `sed` here would
# match the already-patched path and emit a longer string, shifting ELF
# layout and corrupting the .so (#65 follow-up). Trust Tauri's patch.

# --- GLib compiled schemas ---
if [ -f "/usr/share/glib-2.0/schemas/gschemas.compiled" ]; then
    echo "  Bundling GLib schemas..."
    mkdir -p "$APPDIR/usr/share/glib-2.0/schemas"
    cp /usr/share/glib-2.0/schemas/gschemas.compiled "$APPDIR/usr/share/glib-2.0/schemas/"
fi

# --- GIO modules (TLS support) ---
GIO_DIR=$(pkg-config --variable=giomoduledir gio-2.0 2>/dev/null || true)
if [ -z "$GIO_DIR" ] || [ ! -d "$GIO_DIR" ]; then
    for candidate in /usr/lib/x86_64-linux-gnu/gio/modules /usr/lib64/gio/modules; do
        if [ -d "$candidate" ]; then
            GIO_DIR="$candidate"
            break
        fi
    done
fi
if [ -n "$GIO_DIR" ] && [ -d "$GIO_DIR" ]; then
    echo "  Bundling GIO modules from $GIO_DIR..."
    mkdir -p "$APPDIR/usr/lib/gio/modules"
    cp "$GIO_DIR"/*.so "$APPDIR/usr/lib/gio/modules/"
fi

# --- GDK pixbuf loaders ---
PIXBUF_MODULE_DIR=$(pkg-config --variable=gdk_pixbuf_moduledir gdk-pixbuf-2.0 2>/dev/null || true)
PIXBUF_DIR=""
if [ -n "$PIXBUF_MODULE_DIR" ] && [ -d "$PIXBUF_MODULE_DIR" ]; then
    PIXBUF_DIR=$(dirname "$PIXBUF_MODULE_DIR")
else
    for candidate in /usr/lib/x86_64-linux-gnu/gdk-pixbuf-2.0/2.10.0 /usr/lib64/gdk-pixbuf-2.0/2.10.0; do
        if [ -d "$candidate" ]; then
            PIXBUF_DIR="$candidate"
            break
        fi
    done
fi
if [ -n "$PIXBUF_DIR" ] && compgen -G "$PIXBUF_DIR/loaders/*.so" > /dev/null 2>&1; then
    echo "  Bundling GDK pixbuf loaders..."
    mkdir -p "$APPDIR/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders"
    cp "$PIXBUF_DIR/loaders"/*.so "$APPDIR/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders/"
    if command -v gdk-pixbuf-query-loaders &>/dev/null; then
        GDK_PIXBUF_MODULEDIR="$APPDIR/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders" \
            gdk-pixbuf-query-loaders > "$APPDIR/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
    fi
fi

# --- GStreamer plugins (dlopen-loaded, invisible to ldd) ---
GST_PLUGIN_DIR=$(pkg-config --variable=pluginsdir gstreamer-1.0 2>/dev/null || true)
if [ -z "$GST_PLUGIN_DIR" ] || [ ! -d "$GST_PLUGIN_DIR" ]; then
    for candidate in /usr/lib64/gstreamer-1.0 /usr/lib/x86_64-linux-gnu/gstreamer-1.0; do
        if [ -d "$candidate" ]; then
            GST_PLUGIN_DIR="$candidate"
            break
        fi
    done
fi
if [ -n "$GST_PLUGIN_DIR" ] && [ -d "$GST_PLUGIN_DIR" ]; then
    echo "  Bundling GStreamer plugins from $GST_PLUGIN_DIR..."
    mkdir -p "$APPDIR/usr/lib/gstreamer-1.0"
    GST_PLUGINS=(
        coreelements typefindfunctions app playback matroska vpx
        opus vorbis ogg audioconvert audioresample videoconvertscale
        autodetect volume opengl gio
    )
    for plugin in "${GST_PLUGINS[@]}"; do
        FOUND=$(compgen -G "$GST_PLUGIN_DIR/libgst${plugin}.*" 2>/dev/null | head -1)
        if [ -n "$FOUND" ]; then
            cp "$FOUND" "$APPDIR/usr/lib/gstreamer-1.0/"
        else
            echo -e "    ${YELLOW}[WARN]${NC} GStreamer plugin not found: $plugin"
        fi
    done

    # Bundle GStreamer plugin scanner
    GST_SCANNER=""
    GST_SCANNER_DIR=$(pkg-config --variable=pluginscannerdir gstreamer-1.0 2>/dev/null || true)
    if [ -n "$GST_SCANNER_DIR" ] && [ -x "$GST_SCANNER_DIR/gst-plugin-scanner" ]; then
        GST_SCANNER="$GST_SCANNER_DIR/gst-plugin-scanner"
    elif [ -x "/usr/libexec/gstreamer-1.0/gst-plugin-scanner" ]; then
        GST_SCANNER="/usr/libexec/gstreamer-1.0/gst-plugin-scanner"
    else
        GST_SCANNER=$(find /usr -name "gst-plugin-scanner" -path "*/gstreamer-1.0/*" 2>/dev/null | head -1)
    fi
    if [ -n "$GST_SCANNER" ]; then
        mkdir -p "$APPDIR/usr/libexec/gstreamer-1.0"
        cp "$GST_SCANNER" "$APPDIR/usr/libexec/gstreamer-1.0/"
        echo "    GStreamer scanner bundled from $GST_SCANNER"
    else
        echo -e "    ${YELLOW}[WARN]${NC} GStreamer plugin scanner not found"
    fi
else
    echo -e "  ${YELLOW}[WARN]${NC} GStreamer plugin directory not found, skipping"
fi

echo -e "  ${GREEN}Libraries bundled${NC}"

# --- Create AppRun with environment variables and install support ---
rm -f "$APPDIR/AppRun"
cat > "$APPDIR/AppRun" << 'APPRUN'
#!/bin/bash
SELF=$(readlink -f "$0")
HERE=${SELF%/*}
APPIMAGE="${APPIMAGE:-$SELF}"
APP_NAME="__DISPLAY_NAME__"
BINARY_NAME="__BINARY_NAME__"
DESKTOP_ID="__DESKTOP_ID__"
INSTALL_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons"

# Setup bundled library environment. Called only before running the app
# binary, NOT before system tools like zenity/kdialog which break when
# they pick up our bundled GStreamer/GTK libs.
setup_env() {
    export APPDIR="${HERE}"
    export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH}"
    # WebKit binary patching strips `/usr` from helper paths, turning
    # /usr/libexec/webkit2gtk-4.1/WebKitNetworkProcess into ././/libexec/...
    # GTK_EXE_PREFIX="$APPDIR//usr" rejoins the missing prefix so the
    # helper resolves to $APPDIR/usr/libexec/webkit2gtk-4.1/...
    export GTK_EXE_PREFIX="${HERE}//usr"
    export GTK_PATH="${HERE}//usr/lib64/gtk-3.0:${HERE}//usr/lib/gtk-3.0:/usr/lib64/gtk-3.0:/usr/lib/gtk-3.0"
    # WebKit2GTK crashes under the GDK Wayland backend in AppImage:
    # https://github.com/tauri-apps/tauri/issues/8541 — force X11 (XWayland
    # is available on every Wayland session) which is what Tauri's stock
    # linuxdeploy-plugin-gtk does for the same reason.
    export GDK_BACKEND=x11
    export GIO_MODULE_DIR="${HERE}/usr/lib/gio/modules"
    export GIO_EXTRA_MODULES="${HERE}/usr/lib64/gio/modules:${HERE}/usr/lib/gio/modules"
    export GDK_PIXBUF_MODULE_FILE="${HERE}/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
    export GSETTINGS_SCHEMA_DIR="${HERE}//usr/share/glib-2.0/schemas"
    export XDG_DATA_DIRS="${HERE}/usr/share:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
    export GST_PLUGIN_PATH="${HERE}/usr/lib/gstreamer-1.0"
    export GST_PLUGIN_SYSTEM_PATH=""
    export GST_PLUGIN_SCANNER="${HERE}/usr/libexec/gstreamer-1.0/gst-plugin-scanner"
    export GST_REGISTRY="${HOME}/.cache/game-progress-tracker/gst-registry.bin"
    # cd into $APPDIR/usr so the webkit helper path `././/libexec/...`
    # resolves relative to AppDir/usr (where the helpers actually live).
    cd "${HERE}/usr"
}

install_app() {
    echo "Installing $APP_NAME..."

    mkdir -p "$INSTALL_DIR" "$DESKTOP_DIR" "$ICON_DIR"

    # Move AppImage
    DEST="$INSTALL_DIR/$(basename "$APPIMAGE")"
    if [ "$APPIMAGE" != "$DEST" ]; then
        mv "$APPIMAGE" "$DEST"
        chmod +x "$DEST"
        echo "  Moved to: $DEST"
    fi

    # Extract and install icon
    "$DEST" --appimage-extract "$DESKTOP_ID.png" >/dev/null 2>&1
    if [ -f "squashfs-root/$DESKTOP_ID.png" ]; then
        cp "squashfs-root/$DESKTOP_ID.png" "$ICON_DIR/$DESKTOP_ID.png"
        rm -rf squashfs-root
        echo "  Icon installed"
    fi

    # Create .desktop entry
    cat > "$DESKTOP_DIR/$DESKTOP_ID.desktop" << DESKTOP
[Desktop Entry]
Name=$APP_NAME
Comment=Track your Steam game library progress, achievements, trading cards and badges
Exec="$DEST" --run
Icon=$ICON_DIR/$DESKTOP_ID.png
Type=Application
Categories=Game;Utility;
Keywords=steam;games;progress;achievements;trading cards;
Terminal=false
DESKTOP
    echo "  Desktop entry created"
    echo ""
    echo "Installation complete! You can find the app in your application menu."
}

uninstall_app() {
    echo "Uninstalling $APP_NAME..."
    find "$INSTALL_DIR" -maxdepth 1 -iname "*${BINARY_NAME}*.AppImage" -delete 2>/dev/null
    rm -f "$DESKTOP_DIR/$DESKTOP_ID.desktop"
    rm -f "$ICON_DIR/$DESKTOP_ID.png"
    rm -rf "${HOME}/.cache/game-progress-tracker"
    echo "Uninstalled."
}

case "$1" in
    --install)
        install_app
        exit 0
        ;;
    --uninstall)
        uninstall_app
        exit 0
        ;;
    --run)
        shift
        setup_env
        export PATH="${HERE}/usr/bin:${PATH}"
        exec "${HERE}/usr/bin/$BINARY_NAME" "$@"
        ;;
    --help)
        echo "Usage: $(basename "$APPIMAGE") [OPTIONS]"
        echo ""
        echo "Options:"
        echo "  --install     Install to ~/.local/bin and create desktop entry"
        echo "  --uninstall   Remove installation"
        echo "  --run         Run without install check (used by .desktop)"
        echo "  --help        Show this help"
        exit 0
        ;;
esac

# Auto-install prompt when double-clicked (no terminal attached)
if [[ "$APPIMAGE" != "$INSTALL_DIR"/* ]] && [ ! -t 0 ]; then
    if command -v zenity &>/dev/null; then
        if zenity --question --title="Install $APP_NAME" \
            --text="Install to ~/.local/bin and create menu entry?" \
            --width=300 2>/dev/null; then
            install_app
            exec "$INSTALL_DIR/$(basename "$APPIMAGE")" --run "$@"
        fi
    elif command -v kdialog &>/dev/null; then
        if kdialog --yesno "Install to ~/.local/bin and create menu entry?" \
            --title "Install $APP_NAME" 2>/dev/null; then
            install_app
            exec "$INSTALL_DIR/$(basename "$APPIMAGE")" --run "$@"
        fi
    fi
fi

# Run the app
setup_env
export PATH="${HERE}/usr/bin:${PATH}"
exec "${HERE}/usr/bin/$BINARY_NAME" "$@"
APPRUN

# Replace template placeholders
sed -i "s/__BINARY_NAME__/$BINARY_NAME/g" "$APPDIR/AppRun"
sed -i "s/__DISPLAY_NAME__/$APP_NAME/g" "$APPDIR/AppRun"
sed -i "s/__DESKTOP_ID__/$DESKTOP_ID/g" "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"

# ============================================
# [5/5] Generate AppImage
# ============================================

echo -e "${YELLOW}[5/5]${NC} Generating AppImage..."

VERSION=$(grep '^version' src-tauri/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
OUTPUT="$DIST_DIR/appimage/${APP_NAME}_${VERSION}_${APPIMAGE_ARCH}.AppImage"
mkdir -p "$DIST_DIR/appimage"

ARCH=$APPIMAGE_ARCH APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" "$APPDIR" "$OUTPUT" 2>&1 | tail -3

# Cleanup AppDir
rm -rf "$APPDIR"

echo ""
echo -e "  ${GREEN}AppImage:${NC} $OUTPUT"
echo -e "  Size: $(du -h "$OUTPUT" | cut -f1)"
echo ""
echo "  To install: ./$OUTPUT --install"
echo "  Or double-click the AppImage to get an install prompt."
