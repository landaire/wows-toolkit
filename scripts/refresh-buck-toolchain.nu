#!/usr/bin/env nu

let toolchain_root = (^nix build --no-link --print-out-paths .#buck-toolchain | str trim)
$"[nix_toolchain]\nroot = ($toolchain_root)\n" | save -f .buckconfig.local
