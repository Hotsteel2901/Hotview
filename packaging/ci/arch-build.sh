#!/bin/sh
# Executed inside the Arch Linux container by .github/workflows/desktop.yml.
# The repository is bind-mounted at /src.
set -eu

pacman -Syu --noconfirm --needed \
    rust \
    ffmpeg \
    alsa-lib \
    pkgconf \
    zstd \
    libarchive

export CARGO_HOME=/src/.ci/cargo
cargo build --release --locked -p hotview-desktop
