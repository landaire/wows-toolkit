/// Its presence tells `wgc_api.dll` that WGC started the game. The value is
/// not read.
pub const LAUNCH_MARKER_ENV: &str = "WGC_MARKER_JRQK3tEKaI2s2xT";

/// `host:port` of the launcher's loopback listener.
pub const RELAY_ADDR_ENV: &str = "WGC_RELAY_ADDR";

/// The per-launch token in lowercase hex. Passed in the environment rather
/// than the command line, which other local users can read.
pub const RELAY_TOKEN_ENV: &str = "WGC_RELAY_TOKEN";
