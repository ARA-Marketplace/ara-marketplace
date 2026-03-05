#!/usr/bin/env bash
#
# Download static ffmpeg + ffprobe binaries for the current platform
# and place them in app/src-tauri/binaries/ with Tauri target-triple naming.
#
# Usage: bash scripts/download-ffmpeg.sh
#
# Sources:
#   Windows/Linux: https://github.com/BtbN/FFmpeg-Builds (GPL static, n7.1 release)
#   macOS:         https://ffmpeg.martin-riedl.de (latest release)

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

echo "Downloading ffmpeg for $TARGET_TRIPLE..."

case "$PLATFORM" in
    windows)
        # BtbN static GPL build (n7.1 release branch)
        if [[ "$ARCH" == "aarch64" || "$ARCH" == "ARM64" ]]; then
            URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-winarm64-gpl-7.1.zip"
        else
            URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-win64-gpl-7.1.zip"
        fi
        echo "  Source: $URL"
        curl -L --progress-bar -o "$TMP_DIR/ffmpeg.zip" "$URL"
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
        curl -L --progress-bar -o "$TMP_DIR/ffprobe.zip" "$FFPROBE_URL"
        unzip -q -o "$TMP_DIR/ffmpeg.zip" -d "$TMP_DIR/ffmpeg_extract"
        unzip -q -o "$TMP_DIR/ffprobe.zip" -d "$TMP_DIR/ffprobe_extract"
        cp "$TMP_DIR/ffmpeg_extract/ffmpeg" "$FFMPEG_OUT"
        cp "$TMP_DIR/ffprobe_extract/ffprobe" "$FFPROBE_OUT"
        chmod +x "$FFMPEG_OUT" "$FFPROBE_OUT"
        ;;

    linux)
        # BtbN static GPL build (n7.1 release branch)
        if [[ "$ARCH" == "aarch64" ]]; then
            URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-linuxarm64-gpl-7.1.tar.xz"
        else
            URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-linux64-gpl-7.1.tar.xz"
        fi
        echo "  Source: $URL"
        curl -L --progress-bar -o "$TMP_DIR/ffmpeg.tar.xz" "$URL"
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
