#!/usr/bin/env nu

# The store path `installable` builds, kept alive by a GC root at `link`.
# Inside the dev shell `variable` already names it, the path the shell was
# evaluated with, so nothing is evaluated here; outside it, it is built.
# .buckconfig.local names the store path directly, and without the root
# `nix-collect-garbage` deletes it out from under Buck.
def store_path [variable: string, installable: string, link: string] {
    let given = ($env | get --optional $variable | default "")
    if ($given | is-not-empty) {
        ^nix build --out-link $link $given
        $given
    } else {
        ^nix build --out-link $link --print-out-paths $installable | str trim
    }
}

if $nu.os-info.name == "macos" {
    let macos_sdk = (store_path WT_MACOS_SDK .#macos-sdk .tools/macos-sdk)
    nu build-support/check-macos-sdk.nu $macos_sdk
}

let toolchain_root = (store_path WT_BUCK_TOOLCHAIN .#buck-toolchain .tools/buck-toolchain)

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
    # The Windows helpers: windows-sys import libraries and the version
    # resource.
    "llvm-dlltool"
    "llvm-windres"
]

for tool in $required {
    let tool_path = $"($toolchain_root)/bin/($tool)"
    if not ($tool_path | path exists) {
        error make {msg: $"Nix Buck toolchain is missing ($tool_path)."}
    }
}

# The Windows helpers compile against the windows-gnu standard library that
# toolchains/BUCK pins, which this rustc rejects from any other release.
if $nu.os-info.name == "linux" {
    let rustc_version = (^$"($toolchain_root)/bin/rustc" --version | parse "rustc {version} {rest}" | get version.0)
    if not (open --raw toolchains/BUCK | str contains $"/rust-std-($rustc_version)-x86_64-pc-windows-gnu.tar.xz") {
        error make {msg: $"toolchains/BUCK pins a windows-gnu rust-std from another release than rustc ($rustc_version). Move the rust_std_windows_gnu and rust_mingw_windows_gnu archives to ($rustc_version)."}
    }
}

let system_libs_root = (store_path WT_BUCK_SYSTEM_LIBS .#buck-system-libs .tools/buck-system-libs)

# Buck reads these instead of resolving any tool through PATH. Written in
# .tools, which Buck ignores, and renamed over the file: bin/buck2 may run this
# from two shells at once, and Buck must never read half of one.
let config = $".tools/buckconfig.local.(random uuid)"
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
    # Nothing reads debug info out of a CI artifact, and it is most of the
    # size of every debug rlib and test binary CI moves through the action
    # cache. A panic still names its file and line without it.
    ...(if ($env.CI? | is-not-empty) {
        ["[native_build]" "debuginfo = none" ""]
    } else {
        []
    })
] | str join "\n" | save -f $config
mv -f $config .buckconfig.local
