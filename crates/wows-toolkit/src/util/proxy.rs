//! Where the app's HTTP clients should send their traffic on a managed network.
//!
//! Defined in `wows_toolkit_viewmodel::proxy` so both front ends resolve a proxy
//! the same way. This is the path the rest of this crate uses.

pub use wows_toolkit_viewmodel::proxy::ProxyConfig;
pub use wows_toolkit_viewmodel::proxy::ProxySource;
pub use wows_toolkit_viewmodel::proxy::resolve_proxy;
