# Ara Marketplace — Windows setup script
# Installs all prerequisites and builds the app in dev mode.
# Usage: powershell -ExecutionPolicy Bypass -File scripts\setup-windows.ps1
#
# Run this in an ELEVATED (Administrator) PowerShell terminal.

$ErrorActionPreference = "Stop"

function Write-Info($msg)  { Write-Host "[+] $msg" -ForegroundColor Green }
function Write-Warn($msg)  { Write-Host "[!] $msg" -ForegroundColor Yellow }
function Write-Err($msg)   { Write-Host "[x] $msg" -ForegroundColor Red; exit 1 }

# ---------- Check for admin (needed for winget/system installs) ----------
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Warn "Not running as Administrator. Some installs may fail."
    Write-Warn "Re-run with: Start-Process powershell -Verb RunAs -ArgumentList '-File scripts\setup-windows.ps1'"
}

# ---------- WebView2 (required by Tauri) ----------
$webview2 = Get-ItemProperty "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" -ErrorAction SilentlyContinue
if ($webview2) {
    Write-Info "WebView2 already installed."
} else {
    Write-Info "WebView2 not detected. On Windows 10, download from:"
    Write-Host "  https://developer.microsoft.com/en-us/microsoft-edge/webview2/"
    Write-Host "  (Windows 11 includes it by default)"
}

# ---------- Visual Studio Build Tools ----------
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vsWhere) {
    $vsInstall = & $vsWhere -latest -property installationPath 2>$null
    if ($vsInstall) {
        Write-Info "Visual Studio Build Tools found at: $vsInstall"
    }
} else {
    Write-Warn "Visual Studio Build Tools not detected."
    Write-Host "  Install 'Desktop development with C++' workload from:"
    Write-Host "  https://visualstudio.microsoft.com/visual-cpp-build-tools/"
    Write-Host ""
    $response = Read-Host "Continue anyway? (y/n)"
    if ($response -ne 'y') { exit 1 }
}

# ---------- Rust ----------
if (Get-Command rustc -ErrorAction SilentlyContinue) {
    Write-Info "Rust already installed: $(rustc --version)"
} else {
    Write-Info "Installing Rust via winget..."
    winget install Rustlang.Rustup --accept-package-agreements --accept-source-agreements
    # Refresh PATH
    $env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("PATH", "User")
    if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
        Write-Warn "Rust installed but not in PATH yet. You may need to restart your terminal."
    }
}

# Tauri CLI
if (Get-Command cargo-tauri -ErrorAction SilentlyContinue) {
    Write-Info "Tauri CLI already installed."
} else {
    Write-Info "Installing Tauri CLI..."
    cargo install tauri-cli --locked
}

# ---------- Node.js ----------
if (Get-Command node -ErrorAction SilentlyContinue) {
    Write-Info "Node.js already installed: $(node --version)"
} else {
    Write-Info "Installing Node.js via winget..."
    winget install OpenJS.NodeJS.LTS --accept-package-agreements --accept-source-agreements
    $env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("PATH", "User")
}

# ---------- pnpm ----------
if (Get-Command pnpm -ErrorAction SilentlyContinue) {
    Write-Info "pnpm already installed: $(pnpm --version)"
} else {
    Write-Info "Installing pnpm..."
    npm install -g pnpm
}

# ---------- Foundry (optional) ----------
if (Get-Command forge -ErrorAction SilentlyContinue) {
    Write-Info "Foundry already installed."
} else {
    Write-Warn "Foundry (forge) not found. Install from https://getfoundry.sh/ if you need contract development."
}

# ---------- Project setup ----------
Write-Info "Installing Node.js dependencies..."
pnpm install

# Create app/.env if missing
$envFile = Join-Path $PSScriptRoot "..\app\.env"
if (-not (Test-Path $envFile)) {
    Write-Warn "Creating app/.env — you need to add your WalletConnect project ID."
    "VITE_WALLETCONNECT_PROJECT_ID=" | Out-File -FilePath $envFile -Encoding utf8
    Write-Host "  Get one free at https://cloud.walletconnect.com"
}

# ---------- Verify ----------
Write-Host ""
Write-Info "Setup complete! Versions:"
Write-Host "  Rust:    $(if (Get-Command rustc -EA SilentlyContinue) { rustc --version } else { 'not found' })"
Write-Host "  Node:    $(if (Get-Command node -EA SilentlyContinue) { node --version } else { 'not found' })"
Write-Host "  pnpm:    $(if (Get-Command pnpm -EA SilentlyContinue) { pnpm --version } else { 'not found' })"
Write-Host "  Forge:   $(if (Get-Command forge -EA SilentlyContinue) { forge --version 2>$null } else { 'not found' })"
Write-Host ""
Write-Info "Next steps:"
Write-Host "  1. Edit app\.env and add your VITE_WALLETCONNECT_PROJECT_ID"
Write-Host "  2. Run: pnpm dev"
Write-Host "  3. Connect MetaMask to Sepolia testnet"
