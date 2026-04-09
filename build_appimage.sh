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

echo -e "${YELLOW}[1/4]${NC} Building release binary..."
cargo tauri build 2>&1 | tail -5

APPDIR="src-tauri/target/release/bundle/appimage/${APP_NAME}.AppDir"

if [ ! -d "$APPDIR" ]; then
    echo -e "${RED}[ERROR]${NC} AppDir not found at: $APPDIR"
    exit 1
fi

echo -e "${YELLOW}[2/4]${NC} Setting up AppDir..."

# Copy icon to AppDir root (must match Icon= in .desktop)
cp "$ICON_SOURCE" "$APPDIR/$BINARY_NAME.png"
cp "$ICON_SOURCE" "$APPDIR/$DESKTOP_ID.png"

# Replace AppRun with self-installing version
rm -f "$APPDIR/AppRun"
cat > "$APPDIR/AppRun" << 'APPRUN'
#!/bin/bash
SELF=$(readlink -f "$0")
HERE=${SELF%/*}
APPIMAGE="${APPIMAGE:-$SELF}"
APP_NAME="Game Progress Tracker"
BINARY_NAME="game-progress-tracker"
DESKTOP_ID="com.lobinux.game-progress-tracker"
INSTALL_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons"

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

    # Create .desktop entry (escape spaces in Exec path)
    local exec_path="${DEST// /\\ }"
    cat > "$DESKTOP_DIR/$DESKTOP_ID.desktop" << DESKTOP
[Desktop Entry]
Name=$APP_NAME
Comment=Track your Steam game library progress, achievements, trading cards and badges
Exec="${DEST}" --run
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

# Run the app directly
exec "${HERE}/usr/bin/$BINARY_NAME" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

echo -e "${YELLOW}[3/4]${NC} Building AppImage..."

# Download appimagetool if needed
APPIMAGETOOL="/tmp/appimagetool"
if [ ! -f "$APPIMAGETOOL" ]; then
    echo "  Downloading appimagetool..."
    curl -sL "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage" -o "$APPIMAGETOOL"
    chmod +x "$APPIMAGETOOL"
fi

OUTPUT="src-tauri/target/release/bundle/appimage/${APP_NAME}_$(grep '^version' src-tauri/Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')_amd64.AppImage"
APPIMAGE_EXTRACT_AND_RUN=1 "$APPIMAGETOOL" "$APPDIR" "$OUTPUT" 2>&1 | tail -3

echo -e "${YELLOW}[4/4]${NC} Done!"
echo ""
echo -e "  ${GREEN}AppImage:${NC} $OUTPUT"
echo -e "  Size: $(du -h "$OUTPUT" | cut -f1)"
echo ""
echo "  To install: ./$OUTPUT --install"
echo "  Or double-click the AppImage to get an install prompt."
