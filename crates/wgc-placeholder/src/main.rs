//! Installed as `wgc.exe` in the launcher's Wine prefix. `wgc_api.dll` reads
//! only its version resource (wgc.rc) and treats WGC as outdated without one.
//! Nothing runs it on purpose; if something does, it exits at once.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {}
