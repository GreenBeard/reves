#!/usr/bin/env bash

set -euo pipefail

deb_pkgs=(
  "build-essential"
  "ca-certificates"
  "curl"
  "gcc"
  "git"
  "make"
  "perl"
  "rustfmt"
  "shellcheck"
)

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends "${deb_pkgs[@]}"
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.74.0
# shellcheck disable=SC1091
. "$HOME/.cargo/env"
rustup toolchain install 1.63.0
cargo install --locked cargo-deny@0.13.5
