//! Looking for a newer release of the toolkit.

use nsh_core::prelude::JsObject;
use wows_toolkit_viewmodel::update::RELEASES_URL;
use wows_toolkit_viewmodel::update::Release;

use crate::shell::error::UpdateError;

/// What this app calls itself to the release server.
pub const USER_AGENT: &str = concat!("wows-toolkit-nsh/", env!("CARGO_PKG_VERSION"));

/// A release newer than the running build, as the offer shows it.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ReleaseOffer {
    /// The release tag, which carries the version with a leading `v`.
    pub tag: String,
    /// The release page.
    pub html_url: String,
    /// The release notes, in Markdown. `None` when the release has none.
    pub notes: Option<String>,
    /// Whether the Windows application archive is published yet.
    pub windows_asset_published: bool,
}

impl ReleaseOffer {
    fn from_release(release: Release) -> Self {
        let windows_asset_published = release.asset_for_windows().is_some();
        let notes = release.body.filter(|notes| !notes.trim().is_empty());

        Self { tag: release.tag_name, html_url: release.html_url, notes, windows_asset_published }
    }
}

/// Asks `url` for the newest release past `running`, through `proxy_url`.
pub async fn check(url: &str, running: &str, proxy_url: &str) -> Result<Option<ReleaseOffer>, UpdateError> {
    let client = wows_toolkit_viewmodel::http::client(USER_AGENT, proxy_url, reqwest::redirect::Policy::default())?;
    let found = wows_toolkit_viewmodel::update::check_release(&client, url, running).await?;

    Ok(found.map(ReleaseOffer::from_release))
}

/// Asks the toolkit's release feed whether this build is the newest.
pub async fn check_published() -> Result<Option<ReleaseOffer>, UpdateError> {
    let pool = crate::shell::settings::open().await?;
    let settings = crate::shell::settings::load_from(&pool).await?;

    check(RELEASES_URL, env!("CARGO_PKG_VERSION"), &settings.proxy_url).await
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::io::Write;

    use super::*;

    const RELEASES: &str = r###"[{
        "tag_name": "v99.0.0",
        "draft": false,
        "prerelease": false,
        "html_url": "https://github.com/landaire/wows-toolkit/releases/tag/v99.0.0",
        "body": "## Changes\nUpdate notes",
        "assets": [
            {"name": "wows_toolkit_v99.0.0_windows.zip", "browser_download_url": "https://example.invalid/app.zip"}
        ]
    }]"###;

    fn serve_once(status: &str, body: &'static str) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a test HTTP listener");
        let url = format!("http://{}", listener.local_addr().expect("the listener address"));
        let header = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("an HTTP connection");
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).expect("the HTTP request");
                request.push(byte[0]);
            }
            stream.write_all(header.as_bytes()).expect("the response headers");
            stream.write_all(body.as_bytes()).expect("the response body");
        });

        (url, server)
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a test runtime")
    }

    #[test]
    fn a_newer_release_is_offered_with_its_notes() {
        let (url, server) = serve_once("200 OK", RELEASES);
        let offer = runtime().block_on(check(&url, "1.0.0", "")).expect("the check completes").expect("an offer");
        server.join().expect("the test server");

        assert_eq!(offer.tag, "v99.0.0");
        assert_eq!(offer.notes.as_deref(), Some("## Changes\nUpdate notes"));
        assert!(offer.windows_asset_published);
    }

    #[test]
    fn the_newest_build_is_offered_nothing() {
        let (url, server) = serve_once("200 OK", RELEASES);
        let offer = runtime().block_on(check(&url, "99.0.0", "")).expect("the check completes");
        server.join().expect("the test server");

        assert!(offer.is_none());
    }

    #[test]
    fn a_refused_check_is_an_error() {
        let (url, server) = serve_once("403 Forbidden", RELEASES);
        assert!(runtime().block_on(check(&url, "1.0.0", "")).is_err());
        server.join().expect("the test server");
    }
}
