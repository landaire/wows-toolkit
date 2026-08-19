#!/usr/bin/env nu

if $nu.os-info.name == "macos" {
    let macos_sdk = (^nix build --no-link --print-out-paths .#macos-sdk | str trim)
    nu build-support/check-macos-sdk.nu $macos_sdk
}

let toolchain_root = (^nix build --no-link --print-out-paths .#buck-toolchain | str trim)

let required = [
    "ar"
    "bash"
    "clang"
    "clang++"
    "clippy-driver"
    "ld.lld"
    "llvm-link"
    "nasm"
    "nm"
    "objcopy"
    "objdump"
    "python3"
    "ranlib"
    "rustc"
    "rustdoc"
    "rustfmt"
    "strip"
]

for tool in $required {
    let tool_path = $"($toolchain_root)/bin/($tool)"
    if not ($tool_path | path exists) {
        error make {msg: $"Nix Buck toolchain is missing ($tool_path)."}
    }
}

# Buck reads these instead of resolving any tool through PATH.
[
    "[nix_toolchain]"
    $"root = ($toolchain_root)"
    ""
    "[hermetic_tools]"
    $"ar = ($toolchain_root)/bin/ar"
    $"cc = ($toolchain_root)/bin/clang"
    $"cxx = ($toolchain_root)/bin/clang++"
    $"nasm = ($toolchain_root)/bin/nasm"
    $"python = ($toolchain_root)/bin/python3"
    # The prelude has no rustfmt support; bxl/lint.bxl runs it directly.
    $"rustfmt = ($toolchain_root)/bin/rustfmt"
    ""
] | str join "\n" | save -f .buckconfig.local

# Crate sources are not committed; fetch them against Cargo.lock's checksums.
^$"($toolchain_root)/bin/python3" scripts/fetch-buck-deps.py
