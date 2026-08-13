#!/usr/bin/env nu

if $nu.os-info.name != "macos" {
    exit 0
}

let expected_xcode = "Xcode 26.6\nBuild version 17F113"
let expected_developer = "/Applications/Xcode.app/Contents/Developer"
let expected_sdk = "/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX26.5.sdk"
let expected_arch = "arm64"

let observed_xcode = (^xcodebuild -version | str trim)
let observed_developer = (^xcode-select -p | str trim)
let observed_sdk = (^xcrun --sdk macosx --show-sdk-path | str trim)
let observed_arch = (^uname -m | str trim)

if $observed_xcode != $expected_xcode or $observed_developer != $expected_developer or $observed_sdk != $expected_sdk or $observed_arch != $expected_arch {
    print -e "Hermetic Buck2 requires the pinned macOS execution boundary."
    print -e $"Expected Xcode: ($expected_xcode)"
    print -e $"Observed Xcode: ($observed_xcode)"
    print -e $"Expected developer: ($expected_developer)"
    print -e $"Observed developer: ($observed_developer)"
    print -e $"Expected SDK: ($expected_sdk)"
    print -e $"Observed SDK: ($observed_sdk)"
    print -e $"Expected architecture: ($expected_arch)"
    print -e $"Observed architecture: ($observed_arch)"
    exit 1
}
