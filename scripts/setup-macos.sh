#!/usr/bin/env bash
set -euo pipefail

# Ara Marketplace — macOS setup script
# Installs all prerequisites and builds the app in dev mode.
# Usage: bash scripts/setup-macos.sh

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[+]${NC} $1"; }
warn()  { echo -e "${YELLOW}[!]${NC} $1"; }
error() { echo -e "${RED}[x]${NC} $1"; exit 1; }

# ---------- Xcode Command Line Tools ----------
if xcode-select -p &>/dev/null; then
  info "Xcode CLT already installed."
else
  info "Installing Xcode Command Line Tools..."
  xcode-select --install
  echo "Press Enter after the Xcode installer finishes..."
  read -r
fi

# ---------- Homebrew ----------
if command -v brew &>/dev/null; then
  info "Homebrew already installed."
else
  info "Installing Homebrew..."
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  # Add brew to PATH for Apple Silicon
  if [ -f /opt/homebrew/bin/brew ]; then
    eval "$(/opt/homebrew/bin/brew shellenv)"
  fi
fi

# ---------- Rust ----------
if command -v rustc &>/dev/null; then
  info "Rust already installed: $(rustc --version)"
else
  info "Installing Rust via rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  source "$HOME/.cargo/env"
fi

# Ensure Tauri CLI is available
if ! command -v cargo-tauri &>/dev/null; then
  info "Installing Tauri CLI..."
  cargo install tauri-cli --locked
fi

# ---------- Node.js ----------
if command -v node &>/dev/null; then
  info "Node.js already installed: $(node --version)"
else
  info "Installing Node.js..."
  brew install node
fi

# ---------- pnpm ----------
if command -v pnpm &>/dev/null; then
  info "pnpm already installed: $(pnpm --version)"
else
  info "Installing pnpm..."
  npm install -g pnpm
fi

# ---------- Foundry (optional — only needed for contract development) ----------
if command -v forge &>/dev/null; then
  info "Foundry already installed: $(forge --version 2>/dev/null | head -1)"
else
  warn "Foundry (forge) not found. Installing..."
  curl -L https://foundry.paradigm.xyz | bash
  source "$HOME/.foundry/bin/env" 2>/dev/null || true
  foundryup 2>/dev/null || warn "Run 'foundryup' manually after opening a new terminal"
fi

# ---------- Project setup ----------
info "Installing Node.js dependencies..."
pnpm install

# Create app/.env from example if it doesn't exist
if [ ! -f app/.env ]; then
  warn "Creating app/.env — you need to add your WalletConnect project ID."
  echo "VITE_WALLETCONNECT_PROJECT_ID=" > app/.env
  echo "  Get one free at https://cloud.walletconnect.com"
fi

# ---------- Verify ----------
echo ""
info "Setup complete! Versions:"
echo "  Rust:    $(rustc --version 2>/dev/null || echo 'not found')"
echo "  Node:    $(node --version 2>/dev/null || echo 'not found')"
echo "  pnpm:    $(pnpm --version 2>/dev/null || echo 'not found')"
echo "  Forge:   $(forge --version 2>/dev/null | head -1 || echo 'not found')"
echo ""
info "Next steps:"
echo "  1. Edit app/.env and add your VITE_WALLETCONNECT_PROJECT_ID"
echo "  2. Run: pnpm dev"
echo "  3. Connect MetaMask to Sepolia testnet"
