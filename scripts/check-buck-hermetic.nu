#!/usr/bin/env nu

# Any of these appearing in an action command or environment means the action can
# reach outside the declared inputs: a Cargo invocation, a download rule, a cache
# directory, a network client, or a tool resolved through PATH by bare name.
const PROHIBITED = '(^|[[:space:]"])([^[:space:]"]+/)?cargo([[:space:]"]|$)|http_(archive|file)\(|git_(fetch|repository)\(|(^|[[:space:]"])(PATH|CARGO_HOME|RUSTUP_HOME|SCCACHE_DIR|CCACHE_DIR|XDG_CACHE_HOME)=|/(\.cargo|\.cache)(/|[[:space:]",]|$)|(^|[[:space:]"])(curl|wget|ftp|git|ssh|nc)([[:space:]"]|$)|(^|[[:space:]"])(python|python3|bash|sh|clang|clang\+\+|gcc|g\+\+|cc|c\+\+|ld|ar|tar|unzip|mkdir)([[:space:]"]|$)'

# A tool named by absolute path is pinned only if it comes from the toolchain
# root. System paths are as ambient as a bare name: /bin/sh is a different
# interpreter on every host.
const SYSTEM_TOOL_PATHS = '(^|[[:space:]"])/(bin|sbin|usr|opt|Library|System)/'

def main [target: string, native_build_mode = "debug"] {
    if $native_build_mode not-in ["debug", "release"] {
        error make {msg: $"Unsupported native build mode: ($native_build_mode)"}
    }

    let query = $"all_actions\(deps\(($target)))"
    let actions = if $native_build_mode == "release" {
        ^buck2 aquery -c native_build.mode=release $query --output-all-attributes
    } else {
        ^buck2 aquery $query --output-all-attributes
    }

    let prohibited = ($actions | lines | where {|line| ($line =~ $PROHIBITED) or ($line =~ $SYSTEM_TOOL_PATHS)})
    if ($prohibited | is-not-empty) {
        print -e ($prohibited | first 20 | str join "\n")
        error make {msg: $"Buck action graph for ($target) contains a prohibited action or environment."}
    }
}
