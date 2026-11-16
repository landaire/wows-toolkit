#!/usr/bin/env nu

# Re-vendors the Buck2 prelude from the pinned Buck2 binary and reapplies the
# patches this repository depends on.
#
# The prelude must come from the same Buck2 that consumes it: a prelude newer
# than the binary uses rule parameters the binary rejects at load time. Taking
# it from `expand-external-cell` guarantees the pair matches flake.lock.
#
# It is vendored rather than used as a bundled external cell because the patches
# below cannot be applied to a bundled cell.

def patch_all [file: path, from: string, to: string, expected: int] {
    let text = (open --raw $file)
    let occurrences = ($text | split row $from | length) - 1
    if $occurrences == $expected {
        $text | str replace --all $from $to | save -f $file
        return
    }
    if $occurrences == 0 and ($text | str contains $to) {
        print $"Skipping ($file): already upstream."
        return
    }
    error make {msg: $"Expected ($expected) occurrences of the patch site in ($file), found ($occurrences)."}
}

def patch [file: path, from: string, to: string] {
    let text = (open --raw $file)
    let occurrences = ($text | split row $from | length) - 1
    if $occurrences == 1 {
        $text | str replace $from $to | save -f $file
        return
    }
    # A patch that upstream has since made unnecessary is not an error, but only
    # if the result it was written to produce is already there. Anything else
    # means the prelude moved and the patch needs rewriting by hand.
    if $occurrences == 0 and ($text | str contains $to) {
        print $"Skipping ($file): already upstream."
        return
    }
    error make {msg: $"Expected exactly one occurrence of the patch site in ($file), found ($occurrences)."}
}

rm -r -f prelude

# expand-external-cell only works on a cell declared external, and .buckconfig
# declares the vendored one. .buckconfig.local is machine-local and layers on
# top, so the switch leaves no tracked file modified.
let local_config = ".buckconfig.local"
let saved = (if ($local_config | path exists) { open --raw $local_config } else { "" })
$"($saved)\n[external_cells]\n  prelude = bundled\n" | save -f $local_config
try {
    ^bin/buck2 expand-external-cell prelude
} catch { |err|
    $saved | save -f $local_config
    error make {msg: $"buck2 expand-external-cell failed: ($err.msg)"}
}
$saved | save -f $local_config

# Buck actions run with no PATH, so `/usr/bin/env bash` cannot resolve bash.
for file in ["prelude/utils/cmd_script.bzl", "prelude/rust/context.bzl"] {
    patch $file '"#!/usr/bin/env bash",' '"#!{}/bin/bash".format(read_root_config("nix_toolchain", "root")),'
}

# MSVC link.exe does not expand a response file referenced from inside another
# response file, and rustc spills long linker command lines into one of its own.
patch "prelude/rust/build.bzl" ("        rustc_cmd.add(cmd_args(linker_argsfile, format = \"-Clink-arg=@{}\"))
        rustc_cmd.add(cmd_args(compile_ctx.linker_with_pre_args, format = \"-Clinker={}\"))") ("        if compile_ctx.exec_is_windows:
            # MSVC link.exe does not expand a response file referenced from
            # inside another response file, and rustc spills long linker command
            # lines into one of its own. Passing the arguments inline leaves
            # rustc's spill as the only response file, so nothing nests.
            rustc_cmd.add(cmd_args(link_args_output.link_args, separate_debug_info_args, format = \"-Clink-arg={}\"))
            rustc_cmd.add(cmd_args(hidden = [link_args_output.hidden, separate_debug_info_args]))
        else:
            rustc_cmd.add(cmd_args(linker_argsfile, format = \"-Clink-arg=@{}\"))
        rustc_cmd.add(cmd_args(compile_ctx.linker_with_pre_args, format = \"-Clinker={}\"))")

# The POSIX unpack script invokes sh, mkdir, tar and unzip by bare name, which
# resolves them through PATH. Direct it to the pinned toolchain. A .crate file
# is a tar.gz, so this branch extracts every vendored crate.
let unarchive = "prelude/http_archive/unarchive.bzl"
patch $unarchive "# Returns a two-element tuple:\n" ('def _nix_tool(name):
    root = read_root_config("nix_toolchain", "root")
    if root == None:
        fail("Missing [nix_toolchain] root. Run `nu scripts/refresh-buck-toolchain.nu` before invoking Buck2.")
    return root + "/bin/" + name

# GNU tar spawns its decompressor through PATH, which the actions clear.
_NIX_TAR_DECOMPRESSORS = {
    "tar.bz2": "bzip2",
    "tar.gz": "gzip",
    "tar.xz": "xz",
    "tar.zst": "unzstd",
}

def _nix_tar_flags(ext_type: str) -> list[str]:
    decompressor = _NIX_TAR_DECOMPRESSORS.get(ext_type)
    if decompressor == None:
        return []
    return ["--use-compress-program=" + _nix_tool(decompressor)]

# Returns a two-element tuple:
')
patch $unarchive '            "tar",' '            _nix_tool("tar"),'
patch $unarchive "            _TAR_FLAGS[ext_type],\n" "            _nix_tar_flags(ext_type),\n"
patch $unarchive 'format = "unzip {}"' 'format = _nix_tool("unzip") + " {}"'
patch $unarchive 'format = "mkdir -p {}"' 'format = _nix_tool("mkdir") + " -p {}"'
patch $unarchive 'listing_interpreter = ["/bin/sh"]' 'listing_interpreter = [_nix_tool("bash")]'
patch $unarchive 'unpack_interpreter = ["/bin/sh"]' 'unpack_interpreter = [_nix_tool("bash")]'

# Two configured targets of one rule produce byte-identical outputs for several
# of these files, and a content-based path is then the same path for both. On
# Windows the two actions racing to write it fail with a sharing violation
# ("Binary being executed, please close the process first"), which is what a
# build spanning both a transitioned and an untransitioned configuration does
# constantly. The classic per-target paths cost some cache sharing and keep the
# build from failing.
patch "prelude/decls/rust_rules.bzl" '"use_content_based_paths": attrs.bool(default = True),' '"use_content_based_paths": attrs.bool(default = False),'
patch "prelude/decls/cxx_common.bzl" '"use_content_based_paths": attrs.bool(default = True),' '"use_content_based_paths": attrs.bool(default = False),'
# The build-script rule declares its own outputs content-based whatever the
# rule attribute says.
patch_all "prelude/rust/cargo_buildscript.bzl" "has_content_based_path = True" "has_content_based_path = False" 6

# The release toolchains/buck2 pins; scripts/check-buck2-pins.py holds the other
# pins to it.
let release = (
    open --raw toolchains/buck2
    | parse -r 'releases/download/(?<tag>[^/]+)/'
    | get tag
    | uniq
)
if ($release | length) != 1 {
    error make {msg: $"toolchains/buck2 names Buck2 releases ($release); it must name exactly one."}
}
let version = (^bin/buck2 --version | str trim)
$"Expanded from the bundled prelude of: ($version)
Buck2 release: ($release | first)

Regenerate with `nu scripts/vendor-prelude.nu`, which reapplies the patches in
that script. Do not hand-edit files under prelude/.
" | save -f prelude/VENDORED_FROM

print $"Vendored the prelude from Buck2 release ($release | first)."
