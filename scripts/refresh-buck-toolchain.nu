#!/usr/bin/env nu

if $nu.os-info.name == "macos" {
    let macos_sdk = (^nix build --no-link --print-out-paths .#macos-sdk | str trim)
    nu build-support/check-macos-sdk.nu $macos_sdk
}

# .buckconfig.local names the store path directly, so keep a GC root for it;
# without one, `nix-collect-garbage` deletes the toolchain out from under Buck.
let toolchain_root = (^nix build --out-link .tools/buck-toolchain --print-out-paths .#buck-toolchain | str trim)

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
    # wt-web: its C compiler, linker and archiver, then the site's tools.
    "wasm32-unknown-unknown-clang"
    "wasm-ld"
    "llvm-ar"
    "wasm-bindgen"
    "wasm-opt"
]

for tool in $required {
    let tool_path = $"($toolchain_root)/bin/($tool)"
    if not ($tool_path | path exists) {
        error make {msg: $"Nix Buck toolchain is missing ($tool_path)."}
    }
}

let system_libs_root = (^nix build --out-link .tools/buck-system-libs --print-out-paths .#buck-system-libs | str trim)

# The Windows executables the Linux launcher runs under Wine. Other hosts leave
# the section out, which makes those targets incompatible there.
let windows_cross_root = if $nu.os-info.name == "linux" {
    ^nix build --out-link .tools/buck-windows-cross --print-out-paths .#buck-windows-cross | str trim
} else {
    null
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
    # The prelude has no rustfmt support, so toolchains//:rustfmt carries it
    # and scripts/lint.nu runs the path bxl/lint.bxl reports.
    $"rustfmt = ($toolchain_root)/bin/rustfmt"
    ""
    "[nix_system_libs]"
    $"root = ($system_libs_root)"
    ""
    ...(if $windows_cross_root != null {
        ["[nix_windows_cross]" $"root = ($windows_cross_root)" ""]
    } else {
        []
    })
    # Nothing reads debug info out of a CI artifact, and carrying all of it is
    # what makes linking the GUI binary expensive. Line tables are kept so a
    # panic in a Buck test still names a file and line.
    ...(if ($env.CI? | is-not-empty) {
        ["[native_build]" "debuginfo = line-tables-only" ""]
    } else {
        []
    })
] | str join "\n" | save -f .buckconfig.local
