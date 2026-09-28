//! Where the app's HTTP clients should send their traffic on a managed network.
//!
//! Moved to `wows_toolkit_viewmodel::proxy` so the GPUI port resolves a proxy the
//! same way. This is the path the rest of this crate already uses.

pub use wows_toolkit_viewmodel::proxy::ProxyConfig;
pub use wows_toolkit_viewmodel::proxy::ProxySource;
pub use wows_toolkit_viewmodel::proxy::resolve_proxy;
