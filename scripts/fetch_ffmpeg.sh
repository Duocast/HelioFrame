#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "HelioFrame runtime is Linux/NVIDIA/SDR only."
  echo "This helper currently supports Linux package managers only."
  exit 1
fi

if command -v ffmpeg >/dev/null 2>&1 && command -v ffprobe >/dev/null 2>&1; then
  echo "ffmpeg and ffprobe are already installed."
  exit 0
fi

install_cmd=""
if command -v apt-get >/dev/null 2>&1; then
  install_cmd="sudo apt-get update && sudo apt-get install -y ffmpeg"
elif command -v dnf >/dev/null 2>&1; then
  install_cmd="sudo dnf install -y ffmpeg ffmpeg-devel"
elif command -v pacman >/dev/null 2>&1; then
  install_cmd="sudo pacman -Sy --noconfirm ffmpeg"
elif command -v zypper >/dev/null 2>&1; then
  install_cmd="sudo zypper install -y ffmpeg"
fi

if [[ -z "$install_cmd" ]]; then
  echo "Could not detect a supported package manager."
  echo "Please install ffmpeg/ffprobe manually and ensure both are on PATH."
  exit 1
fi

echo "Installing ffmpeg via detected package manager..."
eval "$install_cmd"

if ! command -v ffmpeg >/dev/null 2>&1 || ! command -v ffprobe >/dev/null 2>&1; then
  echo "Installation completed, but ffmpeg and/or ffprobe were not found on PATH."
  echo "Please verify your package installation and shell PATH."
  exit 1
fi

echo "ffmpeg and ffprobe are installed and available on PATH."
