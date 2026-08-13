#!/usr/bin/env nu

nu build-support/check-xcode.nu

let toolchain_root = (^nix build --no-link --print-out-paths .#buck-toolchain | str trim)

for tool in [
    "ar"
    "bash"
    "clang"
    "clang++"
    "clippy-driver"
    "llvm-link"
    "nm"
    "objcopy"
    "objdump"
    "python3"
    "ranlib"
    "rustc"
    "rustdoc"
    "strip"
] {
    let tool_path = $"($toolchain_root)/bin/($tool)"
    if not ($tool_path | path exists) {
        error make $"Nix Buck toolchain is missing ($tool_path)."
    }
}

$"[nix_toolchain]\nroot = ($toolchain_root)\n" | save -f .buckconfig.local
