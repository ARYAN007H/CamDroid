#!/bin/bash
# CamDroid Desktop Client — System Dependencies Setup
# Run this script once to install all required system libraries.

set -e

echo "╔══════════════════════════════════════════════╗"
echo "║       CamDroid Desktop Client Setup          ║"
echo "╚══════════════════════════════════════════════╝"
echo ""

# Detect package manager
if command -v apt &> /dev/null; then
    PKG_MGR="apt"
    INSTALL_CMD="sudo apt install -y"
elif command -v pacman &> /dev/null; then
    PKG_MGR="pacman"
    INSTALL_CMD="sudo pacman -S --noconfirm"
elif command -v dnf &> /dev/null; then
    PKG_MGR="dnf"
    INSTALL_CMD="sudo dnf install -y"
else
    echo "Error: Unsupported package manager. Please install dependencies manually."
    exit 1
fi

echo "Detected package manager: $PKG_MGR"
echo ""

# Install dependencies based on distro
echo "📦 Installing system dependencies..."
echo ""

if [ "$PKG_MGR" = "apt" ]; then
    sudo apt update
    $INSTALL_CMD \
        v4l2loopback-dkms \
        v4l2loopback-utils \
        v4l-utils \
        libavcodec-dev \
        libavformat-dev \
        libavutil-dev \
        libswscale-dev \
        libswresample-dev \
        libavfilter-dev \
        libpulse-dev \
        libclang-dev \
        pkg-config \
        android-tools-adb \
        build-essential

elif [ "$PKG_MGR" = "pacman" ]; then
    $INSTALL_CMD \
        v4l2loopback-dkms \
        v4l-utils \
        ffmpeg \
        libpulse \
        clang \
        pkgconf \
        android-tools \
        base-devel

elif [ "$PKG_MGR" = "dnf" ]; then
    $INSTALL_CMD \
        v4l2loopback \
        v4l-utils \
        ffmpeg-devel \
        pulseaudio-libs-devel \
        clang-devel \
        pkgconfig \
        android-tools
fi

echo ""
echo "📹 Setting up v4l2loopback virtual camera..."

# Load the v4l2loopback kernel module
if lsmod | grep -q v4l2loopback; then
    echo "v4l2loopback is already loaded"
else
    sudo modprobe v4l2loopback exclusive_caps=1 video_nr=10 card_label="CamDroid"
    echo "v4l2loopback loaded: /dev/video10 (CamDroid)"
fi

# Verify the device exists
if [ -e /dev/video10 ]; then
    echo "✅ Virtual camera device: /dev/video10"
else
    echo "⚠️  /dev/video10 not found. Try: sudo modprobe v4l2loopback exclusive_caps=1 video_nr=10 card_label=\"CamDroid\""
fi

# Make v4l2loopback persistent across reboots
echo ""
echo "🔄 Making v4l2loopback persistent across reboots..."

echo "v4l2loopback" | sudo tee /etc/modules-load.d/v4l2loopback.conf > /dev/null
echo 'options v4l2loopback exclusive_caps=1 video_nr=10 card_label="CamDroid"' | \
    sudo tee /etc/modprobe.d/v4l2loopback.conf > /dev/null
echo "✅ v4l2loopback will auto-load on boot"

# Add user to video group for /dev/video* access
echo ""
echo "👤 Adding user to 'video' group..."
if groups | grep -q video; then
    echo "Already in 'video' group"
else
    sudo usermod -aG video "$USER"
    echo "Added $USER to 'video' group (log out and back in for this to take effect)"
fi

# Check for Rust toolchain
echo ""
if command -v cargo &> /dev/null; then
    echo "✅ Rust toolchain found: $(rustc --version)"
else
    echo "⚠️  Rust not found. Install it from https://rustup.rs/"
    echo "   Run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

echo ""
echo "╔══════════════════════════════════════════════╗"
echo "║             Setup Complete!                  ║"
echo "╠══════════════════════════════════════════════╣"
echo "║  Next steps:                                ║"
echo "║  1. cd desktop/                             ║"
echo "║  2. cargo build --release                   ║"
echo "║  3. ./target/release/camdroid-client --help  ║"
echo "╚══════════════════════════════════════════════╝"
