#!/usr/bin/env nu

def --wrapped main [...args: string] {
    nu build-support/check-xcode.nu
    nu scripts/refresh-buck-toolchain.nu
    ^buck2 ...$args
}
