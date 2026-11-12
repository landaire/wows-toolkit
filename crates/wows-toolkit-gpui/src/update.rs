//! Scheduling of the release check and the update install on GPUI's background
//! executor. The work itself is `wows_toolkit_services::update`.

use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Task;
use wows_toolkit_services::update::Downloaded;
use wows_toolkit_services::update::UpdateError;
use wows_toolkit_viewmodel::update::Release;

/// The newest published release, when it is newer than this build.
pub fn check(proxy: String, cx: &App) -> Task<Result<Option<Release>, rootcause::Report>> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let runtime = runtime.ok_or(UpdateError::NoRuntime)?;
        runtime.block_on(wows_toolkit_services::update::check(
            crate::http::USER_AGENT,
            &proxy,
            env!("CARGO_PKG_VERSION"),
        ))
    })
}

/// Downloads and installs `asset_url`, reporting the download on `progress`.
pub fn install(
    asset_url: String,
    proxy: String,
    progress: futures::channel::mpsc::UnboundedSender<Downloaded>,
    cx: &App,
) -> Task<Result<(), rootcause::Report>> {
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let runtime = runtime.ok_or(UpdateError::NoRuntime)?;
        wows_toolkit_services::update::install(&asset_url, crate::http::USER_AGENT, &proxy, &progress, &runtime)
    })
}
