#!/usr/bin/env nu

# Guard the macOS hermetic build boundary.
#
# The build's SDK is the nix-pinned apple-sdk baked into the wrapped clang in
# the Buck toolchain (flake.nix buck-toolchain), pinned by flake.lock. clang
# uses that baked SDK when no Apple env override is present, so the build needs
# no specific host Xcode. An ambient DEVELOPER_DIR/SDKROOT would override it, so
# this check requires those, when set, to be exactly the pinned SDK. The
# expected path is the flake's `macos-sdk` output, passed by
# scripts/refresh-buck-toolchain.nu.
#
# Usage: check-macos-sdk.nu EXPECTED_SDK_ROOT

def check_var [name: string, expected: string] {
    let val = ($env | get --optional $name)
    if $val != null and ($val | str length) > 0 and $val != $expected {
        print -e $"($name) is not the pinned macOS SDK:"
        print -e $"  set:      ($val)"
        print -e $"  expected: ($expected)"
        print -e "This would build against a different SDK than flake.lock pins."
        print -e "Unset it, or enter the nix devShell, then re-run."
        exit 1
    }
}

def main [expected_sdk_root: string] {
    if $nu.os-info.name != "macos" {
        exit 0
    }

    let arch = (^uname -m | str trim)
    if $arch != "arm64" {
        print -e $"Hermetic macOS build targets arm64; observed ($arch)."
        exit 1
    }

    check_var "DEVELOPER_DIR" $expected_sdk_root
    check_var "SDKROOT" $"($expected_sdk_root)/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"
}
