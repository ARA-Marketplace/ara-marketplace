#!/usr/bin/env bash
#
# Download static ffmpeg + ffprobe binaries for the current platform
# and place them in app/src-tauri/binaries/ with Tauri target-triple naming.
#
# Usage: bash scripts/download-ffmpeg.sh
#
# Sources:
#   Windows/Linux: https://github.com/BtbN/FFmpeg-Builds (LGPL static, n7.1 release)
#   macOS:         https://ffmpeg.martin-riedl.de (latest release)
#
# NOTE: SHA256 checksums in verify_checksum calls below are intentionally empty and
# MUST be populated before use in a production or CI environment. To obtain them:
#   1. Download the archive manually from the URL shown
#   2. Run: sha256sum <archive_file>   (Linux/Windows) or: shasum -a 256 <archive_file>  (macOS)
#   3. Paste the resulting hash into the corresponding verify_checksum call in this script

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARIES_DIR="$PROJECT_ROOT/app/src-tauri/binaries"
TMP_DIR="$(mktemp -d)"

cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT

mkdir -p "$BINARIES_DIR"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

echo "Detected: OS=$OS ARCH=$ARCH"

case "$OS" in
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
        PLATFORM="windows"
        ;;
    Darwin)
        PLATFORM="macos"
        ;;
    Linux)
        PLATFORM="linux"
        ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

# Map to Tauri target triples
case "$PLATFORM-$ARCH" in
    windows-x86_64|windows-AMD64)
        TARGET_TRIPLE="x86_64-pc-windows-msvc"
        EXT=".exe"
        ;;
    windows-aarch64|windows-ARM64)
        TARGET_TRIPLE="aarch64-pc-windows-msvc"
        EXT=".exe"
        ;;
    macos-x86_64)
        TARGET_TRIPLE="x86_64-apple-darwin"
        EXT=""
        ;;
    macos-arm64|macos-aarch64)
        TARGET_TRIPLE="aarch64-apple-darwin"
        EXT=""
        ;;
    linux-x86_64)
        TARGET_TRIPLE="x86_64-unknown-linux-gnu"
        EXT=""
        ;;
    linux-aarch64)
        TARGET_TRIPLE="aarch64-unknown-linux-gnu"
        EXT=""
        ;;
    *)
        echo "Unsupported platform: $PLATFORM-$ARCH"
        exit 1
        ;;
esac

FFMPEG_OUT="$BINARIES_DIR/ffmpeg-${TARGET_TRIPLE}${EXT}"
FFPROBE_OUT="$BINARIES_DIR/ffprobe-${TARGET_TRIPLE}${EXT}"

# Check if already downloaded
if [[ -f "$FFMPEG_OUT" && -f "$FFPROBE_OUT" ]]; then
    echo "FFmpeg binaries already exist:"
    echo "  $FFMPEG_OUT"
    echo "  $FFPROBE_OUT"
    echo "Delete them to re-download."
    exit 0
fi

# SECURITY: Verify downloaded archive integrity via SHA256 checksum.
# To update checksums after a new ffmpeg release:
#   1. Download the archive manually
#   2. Run: sha256sum <archive_file>
#   3. Update the corresponding EXPECTED_SHA256 value below
verify_checksum() {
    local file="$1"
    local expected="$2"
    if [[ -z "$expected" ]]; then
        echo ""
        echo "  *** WARNING: SHA256 checksum is empty — integrity of '$file' has NOT been verified. ***"
        echo "  *** Populate the checksum in download-ffmpeg.sh before using this script in production. ***"
        echo "  *** See the instructions at the top of this script for how to obtain the correct hash. ***"
        echo ""
        return 0
    fi
    local actual
    if command -v sha256sum &>/dev/null; then
        actual=$(sha256sum "$file" | cut -d' ' -f1)
    elif command -v shasum &>/dev/null; then
        actual=$(shasum -a 256 "$file" | cut -d' ' -f1)
    else
        echo "  WARNING: No sha256sum or shasum found. Skipping checksum verification."
        return 0
    fi
    if [[ "$actual" != "$expected" ]]; then
        echo "  SECURITY ERROR: SHA256 checksum mismatch!"
        echo "    Expected: $expected"
        echo "    Got:      $actual"
        echo "  The downloaded file may have been tampered with. Aborting."
        exit 1
    fi
    echo "  Checksum verified: $actual"
}

