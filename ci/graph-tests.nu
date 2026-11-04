#!/usr/bin/env nu

# Runs the tests of the build graph itself, which query and build with buck2 and
# so cannot be `buck2 test` targets.
def main [] {
    for test in (glob scripts/test-*.nu | sort) {
        print $"==> ($test)"
        ^nu --no-config-file $test
    }
}
