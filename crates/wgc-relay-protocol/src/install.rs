/// Where the fake WGC install lives inside the prefix, as Windows sees it.
/// `wgc_api.dll` finds it through `data\wgc_path.dat` and the `DisplayIcon`
/// registry value, both of which name this directory or its `wgc.exe`.
pub const GAME_CENTER_DIR: &str = r"C:\ProgramData\Wargaming.net\GameCenter";
