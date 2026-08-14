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
    ^buck2 expand-external-cell prelude
} catch { |err|
    $saved | save -f $local_config
    error make {msg: $"buck2 expand-external-cell failed: ($err.msg)"}
}
$saved | save -f $local_config

# Buck actions run with no PATH, so `/usr/bin/env bash` cannot resolve bash.
for file in ["prelude/utils/cmd_script.bzl", "prelude/rust/context.bzl"] {
    patch $file '"#!/usr/bin/env bash",' '"#!{}/bin/bash".format(read_root_config("nix_toolchain", "root")),'
}

# `--env-set=KEY=VALUE` used to lose its split when VALUE contains `=`, which
# several crates' `cargo:rustc-env` output does. Fixed upstream since 2026-07-31.
patch "prelude/rust/tools/rustc_action.py" 'flag, key, value = arg.split("=", 3)' 'flag, key, value = arg.split("=", 2)'

# MSVC link.exe does not expand a response file referenced from inside another
# response file, and rustc spills long linker command lines into one of its own.
patch "prelude/rust/build.bzl" ("        else:
            rustc_cmd.add(cmd_args(linker_argsfile, format = \"-Clink-arg=@{}\"))
            linker = compile_ctx.linker_with_pre_args") ("        elif compile_ctx.exec_is_windows:
            # MSVC link.exe does not expand a response file referenced from
            # inside another response file, and rustc spills long linker command
            # lines into one of its own. Passing the arguments inline leaves
            # rustc's spill as the only response file, so nothing nests.
            rustc_cmd.add(cmd_args(link_args_output.link_args, separate_debug_info_args, format = \"-Clink-arg={}\"))
            rustc_cmd.add(cmd_args(hidden = [link_args_output.hidden, separate_debug_info_args]))
            linker = compile_ctx.linker_with_pre_args
        else:
            rustc_cmd.add(cmd_args(linker_argsfile, format = \"-Clink-arg=@{}\"))
            linker = compile_ctx.linker_with_pre_args")

# Archive extraction shells out to bare mkdir/tar/unzip, which resolve through
# PATH. Route the POSIX branch through the pinned toolchain instead.
let unarchive = "prelude/http_archive/unarchive.bzl"
patch $unarchive "def _unarchive_cmd(" ('def _nix_tool(name):
    root = read_root_config("nix_toolchain", "root")
    if root == None:
        fail("Missing [nix_toolchain] root. Run `nu scripts/refresh-buck-toolchain.nu` before invoking Buck2.")
    return root + "/bin/" + name

def _unarchive_cmd(')
patch $unarchive '            "tar",' '            "tar" if exec_is_windows else _nix_tool("tar"),'
patch $unarchive "            _TAR_FLAGS[ext_type],\n" "            _TAR_FLAGS[ext_type] if exec_is_windows else _nix_tar_flags(ext_type),\n"
patch $unarchive "def _unarchive_cmd(" ('# GNU tar spawns its decompressor through PATH, which the actions clear.
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

def _unarchive_cmd(')
patch $unarchive 'return cmd_args(archive, format = "unzip {}"), bool(strip_prefix)' ('unzip = "unzip" if exec_is_windows else _nix_tool("unzip")
        return cmd_args(archive, format = unzip + " {}"), bool(strip_prefix)')
patch $unarchive 'mkdir = "mkdir -p {}"' 'mkdir = _nix_tool("mkdir") + " -p {}"'
patch $unarchive 'interpreter = ["/bin/sh"]' 'interpreter = [_nix_tool("bash")]'

let version = (^buck2 --version | str trim)
$"Expanded from the bundled prelude of: ($version)

Regenerate with `nu scripts/vendor-prelude.nu`, which reapplies the patches in
that script. Do not hand-edit files under prelude/.
" | save -f prelude/VENDORED_FROM

print $"Vendored the prelude from ($version)."
