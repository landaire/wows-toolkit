//! The listing's header: its caption and the controls over the selected directory.

use rust_i18n::t;
use zgui::component;
use zgui::css;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

use crate::tabs::replays::browser::folders::BrowserControl;

const SHEET: &str = "wt-replay-listing-header";

const CSS: &str = css!(
    ".wt-listing-header {
        display: flex;
        flex: none;
        align-items: center;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-listing-header__caption {
        flex: 1;
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        font-size: 14px;
        font-weight: 700;
    }"
);

#[component]
pub fn ListingHeader(control: BrowserControl) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let _ = control;

    view! {
        row(class = "wt-listing-header", attr:data-testid = "replay-listing-header") {
            text(class = "wt-listing-header__caption") {{t!("ui.replay.listing_caption").into_owned()}}
        }
    }
}
