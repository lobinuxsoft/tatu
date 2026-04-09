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
cargo tauri build 2>&1 | tail -5

TAURI_APPDIR="src-tauri/target/release/bundle/appimage/${APP_NAME}.AppDir"

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

LINUXDEPLOY="$TOOLS_DIR/linuxdeploy-$APPIMAGE_ARCH.AppImage"
if [ ! -f "$LINUXDEPLOY" ]; then
    echo "  Downloading linuxdeploy..."
    curl -sL -o "$LINUXDEPLOY" \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$APPIMAGE_ARCH.AppImage"
    chmod +x "$LINUXDEPLOY"
    echo -e "  ${GREEN}Downloaded${NC}"
else
    echo -e "  linuxdeploy: ${GREEN}cached${NC}"
fi

# ============================================
# [3/5] Setup AppDir
# ============================================

echo -e "${YELLOW}[3/5]${NC} Setting up AppDir..."

APPDIR="$DIST_DIR/appimage/${DESKTOP_ID}.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"

# Copy binary from Tauri build output
cp "$TAURI_APPDIR/usr/bin/$BINARY_NAME" "$APPDIR/usr/bin/"

# Copy icon
cp "$ICON_SOURCE" "$APPDIR/$DESKTOP_ID.png"

# Create .desktop file
cat > "$APPDIR/$DESKTOP_ID.desktop" << DESKTOP
[Desktop Entry]
Name=$APP_NAME
Comment=Track your Steam game library progress, achievements, trading cards and badges
Exec=$BINARY_NAME
Icon=$DESKTOP_ID
Type=Application
Categories=Game;Utility;
Keywords=steam;games;progress;achievements;trading cards;
Terminal=false
DESKTOP

# ============================================
# [4/5] Bundle libraries
# ============================================

echo -e "${YELLOW}[4/5]${NC} Bundling libraries..."

# Bundle shared libraries with linuxdeploy
echo "  Running linuxdeploy..."
APPIMAGE_EXTRACT_AND_RUN=1 "$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/$BINARY_NAME" \
    --desktop-file "$APPDIR/$DESKTOP_ID.desktop" \
    --icon-file "$APPDIR/$DESKTOP_ID.png"

# --- WebKit binary patching ---
# WebKit hardcodes helper paths at compile time. We binary-patch the .so
# to use relative paths (././ prefix) and cd to AppDir before exec.
echo "  Patching WebKit..."
WEBKIT_HARDCODED=$(strings "$APPDIR/usr/lib/libwebkit2gtk-4.1.so.0" 2>/dev/null | grep -m1 '/webkit2gtk-4.1$' || true)
WEBKIT_DIR=""
for candidate in /usr/libexec/webkit2gtk-4.1 /usr/lib/x86_64-linux-gnu/webkit2gtk-4.1 /usr/lib64/webkit2gtk-4.1; do
    if [ -d "$candidate" ]; then
        WEBKIT_DIR="$candidate"
        break
    fi
done
if [ -z "$WEBKIT_DIR" ]; then
    WKP=$(find /usr -name "WebKitWebProcess" -path "*webkit2gtk*" 2>/dev/null | head -1)
    if [ -n "$WKP" ]; then
        WEBKIT_DIR=$(dirname "$WKP")
    fi
fi
if [ -n "$WEBKIT_DIR" ] && [ -n "$WEBKIT_HARDCODED" ]; then
    WEBKIT_RELATIVE="././${WEBKIT_HARDCODED#/usr}"
    echo "    Path: $WEBKIT_HARDCODED -> $WEBKIT_RELATIVE"
    LC_ALL=C sed -i "s|$WEBKIT_HARDCODED|$WEBKIT_RELATIVE|g" "$APPDIR/usr/lib/libwebkit2gtk-4.1.so.0"
    HELPERS_DEST="$APPDIR${WEBKIT_HARDCODED#/usr}"
    mkdir -p "$HELPERS_DEST"
    cp "$WEBKIT_DIR/WebKitWebProcess" "$HELPERS_DEST/"
    cp "$WEBKIT_DIR/WebKitNetworkProcess" "$HELPERS_DEST/"
    if [ -d "$WEBKIT_DIR/injected-bundle" ]; then
        cp -r "$WEBKIT_DIR/injected-bundle" "$HELPERS_DEST/"
    fi
    echo -e "    ${GREEN}WebKit helpers bundled${NC}"
else
    echo -e "    ${YELLOW}[WARN]${NC} WebKit helpers or library not found"
fi

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

# Bundled library paths
export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH}"
export GIO_MODULE_DIR="${HERE}/usr/lib/gio/modules"
export GDK_PIXBUF_MODULE_FILE="${HERE}/usr/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
export GSETTINGS_SCHEMA_DIR="${HERE}/usr/share/glib-2.0/schemas"

# GStreamer plugin paths (bundled for WebM/VP8/VP9 video playback)
export GST_PLUGIN_PATH="${HERE}/usr/lib/gstreamer-1.0"
export GST_PLUGIN_SYSTEM_PATH=""
export GST_PLUGIN_SCANNER="${HERE}/usr/libexec/gstreamer-1.0/gst-plugin-scanner"
export GST_REGISTRY="${HOME}/.cache/game-progress-tracker/gst-registry.bin"

# WebKit helpers use paths patched to be relative (././ prefix),
# so we must cd to the AppDir root for them to resolve correctly.
cd "${HERE}"

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
Exec=$DEST --run
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