echo "Downloading ffmpeg for $TARGET_TRIPLE..."

case "$PLATFORM" in
    windows)
        # BtbN static LGPL build (n7.1 release branch)
        if [[ "$ARCH" == "aarch64" || "$ARCH" == "ARM64" ]]; then
            URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-winarm64-lgpl-7.1.zip"
        else
            URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-win64-lgpl-7.1.zip"
        fi
        echo "  Source: $URL"
        curl -L --progress-bar -o "$TMP_DIR/ffmpeg.zip" "$URL"
        verify_checksum "$TMP_DIR/ffmpeg.zip" ""
        # Extract just ffmpeg.exe and ffprobe.exe from the bin/ folder
        unzip -q -o "$TMP_DIR/ffmpeg.zip" -d "$TMP_DIR"
        EXTRACTED_DIR=$(find "$TMP_DIR" -maxdepth 1 -type d -name "ffmpeg-*" | head -1)
        cp "$EXTRACTED_DIR/bin/ffmpeg.exe" "$FFMPEG_OUT"
        cp "$EXTRACTED_DIR/bin/ffprobe.exe" "$FFPROBE_OUT"
        ;;

    macos)
        # martin-riedl.de static builds (latest release)
        if [[ "$ARCH" == "arm64" || "$ARCH" == "aarch64" ]]; then
            MACOS_ARCH="arm64"
        else
            MACOS_ARCH="amd64"
        fi
        FFMPEG_URL="https://ffmpeg.martin-riedl.de/redirect/latest/macos/${MACOS_ARCH}/release/ffmpeg.zip"
        FFPROBE_URL="https://ffmpeg.martin-riedl.de/redirect/latest/macos/${MACOS_ARCH}/release/ffprobe.zip"

        echo "  Source: $FFMPEG_URL"
        curl -L --progress-bar -o "$TMP_DIR/ffmpeg.zip" "$FFMPEG_URL"
        verify_checksum "$TMP_DIR/ffmpeg.zip" ""
        curl -L --progress-bar -o "$TMP_DIR/ffprobe.zip" "$FFPROBE_URL"
        verify_checksum "$TMP_DIR/ffprobe.zip" ""
        unzip -q -o "$TMP_DIR/ffmpeg.zip" -d "$TMP_DIR/ffmpeg_extract"
        unzip -q -o "$TMP_DIR/ffprobe.zip" -d "$TMP_DIR/ffprobe_extract"
        cp "$TMP_DIR/ffmpeg_extract/ffmpeg" "$FFMPEG_OUT"
        cp "$TMP_DIR/ffprobe_extract/ffprobe" "$FFPROBE_OUT"
        chmod +x "$FFMPEG_OUT" "$FFPROBE_OUT"
        ;;

    linux)
        # BtbN static LGPL build (n7.1 release branch)
        if [[ "$ARCH" == "aarch64" ]]; then
            URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-linuxarm64-lgpl-7.1.tar.xz"
        else
            URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-linux64-lgpl-7.1.tar.xz"
        fi
        echo "  Source: $URL"
        curl -L --progress-bar -o "$TMP_DIR/ffmpeg.tar.xz" "$URL"
        verify_checksum "$TMP_DIR/ffmpeg.tar.xz" ""
        tar -xf "$TMP_DIR/ffmpeg.tar.xz" -C "$TMP_DIR"
        EXTRACTED_DIR=$(find "$TMP_DIR" -maxdepth 1 -type d -name "ffmpeg-*" | head -1)
        cp "$EXTRACTED_DIR/bin/ffmpeg" "$FFMPEG_OUT"
        cp "$EXTRACTED_DIR/bin/ffprobe" "$FFPROBE_OUT"
        chmod +x "$FFMPEG_OUT" "$FFPROBE_OUT"
        ;;
esac

echo ""
echo "Done! FFmpeg binaries installed:"
ls -lh "$FFMPEG_OUT" "$FFPROBE_OUT"
echo ""
echo "Target triple: $TARGET_TRIPLE"
