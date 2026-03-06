#!/usr/bin/env bash
#
# Download static ffmpeg + ffprobe binaries for the current (or specified) platform
# and place them in app/src-tauri/binaries/ with Tauri target-triple naming.
#
# Usage:
#   bash scripts/download-ffmpeg.sh                           # auto-detect platform
#   TAURI_TARGET_TRIPLE=x86_64-apple-darwin bash scripts/...  # cross-compile override
#
# Sources (LGPL builds):
#   Windows/Linux: https://github.com/BtbN/FFmpeg-Builds (n7.1 release branch)
#   macOS:         https://ffmpeg.martin-riedl.de (latest snapshot)
#
# Integrity: checksums verified dynamically from upstream (not hardcoded, since
# BtbN "latest" rebuilds daily and checksums change each time).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARIES_DIR="$PROJECT_ROOT/app/src-tauri/binaries"
TMP_DIR="$(mktemp -d)"

cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT

mkdir -p "$BINARIES_DIR"

# Allow CI to override the target triple (for cross-compilation)
if [[ -n "${TAURI_TARGET_TRIPLE:-}" ]]; then
    TARGET_TRIPLE="$TAURI_TARGET_TRIPLE"
    echo "Using CI-provided target: $TARGET_TRIPLE"
else
    # Detect OS and architecture
    OS="$(uname -s)"
    ARCH="$(uname -m)"
    echo "Detected: OS=$OS ARCH=$ARCH"

    case "$OS" in
        MINGW*|MSYS*|CYGWIN*|Windows_NT) PLATFORM="windows" ;;
        Darwin)                           PLATFORM="macos" ;;
        Linux)                            PLATFORM="linux" ;;
        *) echo "Unsupported OS: $OS"; exit 1 ;;
    esac

    case "$PLATFORM-$ARCH" in
        windows-x86_64|windows-AMD64)     TARGET_TRIPLE="x86_64-pc-windows-msvc" ;;
        windows-aarch64|windows-ARM64)    TARGET_TRIPLE="aarch64-pc-windows-msvc" ;;
        macos-x86_64)                     TARGET_TRIPLE="x86_64-apple-darwin" ;;
        macos-arm64|macos-aarch64)        TARGET_TRIPLE="aarch64-apple-darwin" ;;
        linux-x86_64)                     TARGET_TRIPLE="x86_64-unknown-linux-gnu" ;;
        linux-aarch64)                    TARGET_TRIPLE="aarch64-unknown-linux-gnu" ;;
        *) echo "Unsupported platform: $PLATFORM-$ARCH"; exit 1 ;;
    esac
fi

# Derive platform info from target triple
case "$TARGET_TRIPLE" in
    *windows*)  PLATFORM="windows"; EXT=".exe" ;;
    *darwin*)   PLATFORM="macos";   EXT="" ;;
    *linux*)    PLATFORM="linux";   EXT="" ;;
    *) echo "Unknown target triple: $TARGET_TRIPLE"; exit 1 ;;
esac

case "$TARGET_TRIPLE" in
    aarch64*|arm64*) ARCH_CLASS="arm64" ;;
    x86_64*|amd64*)  ARCH_CLASS="x64" ;;
    *) echo "Unknown arch in triple: $TARGET_TRIPLE"; exit 1 ;;
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

# SHA256 verification helper
verify_sha256() {
    local file="$1"
    local expected="$2"
    local actual
    if command -v sha256sum &>/dev/null; then
        actual=$(sha256sum "$file" | cut -d' ' -f1)
    elif command -v shasum &>/dev/null; then
        actual=$(shasum -a 256 "$file" | cut -d' ' -f1)
    else
        echo "  WARNING: No sha256sum/shasum found. Skipping checksum."
        return 0
    fi
    if [[ "$actual" != "$expected" ]]; then
        echo "  SECURITY ERROR: SHA256 mismatch!"
        echo "    Expected: $expected"
        echo "    Got:      $actual"
        exit 1
    fi
    echo "  Checksum OK: ${actual:0:16}..."
}

echo "Downloading ffmpeg for $TARGET_TRIPLE..."

