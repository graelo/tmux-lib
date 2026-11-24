#!/bin/bash
# Install the minimum supported tmux on Linux GitHub Actions runners.
set -euo pipefail

version=3.8
sha256=e79c699c7e949dccd0a4a125e17b8d1261e311b16979dbc8eb34542f3966d82e
archive="tmux-${version}.tar.gz"
prefix="${RUNNER_TEMP:?}/tmux"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential bison libevent-dev libncurses-dev pkg-config

cd "$work"
curl -fsSLO "https://github.com/tmux/tmux/releases/download/${version}/${archive}"
printf '%s  %s\n' "$sha256" "$archive" | sha256sum --check
tar xzf "$archive"
cd "tmux-${version}"
./configure --prefix="$prefix"
make -j"$(nproc)"
make install
"$prefix/bin/tmux" -V
printf '%s\n' "$prefix/bin" >> "${GITHUB_PATH:?}"
