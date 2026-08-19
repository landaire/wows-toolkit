#!/usr/bin/env bash
# Enlarge swap on a hosted Linux runner.
#
# Release-mode wasm codegen of wt-web runs cross-crate ThinLTO over the whole
# egui/eframe/wgpu graph in a single rustc, and its peak exceeds the 16 GB of a
# hosted runner. The runner is reclaimed with exit 143 and no diagnostic, which
# also skips post-steps, so the rust-cache is never written either.
#
# Capping cargo's jobserver does not help: the peak is one process, not several.
# Swap absorbs it without changing the profile, so the deployed artifact stays
# byte-for-byte what the release profile produces.
set -euo pipefail

size="${1:-16G}"
# /mnt is the runner's large ephemeral disk; / has far less headroom.
swapfile=/mnt/swapfile

echo "Memory and swap before:"
free -h

sudo swapoff -a
sudo rm -f "$swapfile"
sudo fallocate -l "$size" "$swapfile"
sudo chmod 600 "$swapfile"
sudo mkswap "$swapfile"
sudo swapon "$swapfile"

# Default 60 evicts page cache long before anonymous pages, which is wrong when
# one process legitimately needs more anonymous memory than RAM.
sudo sysctl -w vm.swappiness=10

echo "Memory and swap after:"
free -h

available_swap=$(free -g | awk '/^Swap:/ { print $2 }')
want=${size%G}
if [ "$available_swap" -lt "$((want - 1))" ]; then
  echo "Expected roughly ${size} of swap, found ${available_swap}G." >&2
  exit 1
fi