case "$PLATFORM" in
    windows)
        if [[ "$ARCH_CLASS" == "arm64" ]]; then
            ARCHIVE="ffmpeg-n7.1-latest-winarm64-lgpl-7.1.zip"
        else
            ARCHIVE="ffmpeg-n7.1-latest-win64-lgpl-7.1.zip"
        fi
        BASE_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"
        echo "  Source: $BASE_URL/$ARCHIVE"

        # Download checksums and archive
        curl -sL "$BASE_URL/checksums.sha256" -o "$TMP_DIR/checksums.sha256"
        curl -L --progress-bar -o "$TMP_DIR/ffmpeg.zip" "$BASE_URL/$ARCHIVE"

        # Verify integrity
        EXPECTED=$(grep "$ARCHIVE" "$TMP_DIR/checksums.sha256" | cut -d' ' -f1)
        if [[ -n "$EXPECTED" ]]; then
            verify_sha256 "$TMP_DIR/ffmpeg.zip" "$EXPECTED"
        else
            echo "  WARNING: archive not found in checksums file, skipping verification"
        fi

        unzip -q -o "$TMP_DIR/ffmpeg.zip" -d "$TMP_DIR"
        EXTRACTED_DIR=$(find "$TMP_DIR" -maxdepth 1 -type d -name "ffmpeg-*" | head -1)
        cp "$EXTRACTED_DIR/bin/ffmpeg.exe" "$FFMPEG_OUT"
        cp "$EXTRACTED_DIR/bin/ffprobe.exe" "$FFPROBE_OUT"
        ;;

    macos)
        if [[ "$ARCH_CLASS" == "arm64" ]]; then
            MACOS_ARCH="arm64"
        else
            MACOS_ARCH="amd64"
        fi
        FFMPEG_URL="https://ffmpeg.martin-riedl.de/redirect/latest/macos/${MACOS_ARCH}/release/ffmpeg.zip"
        FFPROBE_URL="https://ffmpeg.martin-riedl.de/redirect/latest/macos/${MACOS_ARCH}/release/ffprobe.zip"

        echo "  Source: $FFMPEG_URL"
        curl -L --progress-bar -o "$TMP_DIR/ffmpeg.zip" "$FFMPEG_URL"
        curl -L --progress-bar -o "$TMP_DIR/ffprobe.zip" "$FFPROBE_URL"

        # martin-riedl.de provides .sha256 sidecar files — try to fetch them
        FFMPEG_SHA_URL="$(echo "$FFMPEG_URL" | sed 's|/ffmpeg.zip$|/ffmpeg.zip.sha256|')"
        if curl -sL "$FFMPEG_SHA_URL" -o "$TMP_DIR/ffmpeg.zip.sha256" 2>/dev/null; then
            EXPECTED=$(cat "$TMP_DIR/ffmpeg.zip.sha256" | cut -d' ' -f1)
            if [[ -n "$EXPECTED" && ${#EXPECTED} -eq 64 ]]; then
                verify_sha256 "$TMP_DIR/ffmpeg.zip" "$EXPECTED"
            fi
        else
            echo "  WARNING: could not fetch checksum for macOS ffmpeg, skipping verification"
        fi

        unzip -q -o "$TMP_DIR/ffmpeg.zip" -d "$TMP_DIR/ffmpeg_extract"
        unzip -q -o "$TMP_DIR/ffprobe.zip" -d "$TMP_DIR/ffprobe_extract"
        cp "$TMP_DIR/ffmpeg_extract/ffmpeg" "$FFMPEG_OUT"
        cp "$TMP_DIR/ffprobe_extract/ffprobe" "$FFPROBE_OUT"
        chmod +x "$FFMPEG_OUT" "$FFPROBE_OUT"
        ;;

    linux)
        if [[ "$ARCH_CLASS" == "arm64" ]]; then
            ARCHIVE="ffmpeg-n7.1-latest-linuxarm64-lgpl-7.1.tar.xz"
        else
            ARCHIVE="ffmpeg-n7.1-latest-linux64-lgpl-7.1.tar.xz"
        fi
        BASE_URL="https://github.com/BtbN/FFmpeg-Builds/releases/download/latest"
        echo "  Source: $BASE_URL/$ARCHIVE"

        curl -sL "$BASE_URL/checksums.sha256" -o "$TMP_DIR/checksums.sha256"
        curl -L --progress-bar -o "$TMP_DIR/ffmpeg.tar.xz" "$BASE_URL/$ARCHIVE"

        EXPECTED=$(grep "$ARCHIVE" "$TMP_DIR/checksums.sha256" | cut -d' ' -f1)
        if [[ -n "$EXPECTED" ]]; then
            verify_sha256 "$TMP_DIR/ffmpeg.tar.xz" "$EXPECTED"
        else
            echo "  WARNING: archive not found in checksums file, skipping verification"
        fi

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
