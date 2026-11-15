load("//build-support/buck:workspace_rules.bzl", "native_binary_alias")

native_binary_alias("wows_toolkit_egui", "//crates/wows-toolkit:wows_toolkit_bin")
native_binary_alias("wows_toolkit_gpui", "//crates/wows-toolkit-gpui:gpui_bin")
native_binary_alias("wows_toolkit_zgui", "//crates/wows-toolkit-zgui:zgui_bin")
native_binary_alias("wowsunpack", "//crates/wowsunpack:wowsunpack_bin")
native_binary_alias("wows_data_mgr", "//crates/wows-data-mgr:wows_data_mgr_bin")
native_binary_alias("replayshark", "//crates/replayshark:replayshark_bin")
native_binary_alias("minimap_renderer", "//crates/minimap-renderer:minimap_renderer_bin")
native_binary_alias("minimap_renderer_cpu", "//crates/minimap-renderer:minimap_renderer_cpu_bin")
native_binary_alias("wgcheck", "//crates/wgcheck:wgcheck_bin")
native_binary_alias("dhat_load", "//crates/wows-toolkit:dhat_load_bin")
native_binary_alias("profile_replay", "//crates/wows-toolkit:profile_replay_bin")
native_binary_alias("dhat_parse", "//crates/wowsunpack:dhat_parse_bin")
native_binary_alias("wt_web_site", "//crates/wt-web:site")
native_binary_alias("replay_demo_site", "//crates/wt-replay-demo:site")

# Embedded by //crates/wows-toolkit-notices and installed by the MSI. Every
# other target in this package is a release alias; the checks that enumerate
# aliases select `kind(alias, ...)`.
export_file(
    name = "license",
    src = "LICENSE",
    visibility = [
        "//crates/wows-toolkit-notices/...",
        "toolchains//windows:wows_toolkit_msi",
    ],
)

export_file(
    name = "third_party_notices",
    src = "THIRD_PARTY_NOTICES.txt",
    visibility = [
        "//crates/wows-toolkit-notices/...",
        "toolchains//windows:wows_toolkit_msi",
    ],
)
