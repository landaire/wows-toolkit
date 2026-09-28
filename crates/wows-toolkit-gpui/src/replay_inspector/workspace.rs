//! A replay directory opened as a tab of its own.
//!
//! The egui app calls these workspaces: one dock tab per directory, titled by
//! its root and closeable, so an archive of old replays sits beside the
//! install's own listing rather than replacing it (`tab_state.rs`'s
//! `ReplayWorkspace`). This is that tab: a listing bound to one directory, which
//! raises the same events the sidebar's listing does.

use std::path::Path;
use std::path::PathBuf;

use gpui_kit::component::dock::BasePanel;
use gpui_kit::component::dock::Panel;
use gpui_kit::component::dock::PanelEvent;
use gpui_kit::*;
use rust_i18n::t;

use super::browser_view::ReplayBrowser;
use super::browser_view::ReplayBrowserEvent;

/// One directory, listed in its own tab.
pub struct ReplayWorkspace {
    focus_handle: FocusHandle,
    /// The directory this tab is reading. Kept because the tab is titled by it
    /// and because searching it needs to say which directory.
    root: PathBuf,
    browser: Entity<ReplayBrowser>,
    _browser_subscription: Subscription,
}

/// What a workspace asks of whoever is hosting it.
///
/// The listing's own events pass straight through: a workspace owns neither the
/// dock nor the tabs a replay opens into.
pub enum WorkspaceEvent {
    /// The listing raised one of its own.
    Listing(ReplayBrowserEvent),
    /// Search only the replays in this directory.
    SearchThese(PathBuf),
}

impl EventEmitter<WorkspaceEvent> for ReplayWorkspace {}
impl EventEmitter<PanelEvent> for ReplayWorkspace {}

impl ReplayWorkspace {
    /// Opens `root` as a tab, and starts reading it.
    ///
    /// `wows_dir` is the install, which the listing needs whatever directory it
    /// is reading: "Open in Game" launches the executable beside it.
    pub fn new(root: PathBuf, wows_dir: String, cx: &mut Context<Self>) -> Self {
        let browser = cx.new(ReplayBrowser::new);
        browser.update(cx, |browser, cx| {
            browser.set_wows_dir(wows_dir);
            browser.scan_directory(root.clone(), cx);
        });
        let subscription = cx.subscribe(&browser, |_this, _browser, event, cx| {
            cx.emit(WorkspaceEvent::Listing(event.clone()));
        });
        Self { focus_handle: cx.focus_handle(), root, browser, _browser_subscription: subscription }
    }

    /// The directory this tab is reading.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The listing inside it, for a caller that has to tell it something the
    /// sidebar's listing is told too.
    pub fn browser(&self) -> &Entity<ReplayBrowser> {
        &self.browser
    }

    /// What the tab is called: the directory's own last part, which is what
    /// tells two archives apart without spelling out a whole path.
    fn tab_title(&self) -> SharedString {
        self.root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.display().to_string())
            .into()
    }
}

impl Focusable for ReplayWorkspace {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for ReplayWorkspace {
    fn panel_name(&self) -> &'static str {
        "ReplayWorkspace"
    }
}

impl Panel for ReplayWorkspace {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.tab_title()
    }
}

impl Render for ReplayWorkspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = self.root.clone();
        gpui_kit::component::v_flex()
            .size_full()
            .child(
                gpui_kit::component::h_flex()
                    .gap_2()
                    .items_center()
                    .p_1()
                    .child(
                        div().flex_1().text_xs().text_color(crate::theme::text_dim()).child(root.display().to_string()),
                    )
                    .child(
                        gpui_kit::component::button::Button::new("workspace-search-these")
                            .label(t!("ui.tabs.search_these_replays").into_owned())
                            .compact()
                            .on_click(cx.listener(move |this, _event, _window, cx| {
                                cx.emit(WorkspaceEvent::SearchThese(this.root.clone()));
                            })),
                    ),
            )
            .child(div().flex_1().min_h(px(0.)).child(self.browser.clone()))
    }
}
