#!/bin/sh
# Executed inside the Debian (trixie) container by .github/workflows/desktop.yml.
# The repository is bind-mounted at /src.
set -eu

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
    build-essential \
    pkg-config \
    curl \
    ca-certificates \
    xz-utils \
    libavcodec-dev \
    libavformat-dev \
    libavutil-dev \
    libswscale-dev \
    libswresample-dev \
    libasound2-dev \
    libwayland-dev \
    libxkbcommon-dev

export RUSTUP_HOME=/src/.ci/rustup
export CARGO_HOME=/src/.ci/cargo
# The CI cache keeps the toolchain between runs; reinstall only when the
# cached proxy cannot actually find a toolchain.
if ! "$CARGO_HOME/bin/rustc" --version >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
        sh -s -- -y --profile minimal --no-modify-path
fi

"$CARGO_HOME/bin/cargo" build --release --locked -p hotview-desktop
# The WGSL shader is compiled at runtime by wgpu, so a broken shader would
# otherwise only surface as a black window on a real desktop.
"$CARGO_HOME/bin/cargo" test --locked -p hotview-render --test shader
