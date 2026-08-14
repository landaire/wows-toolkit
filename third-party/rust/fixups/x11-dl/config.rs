// Stands in for the build script's pkg-config probe, which reads host state.
//
// Each entry is the directory to load that X library from. `None` leaves the
// soname to the dynamic loader's normal search path, which is what a relocatable
// build wants; baking in a builder's libdir would not resolve on another machine.
pub mod config {
    pub mod libdir {
        pub const xext: Option<&'static str> = None;
        pub const gl: Option<&'static str> = None;
        pub const xcursor: Option<&'static str> = None;
        pub const xxf86vm: Option<&'static str> = None;
        pub const xft: Option<&'static str> = None;
        pub const xinerama: Option<&'static str> = None;
        pub const xi: Option<&'static str> = None;
        pub const x11: Option<&'static str> = None;
        pub const xlib_xcb: Option<&'static str> = None;
        pub const xmu: Option<&'static str> = None;
        pub const xrandr: Option<&'static str> = None;
        pub const xtst: Option<&'static str> = None;
        pub const xrender: Option<&'static str> = None;
        pub const xpresent: Option<&'static str> = None;
        pub const xscrnsaver: Option<&'static str> = None;
        pub const xt: Option<&'static str> = None;
    }
}
