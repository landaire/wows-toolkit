//! The Settings drawer: every setting in one panel that slides over the right
//! of the window, filtered by a search over each setting's name and
//! description, and grouped into sections the strip under the search jumps to.

use std::borrow::Cow;
use std::cell::Cell;

use gpui_kit::component::Size;
use gpui_kit::component::StyleSized as _;
use gpui_kit::component::label::Label;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;

use super::*;
use crate::text_filter::FilterQuery;
use crate::text_filter::MarkedLabel;
use crate::text_filter::MarkedQuery;
use crate::text_filter::TypedQuery;

/// How wide the drawer is.
const DRAWER_WIDTH: Pixels = px(440.);

/// One theme tile, three to a row inside the drawer.
const THEME_TILE_WIDTH: Pixels = px(116.);

/// The language combo, and the menu under it.
const LANGUAGE_COMBO_WIDTH: Pixels = px(170.);

/// The corner a settings card is drawn with.
const CARD_RADIUS: Pixels = px(6.);

/// The narrowest a directory field gets before its button drops under it.
const PATH_FIELD_MIN_WIDTH: Rems = Rems(10.);

/// The formats a battle can be auto-exported in, in the order the egui
/// combo lists them.
const EXPORT_FORMATS: [ReplayExportFormat; 3] =
    [ReplayExportFormat::Json, ReplayExportFormat::Csv, ReplayExportFormat::Cbor];

/// A group of settings, in the order the drawer lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    General,
    Appearance,
    Game,
    GameDataCache,
    ReplayIndex,
    Replays,
    Collaboration,
    Twitch,
    ArmorViewer,
}

impl SettingsSection {
    pub(crate) const ALL: [Self; 9] = [
        Self::General,
        Self::Appearance,
        Self::Game,
        Self::GameDataCache,
        Self::ReplayIndex,
        Self::Replays,
        Self::Collaboration,
        Self::Twitch,
        Self::ArmorViewer,
    ];

    fn name(self) -> String {
        match self {
            Self::General => t!("ui.settings.panel.general"),
            Self::Appearance => t!("ui.settings.panel.appearance"),
            Self::Game => t!("ui.settings.panel.game"),
            Self::GameDataCache => t!("ui.settings.wows.cache.heading"),
            Self::ReplayIndex => t!("ui.settings.index.heading"),
            Self::Replays => t!("ui.settings.panel.replays"),
            Self::Collaboration => t!("ui.settings.panel.collaboration"),
            Self::Twitch => t!("ui.settings.panel.twitch"),
            Self::ArmorViewer => t!(AppTab::ArmorViewer.label_key()),
        }
        .into_owned()
    }

    fn glyph(self) -> crate::icons::Glyph {
        use crate::icons::Glyph;

        match self {
            Self::General => Glyph::Settings,
            Self::Appearance => Glyph::Paintbrush,
            Self::Game => Glyph::FolderOpen,
            Self::GameDataCache => Glyph::Archive,
            Self::ReplayIndex => Glyph::Database,
            Self::Replays => Glyph::Table,
            Self::Collaboration => Glyph::Users,
            Self::Twitch => Glyph::Twitch,
            Self::ArmorViewer => Glyph::Shield,
        }
    }
}

/// One setting, as a row of the drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Setting {
    Language,
    DataSharing,
    CodeIntegrity,
    Proxy,
    CheckForUpdates,
    EnableLogging,
    DataFolder,
    Theme,
    Zoom,
    WowsDirectory,
    AutoDump,
    CacheDirectory,
    CachedBuilds,
    ReplayIndex,
    CurrentReplayPath,
    ShowRawXp,
    ShowObservedDamage,
    ShowEntityId,
    ShowHeals,
    ReplayPreviews,
    AutoExport,
    DisplayName,
    SuppressIpWarning,
    DisableAutoOpen,
    TwitchGetToken,
    TwitchToken,
    TwitchChannel,
    ArmorPlateEdges,
    ArmorOpaqueHull,
    /// Stands in for the two above while nothing has been saved.
    ArmorNoDefaults,
}

impl Setting {
    /// Every setting, in the order the drawer lists them.
    const ALL: [Self; 30] = [
        Self::Language,
        Self::DataSharing,
        Self::CodeIntegrity,
        Self::Proxy,
        Self::CheckForUpdates,
        Self::EnableLogging,
        Self::DataFolder,
        Self::Theme,
        Self::Zoom,
        Self::WowsDirectory,
        Self::AutoDump,
        Self::CacheDirectory,
        Self::CachedBuilds,
        Self::ReplayIndex,
        Self::CurrentReplayPath,
        Self::ShowRawXp,
        Self::ShowObservedDamage,
        Self::ShowEntityId,
        Self::ShowHeals,
        Self::ReplayPreviews,
        Self::AutoExport,
        Self::DisplayName,
        Self::SuppressIpWarning,
        Self::DisableAutoOpen,
        Self::TwitchGetToken,
        Self::TwitchToken,
        Self::TwitchChannel,
        Self::ArmorPlateEdges,
        Self::ArmorOpaqueHull,
        Self::ArmorNoDefaults,
    ];

    fn section(self) -> SettingsSection {
        match self {
            Self::Language
            | Self::DataSharing
            | Self::CodeIntegrity
            | Self::Proxy
            | Self::CheckForUpdates
            | Self::EnableLogging
            | Self::DataFolder => SettingsSection::General,
            Self::Theme | Self::Zoom => SettingsSection::Appearance,
            Self::WowsDirectory => SettingsSection::Game,
            Self::AutoDump | Self::CacheDirectory | Self::CachedBuilds => SettingsSection::GameDataCache,
            Self::ReplayIndex => SettingsSection::ReplayIndex,
            Self::CurrentReplayPath
            | Self::ShowRawXp
            | Self::ShowObservedDamage
            | Self::ShowEntityId
            | Self::ShowHeals
            | Self::ReplayPreviews
            | Self::AutoExport => SettingsSection::Replays,
            Self::DisplayName | Self::SuppressIpWarning | Self::DisableAutoOpen => SettingsSection::Collaboration,
            Self::TwitchGetToken | Self::TwitchToken | Self::TwitchChannel => SettingsSection::Twitch,
            Self::ArmorPlateEdges | Self::ArmorOpaqueHull | Self::ArmorNoDefaults => SettingsSection::ArmorViewer,
        }
    }

    fn title(self) -> Cow<'static, str> {
        match self {
            Self::Language => t!("ui.settings.app.language"),
            Self::DataSharing => t!("ui.settings.app.data_sharing_mode"),
            Self::CodeIntegrity => t!("ui.settings.app.code_integrity"),
            Self::Proxy => t!("ui.settings.app.proxy_url"),
            Self::CheckForUpdates => t!("ui.settings.app.check_for_updates"),
            Self::EnableLogging => t!("ui.settings.app.enable_logging"),
            Self::DataFolder => t!("ui.settings.panel.data_folder"),
            Self::Theme => t!("ui.settings.app.theme"),
            Self::Zoom => t!("ui.settings.panel.zoom"),
            Self::WowsDirectory => t!("ui.settings.wows.directory_hint"),
            Self::AutoDump => t!("ui.settings.wows.cache.auto_dump"),
            Self::CacheDirectory => t!("ui.settings.panel.cache_directory"),
            Self::CachedBuilds => t!("ui.settings.panel.cached_builds"),
            Self::ReplayIndex => t!("ui.settings.index.build"),
            Self::CurrentReplayPath => t!("ui.settings.replay.current_path"),
            Self::ShowRawXp => t!("ui.settings.replay.show_raw_xp"),
            Self::ShowObservedDamage => t!("ui.settings.replay.show_observed_damage"),
            Self::ShowEntityId => t!("ui.settings.replay.show_entity_id"),
            Self::ShowHeals => t!("ui.settings.replay.show_heals"),
            Self::ReplayPreviews => t!("ui.settings.replay.enable_previews"),
            Self::AutoExport => t!("ui.settings.replay.auto_export_data"),
            Self::DisplayName => t!("ui.settings.session.display_name"),
            Self::SuppressIpWarning => t!("ui.settings.session.suppress_ip_warning"),
            Self::DisableAutoOpen => t!("ui.settings.session.disable_auto_open"),
            Self::TwitchGetToken => t!("ui.settings.twitch.get_token"),
            Self::TwitchToken => t!("ui.settings.panel.token"),
            Self::TwitchChannel => t!("ui.settings.twitch.monitored_channel"),
            Self::ArmorPlateEdges => t!("ui.armor.plate_edges"),
            Self::ArmorOpaqueHull => t!("ui.armor.opaque_hull"),
            Self::ArmorNoDefaults => t!("ui.settings.panel.armor_defaults"),
        }
    }

    fn description(self) -> Cow<'static, str> {
        match self {
            Self::Language => t!("ui.settings.panel.language_description"),
            Self::DataSharing => t!("ui.settings.panel.data_sharing_description"),
            Self::CodeIntegrity => t!("ui.settings.panel.code_integrity_description"),
            Self::Proxy => t!("ui.settings.app.proxy_url_hint"),
            Self::CheckForUpdates => t!("ui.settings.panel.check_for_updates_description"),
            Self::EnableLogging => t!("ui.settings.app.enable_logging_tooltip"),
            Self::DataFolder => t!("ui.settings.app.open_data_dir_tooltip"),
            Self::Theme => t!("ui.settings.panel.theme_description"),
            Self::Zoom => t!("ui.settings.panel.zoom_description"),
            Self::WowsDirectory => t!("ui.settings.wows.description"),
            Self::AutoDump => t!("ui.settings.wows.cache.description"),
            Self::CacheDirectory => t!("ui.settings.panel.cache_directory_description"),
            Self::CachedBuilds => t!("ui.settings.wows.cache.actions_tooltip"),
            Self::ReplayIndex => t!("ui.settings.index.description"),
            Self::CurrentReplayPath => t!("ui.settings.panel.current_path_description"),
            Self::ShowRawXp => t!("ui.settings.panel.show_raw_xp_description"),
            Self::ShowObservedDamage => t!("ui.settings.panel.show_observed_damage_description"),
            Self::ShowEntityId => t!("ui.settings.panel.show_entity_id_description"),
            Self::ShowHeals => t!("ui.settings.panel.show_heals_description"),
            Self::ReplayPreviews => t!("ui.settings.replay.enable_previews_tooltip"),
            Self::AutoExport => t!("ui.settings.panel.auto_export_description"),
            Self::DisplayName => t!("ui.settings.panel.display_name_description"),
            Self::SuppressIpWarning => t!("ui.settings.session.ip_warning_tooltip"),
            Self::DisableAutoOpen => t!("ui.settings.session.auto_open_tooltip"),
            Self::TwitchGetToken => t!("ui.settings.panel.get_token_description"),
            Self::TwitchToken => t!("ui.settings.twitch.paste_token_tooltip"),
            Self::TwitchChannel => t!("ui.settings.panel.monitored_channel_description"),
            Self::ArmorPlateEdges | Self::ArmorOpaqueHull => t!("ui.settings.panel.armor_saved_description"),
            Self::ArmorNoDefaults => t!("ui.settings.armor.no_defaults"),
        }
    }

    /// Whether the control sits under the text rather than beside it: the
    /// fields and button rows are wider than the space beside a title.
    fn stacked(self) -> bool {
        matches!(
            self,
            Self::DataSharing
                | Self::CodeIntegrity
                | Self::Proxy
                | Self::Theme
                | Self::Zoom
                | Self::WowsDirectory
                | Self::CacheDirectory
                | Self::CachedBuilds
                | Self::ReplayIndex
                | Self::CurrentReplayPath
                | Self::AutoExport
                | Self::DisplayName
                | Self::TwitchToken
                | Self::TwitchChannel
        )
    }

    /// Whether the name or the description matches `query`. An inactive
    /// query matches everything.
    fn matches(self, query: &FilterQuery) -> bool {
        query.keeps(&self.title()) || query.keeps(&self.description())
    }
}

/// A place in the drawer to open at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsTarget {
    Section(SettingsSection),
    Setting(Setting),
}

/// One child of the drawer's scroll area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drawn {
    Heading(SettingsSection),
    Row(Setting),
}

impl Drawn {
    fn section(self) -> SettingsSection {
        match self {
            Self::Heading(section) => section,
            Self::Row(setting) => setting.section(),
        }
    }

    fn is(self, target: SettingsTarget) -> bool {
        match (self, target) {
            (Self::Heading(drawn), SettingsTarget::Section(wanted)) => drawn == wanted,
            (Self::Row(drawn), SettingsTarget::Setting(wanted)) => drawn == wanted,
            _ => false,
        }
    }
}

/// What the drawer keeps between frames.
pub(super) struct SettingsDrawer {
    open: bool,
    search: Entity<InputState>,
    /// The query read from `search`, kept while its text is unchanged.
    query: TypedQuery,
    scroll: ScrollHandle,
    /// Where to scroll once there is a layout to measure it against.
    scroll_to: Option<SettingsTarget>,
    /// What each child of the scroll area was in the last frame drawn. The
    /// strip marks the section at the top from it, and a scroll waits until
    /// the layout it measures against holds the same children.
    drawn: Vec<Drawn>,
    /// Where the zoom slider was before a zoom change, until the rows are
    /// laid out at the new zoom.
    zoom_slider_anchor: Option<ZoomSliderAnchor>,
    /// Where the zoom slider is laid out and drawn.
    zoom_slider: Rc<ZoomSliderSeen>,
    _search_edited: Subscription,
}

impl SettingsDrawer {
    pub(super) fn new(window: &mut Window, cx: &mut Context<App>) -> Self {
        let search = cx
            .new(|cx| InputState::new(window, cx).placeholder(t!("ui.settings.panel.search_placeholder").to_string()));
        // The rows are filtered as the query is typed, and only the app's own
        // render draws them.
        let search_edited = cx.subscribe(&search, |_this, _search, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            open: false,
            search,
            query: TypedQuery::default(),
            scroll: ScrollHandle::new(),
            scroll_to: None,
            drawn: Vec::new(),
            zoom_slider_anchor: None,
            zoom_slider: Rc::default(),
            _search_edited: search_edited,
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn search(&self) -> &Entity<InputState> {
        &self.search
    }
}

/// The zoom slider's top in the window before a zoom change.
#[derive(Clone, Copy, Debug)]
struct ZoomSliderAnchor {
    top: Pixels,
}

/// The zoom slider's layout node in the frame being drawn, and its bounds
/// in the frame last drawn.
#[derive(Default)]
struct ZoomSliderSeen {
    layout: Cell<Option<LayoutId>>,
    drawn: Cell<Option<Bounds<Pixels>>>,
}

/// Records where the element it wraps is laid out and drawn, for
/// [`AnchoredScroll`] to read before the element is placed.
struct LayoutProbe {
    seen: Rc<ZoomSliderSeen>,
    child: AnyElement,
}

impl IntoElement for LayoutProbe {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for LayoutProbe {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout = self.child.request_layout(window, cx);
        self.seen.layout.set(Some(layout));

        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> Self::PrepaintState {
        self.seen.drawn.set(Some(bounds));
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) {
        self.child.paint(window, cx);
    }
}

/// A scroll area that, given an anchor, scrolls so the zoom slider is drawn
/// at the anchor. It acts between layout and placement, so the first frame
/// at a new zoom already shows the slider in place.
struct AnchoredScroll {
    scroll: ScrollHandle,
    slider: Rc<ZoomSliderSeen>,
    anchor: Option<ZoomSliderAnchor>,
    child: AnyElement,
}

impl IntoElement for AnchoredScroll {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for AnchoredScroll {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        // What was probed in an earlier frame is not in this frame's layout.
        self.slider.layout.set(None);
        self.slider.drawn.set(None);

        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) -> Self::PrepaintState {
        if let Some(anchor) = self.anchor
            && let Some(slider) = self.slider.layout.take()
        {
            // The slider's unscrolled top. Not above the area's own top, which
            // a zoom in moves down as the header over it grows.
            let slider_top = window.layout_bounds(slider).top();
            let top = anchor.top.max(bounds.top());
            let offset = self.scroll.offset();
            // The scroll area clamps this to its range as it is placed.
            self.scroll.set_offset(point(offset.x, top - slider_top));
        }

        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut gpui_kit::App,
    ) {
        self.child.paint(window, cx);
    }
}

/// A line under a setting that reports on it.
struct Note {
    text: String,
    error: bool,
}

impl App {
    /// Opens the drawer with an empty search, scrolled to `target` when one
    /// is given.
    pub(crate) fn open_settings(
        &mut self,
        target: Option<SettingsTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_drawer.open = true;
        if target.is_some() {
            self.settings_drawer.scroll_to = target;
        }
        self.settings_drawer.search.update(cx, |search, cx| search.set_value("", window, cx));
        // Deferred: a palette entry opens the drawer from inside the palette,
        // which hands focus back to where it was as it closes.
        cx.defer_in(window, |this, window, cx| {
            if this.settings_drawer.open {
                this.settings_drawer.search.update(cx, |search, cx| search.focus(window, cx));
            }
        });
        cx.notify();
    }

    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings_drawer.open {
            return;
        }

        self.settings_drawer.open = false;
        self.settings_drawer.scroll_to = None;
        self.settings_drawer.zoom_slider_anchor = None;
        // Focus leaves the search with the drawer, back to where the window's
        // shortcuts are heard.
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Notes where the zoom slider is drawn, for the rows laid out at a new
    /// zoom to be scrolled so it stays there. A slider scrolled out of view
    /// is left out of view.
    pub(super) fn anchor_zoom_slider(&mut self) {
        if !self.settings_drawer.open {
            return;
        }

        let Some(slider) = self.settings_drawer.zoom_slider.drawn.get() else { return };
        if !slider.intersects(&self.settings_drawer.scroll.bounds()) {
            return;
        }

        self.settings_drawer.zoom_slider_anchor = Some(ZoomSliderAnchor { top: slider.top() });
    }

    /// Wraps the zoom slider so the drawer can keep it in place.
    pub(super) fn probe_zoom_slider(&self, slider: impl IntoElement) -> AnyElement {
        LayoutProbe { seen: Rc::clone(&self.settings_drawer.zoom_slider), child: slider.into_any_element() }
            .into_any_element()
    }

    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_drawer.open {
            self.close_settings(window, cx);
        } else {
            self.open_settings(None, window, cx);
        }
    }

    /// A button for a message about a setting, which opens the drawer at it.
    pub(super) fn settings_action(&self, setting: Setting, cx: &mut Context<Self>) -> crate::toast::Action {
        let app = cx.entity().downgrade();
        crate::toast::Action::new(t!("ui.settings.panel.open").into_owned(), move |window, cx| {
            let Some(app) = app.upgrade() else { return };
            app.update(cx, |app, cx| app.open_settings(Some(SettingsTarget::Setting(setting)), window, cx));
        })
    }

    /// The gear at the end of the tab strip. It carries the mark the drawer
    /// has for a game directory that cannot be read.
    pub(super) fn render_settings_button(&self, cx: &Context<Self>) -> AnyElement {
        let glyph = Icon::from(crate::icons::Glyph::Settings);
        let glyph = if self.wows_dir_invalid { glyph.text_color(cx.theme().danger) } else { glyph };
        Button::new("settings-open")
            .ghost()
            .small()
            .icon(glyph)
            .selected(self.settings_drawer.open)
            .tooltip(t!("ui.tabs.settings").to_string())
            .on_click(cx.listener(|this, _event: &ClickEvent, window, cx| this.toggle_settings(window, cx)))
            .into_any_element()
    }

    /// Handles the drawer's keys: Ctrl+, (Cmd+, on macOS) opens and shuts
    /// it, and Escape shuts it. Reports whether the key was one of them.
    pub(super) fn settings_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if key == "," && modifiers.secondary(cx) && !modifiers.shift && !modifiers.alt {
            self.toggle_settings(window, cx);
            return true;
        }
        if key == "escape" && !modifiers.modified() && self.settings_drawer.open {
            self.close_settings(window, cx);
            return true;
        }

        false
    }

    /// The drawer over the content area, with the scrim that shuts it on a
    /// click beside it. `None` once it has slid shut.
    pub(super) fn render_settings_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.settings_drawer.open;
        let shown = crate::motion::slide_progress("settings-drawer", open, window, cx);
        if !open && crate::motion::settled(shown) {
            self.settings_drawer.drawn.clear();
            return None;
        }

        let panel = self.render_settings_panel(window, cx);
        let scrim = div()
            .id("settings-drawer-scrim")
            .test_support()
            .absolute()
            .inset_0()
            .bg(black().opacity(0.25 * shown.value))
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _event, window, cx| this.close_settings(window, cx)));
        let slot = div().absolute().top_0().right_0().h_full().child(crate::motion::slide_frame(
            DRAWER_WIDTH,
            shown.value,
            crate::motion::Edge::Right,
            panel,
        ));

        Some(div().absolute().inset_0().child(scrim).child(slot).into_any_element())
    }

    fn render_settings_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let header = h_flex()
            .flex_none()
            .gap_2()
            .items_center()
            .px_4()
            .pt_4()
            .pb_3()
            .child(crate::icons::icon(crate::icons::Glyph::Settings).text_color(crate::theme::accent()))
            .child(
                div().flex_1().text_base().font_weight(FontWeight::SEMIBOLD).child(t!("ui.tabs.settings").to_string()),
            )
            .child(
                Button::new("settings-close")
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .tooltip(t!("ui.settings.panel.close").to_string())
                    .on_click(cx.listener(|this, _event: &ClickEvent, window, cx| this.close_settings(window, cx))),
            );

        let search = div().flex_none().px_4().pb_2().child(
            Input::new(&self.settings_drawer.search)
                .id("settings-search")
                .small()
                .cleanable(true)
                .prefix(Icon::new(IconName::Search).small().text_color(crate::theme::text_dim())),
        );

        let body = match &self.settings {
            SettingsState::Loading => v_flex()
                .flex_1()
                .child(crate::loading::Loading::new("settings-loading", t!("ui.settings.loading").into_owned()).top())
                .into_any_element(),
            SettingsState::Failed(reason) => v_flex()
                .flex_1()
                .px_4()
                .gap_1()
                .child(div().text_sm().font_weight(FontWeight::BOLD).child(t!("ui.settings.load_failed").to_string()))
                .child(div().text_sm().text_color(crate::theme::text_dim()).child(reason.clone()))
                .into_any_element(),
            SettingsState::Loaded(_) => self.render_settings_rows(window, cx),
        };

        v_flex()
            .id("settings-drawer")
            .test_support()
            .size_full()
            .occlude()
            .bg(cx.theme().background)
            .border_l_1()
            .border_color(theme::border_bright())
            .shadow(theme::overlay_shadow())
            .child(header)
            .child(search)
            .child(body)
            .into_any_element()
    }

    /// The count, the section strip and the scrolling rows, for the query in
    /// the search field.
    fn render_settings_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.measure_cache_once(cx);

        let query = self.settings_drawer.query.of(&self.settings_drawer.search.read(cx).value());
        let rows: Vec<Setting> = Setting::ALL
            .into_iter()
            .filter(|setting| self.setting_present(*setting) && setting.matches(&query))
            .collect();
        // A tile the search takes away never reports the pointer leaving it.
        if !rows.contains(&Setting::Theme) && self.theme_preview.take().is_some() {
            theme::apply_egui_theme(self.theme, self.zoom, window, cx);
        }

        let mut drawn = Vec::with_capacity(rows.len() + SettingsSection::ALL.len());
        let mut children = Vec::with_capacity(drawn.capacity());
        for section in SettingsSection::ALL {
            let in_section: Vec<Setting> =
                rows.iter().copied().filter(|setting| setting.section() == section).collect();
            let Some(last) = in_section.len().checked_sub(1) else { continue };

            children.push(settings_heading(section, drawn.is_empty()));
            drawn.push(Drawn::Heading(section));
            for (position, setting) in in_section.into_iter().enumerate() {
                children.push(self.render_setting_row(setting, &query, position == 0, position == last, window, cx));
                drawn.push(Drawn::Row(setting));
            }
        }

        // The section at the top of the last frame's layout, which is the
        // layout `top_item` reads.
        let at_top =
            self.settings_drawer.drawn.get(self.settings_drawer.scroll.top_item()).map(|drawn| drawn.section());
        self.scroll_settings_into_place(&drawn, window);
        self.settings_drawer.drawn = drawn;

        let count = query.is_active().then(|| {
            let text = match rows.len() {
                1 => t!("ui.settings.panel.count_one").into_owned(),
                count => t!("ui.settings.panel.count", count = count).into_owned(),
            };
            div()
                .id("settings-count")
                .test_support()
                .aria_label(text.clone())
                .flex_none()
                .px_4()
                .pb_2()
                .text_xs()
                .text_color(crate::theme::text_dim())
                .child(text)
        });

        let categories = h_flex()
            .flex_none()
            .flex_wrap()
            .gap_1()
            .px_4()
            .pb_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .children(SettingsSection::ALL.map(|section| {
                let listed = rows.iter().any(|setting| setting.section() == section);
                let current = at_top == Some(section);
                selectable(
                    ("settings-category", section as usize),
                    current,
                    Button::new(("settings-category-button", section as usize))
                        .ghost()
                        .small()
                        .icon(Icon::from(section.glyph()))
                        .accessibility_label(section.name())
                        .tooltip(section.name())
                        .selected(current)
                        .disabled(!listed)
                        .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                            this.settings_drawer.scroll_to = Some(SettingsTarget::Section(section));
                            cx.notify();
                        })),
                )
            }));

        // Taken even when no rows are drawn, so that no later frame applies it.
        let anchor = self.settings_drawer.zoom_slider_anchor.take();
        let list = if children.is_empty() {
            let said = t!("ui.settings.panel.no_matches").into_owned();
            div()
                .id("settings-no-match")
                .test_support()
                .aria_label(said.clone())
                .flex_1()
                .p_4()
                .text_sm()
                .text_color(crate::theme::text_dim())
                .child(said)
                .into_any_element()
        } else {
            let list = v_flex()
                .id("settings-drawer-scroll")
                .test_support()
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .track_scroll(&self.settings_drawer.scroll)
                .px_4()
                .pt_3()
                .pb_4()
                .children(children)
                .into_any_element();
            AnchoredScroll {
                scroll: self.settings_drawer.scroll.clone(),
                slider: Rc::clone(&self.settings_drawer.zoom_slider),
                anchor,
                child: list,
            }
            .into_any_element()
        };

        v_flex().flex_1().min_h(px(0.)).children(count).child(categories).child(list).into_any_element()
    }

    /// Scrolls to the place asked for, once the last frame laid out the same
    /// children as this one: the scroll is measured against that layout, and
    /// a layout of other children would put it somewhere else.
    fn scroll_settings_into_place(&mut self, drawn: &[Drawn], window: &mut Window) {
        let Some(target) = self.settings_drawer.scroll_to else { return };

        if self.settings_drawer.drawn != drawn {
            window.request_animation_frame();
            return;
        }

        self.settings_drawer.scroll_to = None;
        // A setting that is not drawn now (the cache contents before the
        // cache is measured) is looked for under its section.
        let found = drawn.iter().position(|child| child.is(target)).or_else(|| match target {
            SettingsTarget::Setting(setting) => {
                drawn.iter().position(|child| *child == Drawn::Heading(setting.section()))
            }
            SettingsTarget::Section(_) => None,
        });
        if let Some(index) = found {
            self.settings_drawer.scroll.scroll_to_top_of_item(index);
        }
    }

    /// Whether a setting has a row at all, which for a few depends on what
    /// is there to show.
    fn setting_present(&self, setting: Setting) -> bool {
        let armor_saved = self.settings().is_some_and(|settings| settings.armor_defaults.is_some());
        match setting {
            Setting::CodeIntegrity => cfg!(windows),
            Setting::CachedBuilds => {
                self.cache_base().is_some() && self.cache.stats.is_some_and(|stats| stats.version_count > 0)
            }
            Setting::ArmorPlateEdges | Setting::ArmorOpaqueHull => armor_saved,
            Setting::ArmorNoDefaults => !armor_saved,
            _ => true,
        }
    }

    /// Starts measuring the cache, once per directory. The measurement walks
    /// every stored object, so it runs off the UI thread and is kept until
    /// something changes the cache.
    fn measure_cache_once(&mut self, cx: &mut Context<Self>) {
        let Some(base) = self.cache_base() else { return };
        let Some(generation) = self.cache.wants_measure() else { return };

        game_data_cache::measure(base, generation, cx, |this, measured, generation, cx| {
            this.cache.measured(measured, generation);
            cx.notify();
        });
    }

    fn render_setting_row(
        &self,
        setting: Setting,
        query: &MarkedQuery,
        first: bool,
        last: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (control, note) = self.setting_control(setting, window, cx);

        let text = v_flex()
            // A title narrower than this is a column of a few words, so the
            // control beside it drops under it instead.
            .min_w(rems(8.))
            .flex_1()
            .gap_0p5()
            .child(
                div()
                    .id("settings-row-title")
                    .test_support()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(query.marked(setting.title().into_owned())),
            )
            .child(
                div()
                    .id("settings-row-description")
                    .test_support()
                    .text_xs()
                    .text_color(crate::theme::text_dim())
                    .child(query.marked(setting.description().into_owned())),
            );
        let body = if setting.stacked() {
            v_flex().gap_2().child(text).children(control).into_any_element()
        } else {
            h_flex()
                .flex_wrap()
                .gap_x_3()
                .gap_y_2()
                .items_center()
                .child(text)
                .children(control.map(|control| div().flex_none().child(control)))
                .into_any_element()
        };
        let note = note.map(|note| {
            div()
                .id("settings-row-note")
                .test_support()
                .text_xs()
                .text_color(if note.error {
                    rgb(crate::theme::semantic().error).into()
                } else {
                    crate::theme::text_dim()
                })
                .child(note.text)
        });

        v_flex()
            .id(("settings-row", setting as usize))
            .test_support()
            .gap_1()
            .px_3()
            .py_2p5()
            .bg(crate::theme::card())
            .border_color(cx.theme().border)
            .border_l_1()
            .border_r_1()
            .border_b_1()
            .when(first, |row| row.border_t_1().rounded_t(CARD_RADIUS))
            .when(last, |row| row.rounded_b(CARD_RADIUS))
            .child(body)
            .children(note)
            .into_any_element()
    }

    /// The control a setting is changed with, and what is reported under it.
    fn setting_control(
        &self,
        setting: Setting,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> (Option<AnyElement>, Option<Note>) {
        let Some(settings) = self.settings() else { return (None, None) };
        let dim = |text: String| Some(Note { text, error: false });

        match setting {
            Setting::Language => (Some(self.language_control()), None),
            Setting::DataSharing => (
                Some(self.data_sharing_control(settings.data_sharing, cx)),
                dim(settings.data_sharing.description().to_owned()),
            ),
            Setting::CodeIntegrity => {
                (self.code_integrity_control(cx), dim(t!("ui.settings.app.code_integrity_tooltip").into_owned()))
            }
            Setting::Proxy => {
                (Some(Input::new(&self.proxy_input).id("proxy-url").small().w_full().into_any_element()), None)
            }
            Setting::CheckForUpdates => {
                let control = setting_switch("check-for-updates", setting, settings.check_for_updates).on_click(
                    cx.listener(|this, checked: &bool, _window, cx| {
                        let checked = *checked;
                        this.edit_setting(keys::CHECK_FOR_UPDATES, cx, |settings| {
                            settings.check_for_updates = checked;
                            checked
                        });
                    }),
                );
                (Some(control.into_any_element()), None)
            }
            Setting::EnableLogging => {
                let control = setting_switch("enable-logging", setting, settings.enable_logging).on_click(cx.listener(
                    |this, checked: &bool, _window, cx| {
                        let checked = *checked;
                        this.edit_setting(keys::ENABLE_LOGGING, cx, |settings| {
                            settings.enable_logging = checked;
                            checked
                        });
                    },
                ));
                (Some(control.into_any_element()), None)
            }
            Setting::DataFolder => {
                let control = Button::new("open-data-dir")
                    .icon(IconName::FolderOpen)
                    .label(t!("ui.settings.wows.cache.open_folder").to_string())
                    .compact()
                    .on_click(|_event, _window, _cx| open_data_directory());
                (Some(control.into_any_element()), None)
            }
            Setting::Theme => (Some(self.theme_control(settings.theme, cx)), None),
            Setting::Zoom => (Some(self.render_zoom_row(cx).into_any_element()), dim(crate::zoom::shortcuts_hint(cx))),
            Setting::WowsDirectory => {
                let note = self
                    .wows_dir_invalid
                    .then(|| Note { text: t!("ui.messages.wows_dir_invalid").into_owned(), error: true });
                (Some(self.wows_dir_control(window, cx)), note)
            }
            Setting::AutoDump => {
                let control = setting_switch("cache-auto-dump", setting, settings.auto_dump_game_data).on_click(
                    cx.listener(|this, checked: &bool, _window, cx| {
                        let on = *checked;
                        this.edit_setting(keys::AUTO_DUMP_GAME_DATA, cx, |settings| {
                            settings.auto_dump_game_data = on;
                            on
                        });
                    }),
                );
                (Some(control.into_any_element()), None)
            }
            Setting::CacheDirectory => (Some(self.cache_dir_control(window, cx)), None),
            Setting::CachedBuilds => {
                let base = self.cache_base();
                let stats = self.cache.stats;
                let control = base.zip(stats).map(|(base, stats)| self.render_cache_body(base, stats, cx));
                (control, None)
            }
            Setting::ReplayIndex => (Some(self.replay_index_control(cx)), None),
            Setting::CurrentReplayPath => {
                let path = settings.current_replay_path.display().to_string();
                let shown = if path.is_empty() { t!("ui.settings.not_set").into_owned() } else { path };
                let control = div().text_sm().child(crate::ui::selectable_text("settings-current-replay-path", shown));
                (Some(control.into_any_element()), None)
            }
            Setting::ShowRawXp => (
                Some(self.replay_switch("show-raw-xp", setting, settings.replay.show_raw_xp, cx, |replay, on| {
                    replay.show_raw_xp = on
                })),
                None,
            ),
            Setting::ShowObservedDamage => (
                Some(self.replay_switch(
                    "show-observed-damage",
                    setting,
                    settings.replay.show_observed_damage,
                    cx,
                    |replay, on| replay.show_observed_damage = on,
                )),
                None,
            ),
            Setting::ShowEntityId => (
                Some(self.replay_switch(
                    "show-entity-id",
                    setting,
                    settings.replay.show_entity_id,
                    cx,
                    |replay, on| replay.show_entity_id = on,
                )),
                None,
            ),
            Setting::ShowHeals => (
                Some(self.replay_switch("show-heals", setting, settings.replay.show_heals, cx, |replay, on| {
                    replay.show_heals = on
                })),
                None,
            ),
            Setting::ReplayPreviews => (
                Some(self.replay_switch(
                    "enable-replay-previews",
                    setting,
                    settings.replay.enable_replay_previews,
                    cx,
                    |replay, on| replay.enable_replay_previews = on,
                )),
                None,
            ),
            Setting::AutoExport => (Some(self.auto_export_control(&settings.replay, cx)), None),
            Setting::DisplayName => (
                Some(
                    Input::new(&self.collab_name_input).id("session-display-name").small().w_full().into_any_element(),
                ),
                None,
            ),
            Setting::SuppressIpWarning => {
                let control = setting_switch("session-suppress-ip-warning", setting, settings.suppress_p2p_ip_warning)
                    .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                        let on = *checked;
                        this.edit_setting(keys::SUPPRESS_P2P_IP_WARNING, cx, |settings| {
                            settings.suppress_p2p_ip_warning = on;
                            on
                        });
                    }));
                (Some(control.into_any_element()), None)
            }
            Setting::DisableAutoOpen => {
                let control =
                    setting_switch("session-disable-auto-open", setting, settings.disable_auto_open_session_windows)
                        .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                            let on = *checked;
                            this.edit_setting(keys::DISABLE_AUTO_OPEN_SESSION_WINDOWS, cx, |settings| {
                                settings.disable_auto_open_session_windows = on;
                                on
                            });
                        }));
                (Some(control.into_any_element()), None)
            }
            Setting::TwitchGetToken => {
                let control = Button::new("twitch-get-token")
                    .icon(Icon::from(crate::icons::Glyph::AppWindow))
                    .compact()
                    .accessibility_label(setting.title().into_owned())
                    .tooltip(t!("ui.settings.twitch.get_token_tooltip").to_string())
                    .on_click(|_event, _window, cx: &mut gpui_kit::App| cx.open_url(TWITCH_TOKEN_URL));
                (Some(control.into_any_element()), None)
            }
            Setting::TwitchToken => {
                let note = match self.twitch_paste.as_ref() {
                    Some(Ok(who)) => dim(t!("ui.settings.twitch.signed_in_as", who = who).into_owned()),
                    Some(Err(why)) => Some(Note { text: why.clone(), error: true }),
                    None => None,
                };
                (Some(self.twitch_token_control(cx)), note)
            }
            Setting::TwitchChannel => (
                Some(Input::new(&self.twitch_channel_input).id("twitch-channel").small().w_full().into_any_element()),
                None,
            ),
            // The viewport writes these itself when a pane changes, so the
            // drawer reports them rather than offering a second way to set them.
            Setting::ArmorPlateEdges | Setting::ArmorOpaqueHull => {
                let Some(defaults) = settings.armor_defaults.as_ref() else { return (None, None) };
                let (id, on) = match setting {
                    Setting::ArmorPlateEdges => ("armor-show-plate-edges", defaults.show_plate_edges),
                    _ => ("armor-hull-opaque", defaults.hull_opaque),
                };
                (Some(setting_switch(id, setting, on).disabled(true).into_any_element()), None)
            }
            Setting::ArmorNoDefaults => (None, None),
        }
    }

    fn replay_switch(
        &self,
        id: &'static str,
        setting: Setting,
        on: bool,
        cx: &mut Context<Self>,
        apply: fn(&mut ReplaySettings, bool),
    ) -> AnyElement {
        setting_switch(id, setting, on)
            .on_click(cx.listener(move |this, checked: &bool, _window, cx| {
                let checked = *checked;
                this.edit_replay_settings(cx, |replay| apply(replay, checked));
            }))
            .into_any_element()
    }

    fn language_control(&self) -> AnyElement {
        crate::ui::boxed(LANGUAGE_COMBO_WIDTH, crate::ui::SELECT_SMALL_HEIGHT)
            .child(
                Select::new(&self.language_select)
                    .id("settings-language")
                    .accessibility_label(t!("ui.settings.app.language").to_string())
                    .small()
                    .w(LANGUAGE_COMBO_WIDTH)
                    .menu_width(LANGUAGE_COMBO_WIDTH),
            )
            .into_any_element()
    }

    fn data_sharing_control(&self, chosen: DataSharingMode, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .flex_wrap()
            .gap_1()
            .children(DataSharingMode::ALL.map(|mode| {
                selectable(
                    ("data-sharing", mode as usize),
                    chosen == mode,
                    Button::new(("data-sharing-button", mode as usize))
                        .label(mode.label())
                        .compact()
                        .selected(chosen == mode)
                        .tooltip(mode.description())
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.adopt_data_sharing(mode, cx);
                        })),
                )
            }))
            .into_any_element()
    }

    /// Whether unsigned code may load into this process, which is the one
    /// mitigation with a compatibility cost worth a choice.
    ///
    /// Windows only: the policy does not exist elsewhere, and a control for one
    /// that cannot be applied would be a control that does nothing. The change
    /// lands at the next launch, because the policy cannot be lifted once a
    /// process has started, which is what the label says.
    #[cfg(windows)]
    fn code_integrity_control(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use wows_toolkit_hardening::CodeIntegrityPreference;

        let chosen = self.settings()?.code_integrity;
        let choices = h_flex().flex_wrap().gap_1().children(CodeIntegrityPreference::ALL.map(|choice| {
            selectable(
                ("code-integrity", choice as usize),
                chosen == choice,
                Button::new(("code-integrity-button", choice as usize))
                    .label(t!(choice.key()).to_string())
                    .compact()
                    .selected(chosen == choice)
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.edit_setting(keys::CODE_INTEGRITY, cx, |settings| {
                            settings.code_integrity = choice;
                            choice
                        });
                    })),
            )
        }));

        Some(choices.into_any_element())
    }

    /// Nothing, where the policy does not exist.
    #[cfg(not(windows))]
    fn code_integrity_control(&self, _cx: &mut Context<Self>) -> Option<AnyElement> {
        None
    }

    fn theme_control(&self, chosen: ThemeChoice, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .flex_wrap()
            .gap_2()
            .children(ThemeChoice::ALL.map(|choice| {
                let colors = theme::swatches(choice, cx);
                let color_summary = colors.iter().map(|color| format!("#{color:06X}")).collect::<Vec<_>>().join(" ");
                div()
                    .id(("theme-choice-preview", choice as usize))
                    // Previewed on a pointer move rather than on entry: a zoom
                    // change rescales the page under a still pointer, and a tile
                    // that lands under it was not pointed at.
                    .on_mouse_move(cx.listener(move |this, _event: &MouseMoveEvent, window, cx| {
                        if this.theme_preview == Some(choice) {
                            return;
                        }
                        this.theme_preview = Some(choice);
                        theme::apply_egui_theme(choice, this.zoom, window, cx);
                    }))
                    .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                        if *hovered || this.theme_preview != Some(choice) {
                            return;
                        }
                        this.theme_preview = None;
                        theme::apply_egui_theme(this.theme, this.zoom, window, cx);
                    }))
                    .child(selectable(
                        ("theme-choice", choice as usize),
                        chosen == choice,
                        Button::new(("theme-choice-button", choice as usize))
                            .accessibility_label(choice.label())
                            .child(v_flex().gap_1().child(div().text_xs().child(choice.label())).child(
                                h_flex().gap_1().children(
                                    colors.map(|color| div().w(px(20.)).h(px(9.)).rounded(px(1.)).bg(rgb(color))),
                                ),
                            ))
                            .w(THEME_TILE_WIDTH)
                            .compact()
                            .h_10()
                            .selected(chosen == choice)
                            .tooltip(format!("{}  {}", choice.label(), color_summary))
                            .on_click(cx.listener(move |this, _event, window, cx| {
                                this.set_theme(choice, window, cx);
                            })),
                    ))
            }))
            .into_any_element()
    }

    fn wows_dir_control(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .flex_wrap()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w(PATH_FIELD_MIN_WIDTH)
                    .when(self.wows_dir_invalid, |this| this.text_color(rgb(crate::theme::semantic().error)))
                    .child(path_field(Setting::WowsDirectory, "wows-dir", &self.wows_dir_input, window, cx)),
            )
            .child(
                Button::new("wows-dir-browse")
                    .icon(IconName::FolderOpen)
                    .label(t!("ui.settings.wows.browse").to_string())
                    .compact()
                    .on_click(cx.listener(|this, _event, window, cx| this.browse_for_wows_dir(window, cx))),
            )
            .into_any_element()
    }

    fn cache_dir_control(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .flex_wrap()
            .gap_2()
            .child(div().flex_1().min_w(PATH_FIELD_MIN_WIDTH).child(path_field(
                Setting::CacheDirectory,
                "cache-dir",
                &self.cache_dir_input,
                window,
                cx,
            )))
            .child(
                Button::new("cache-dir-browse")
                    .icon(IconName::FolderOpen)
                    .label(t!("ui.settings.wows.cache.browse").to_string())
                    .compact()
                    .on_click(cx.listener(|this, _event, window, cx| this.browse_for_cache_dir(window, cx))),
            )
            .into_any_element()
    }

    /// What a cache holding at least one build offers: its size, the jobs
    /// that check it, and whatever those jobs found.
    fn render_cache_body(
        &self,
        base: PathBuf,
        stats: wows_data_mgr::dump::CacheStats,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let busy = self.cache.busy();
        let running = self.cache.running.is_some();

        let summary = h_flex()
            .gap_2()
            .flex_wrap()
            .items_center()
            .child(
                // Without a zero minimum the text keeps its unwrapped width and
                // runs past the card.
                div().id("cache-stats").test_support().min_w_0().text_sm().child(
                    t!(
                        "ui.settings.wows.cache.stats",
                        size = humansize::format_size(stats.total_bytes, humansize::BINARY),
                        count = stats.version_count,
                    )
                    .to_string(),
                ),
            )
            .child({
                let folder = base.clone();
                Button::new("cache-open-folder")
                    .label(t!("ui.settings.wows.cache.open_folder").to_string())
                    .compact()
                    .on_click(move |_event, _window, _cx| open_directory(&folder))
            })
            // Nothing to prune while there is one build: the newest is kept.
            .when(stats.version_count > 1, |this| {
                let dir = base.clone();
                this.child(
                    Button::new("cache-delete-old")
                        .label(t!("ui.settings.wows.cache.delete_old").to_string())
                        .compact()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.delete_old_cache_versions(dir.clone(), cx)
                        })),
                )
            });

        let jobs = h_flex()
            .gap_2()
            .items_center()
            .child(
                Button::new("cache-actions")
                    .label(t!("ui.settings.wows.cache.actions").to_string())
                    .compact()
                    .disabled(busy)
                    .tooltip(t!("ui.settings.wows.cache.actions_tooltip").to_string())
                    .dropdown_menu({
                        let app = cx.entity().downgrade();
                        let check_dir = base.clone();
                        let validate_dir = base.clone();
                        move |menu, _window, _cx| {
                            let app_for_check = app.clone();
                            let app_for_validate = app.clone();
                            let check_dir = check_dir.clone();
                            let validate_dir = validate_dir.clone();
                            menu.item(
                                PopupMenuItem::new(t!("ui.settings.wows.cache.check_updates").into_owned()).on_click(
                                    move |_event, _window, cx| {
                                        if let Some(app) = app_for_check.upgrade() {
                                            app.update(cx, |app, cx| {
                                                app.check_cache_for_updates(check_dir.clone(), cx)
                                            });
                                        }
                                    },
                                ),
                            )
                            .item(
                                PopupMenuItem::new(t!("ui.settings.wows.cache.validate").into_owned()).on_click(
                                    move |_event, _window, cx| {
                                        if let Some(app) = app_for_validate.upgrade() {
                                            app.update(cx, |app, cx| app.validate_cache(validate_dir.clone(), cx));
                                        }
                                    },
                                ),
                            )
                        }
                    }),
            )
            .when(running, |this| this.child(Spinner::new()));

        let progress = self.cache.progress.filter(|step| step.total > 0).map(|step| {
            div().text_xs().text_color(crate::theme::text_dim()).child(format!("{} / {}", step.done, step.total))
        });

        let updates = (!self.cache.updates.is_empty()).then(|| {
            let count = self.cache.updates.len();
            let dir = base.clone();
            h_flex()
                .flex_wrap()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .id("cache-updates")
                        .test_support()
                        .min_w_0()
                        .text_sm()
                        .child(t!("ui.settings.wows.cache.updates_available", count = count).to_string()),
                )
                .child(
                    Button::new("cache-update-all")
                        .label(t!("ui.settings.wows.cache.update_all").to_string())
                        .compact()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            let builds = this.cache.updates.clone();
                            this.fetch_cache_builds(dir.clone(), builds, cx);
                        })),
                )
        });

        let repair = (!self.cache.repair.is_empty()).then(|| {
            let count = self.cache.repair.len();
            let dir = base.clone();
            h_flex()
                .flex_wrap()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .id("cache-repair-needed")
                        .test_support()
                        .min_w_0()
                        .text_sm()
                        .text_color(rgb(crate::theme::semantic().error))
                        .child(t!("ui.settings.wows.cache.repair_needed", count = count).to_string()),
                )
                .child(
                    Button::new("cache-repair")
                        .label(t!("ui.settings.wows.cache.repair").to_string())
                        .compact()
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            let builds = this.cache.repair.clone();
                            this.fetch_cache_builds(dir.clone(), builds, cx);
                        })),
                )
        });

        let failure = self.cache.failure.clone().map(|reason| {
            div()
                .id("cache-failure")
                .test_support()
                .text_xs()
                .text_color(rgb(crate::theme::semantic().error))
                .child(reason)
        });

        v_flex()
            .gap_2()
            .child(summary)
            .child(jobs)
            .children(progress)
            .children(updates)
            .children(repair)
            .children(failure)
            .into_any_element()
    }

    /// Building the replay index, and how far a build has got. Refused
    /// without a game directory, since the replays are under it.
    fn replay_index_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let has_dir = self.settings().is_some_and(|settings| !settings.wows_dir.is_empty());
        let running = self.index_cancel.is_some();

        let buttons = h_flex()
            .flex_wrap()
            .gap_2()
            .items_center()
            .child(
                Button::new("index-build")
                    .label(t!("ui.settings.index.build").to_string())
                    .compact()
                    .disabled(!has_dir || running)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.build_replay_index(crate::replay_index::IndexMode::FillGaps, cx)
                    })),
            )
            // For rows an older parse decoded through constants that did not
            // fit: nothing else can correct them (the egui app's Re-index All).
            .child(
                Button::new("index-rebuild")
                    .label(t!("ui.settings.index.rebuild").to_string())
                    .compact()
                    .disabled(!has_dir || running)
                    .tooltip(t!("ui.settings.index.rebuild_hint").to_string())
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.build_replay_index(crate::replay_index::IndexMode::RefreshAll, cx)
                    })),
            )
            .when(running, |this| {
                this.child(Spinner::new()).child(
                    Button::new("index-stop").label(t!("ui.buttons.stop").to_string()).compact().on_click(cx.listener(
                        |this, _event, _window, cx| {
                            if let Some(cancel) = &this.index_cancel {
                                cancel.store(true, Ordering::Relaxed);
                            }
                            cx.notify();
                        },
                    )),
                )
            });

        let progress = self.index_progress.map(|step| {
            let said = if step.skipped > 0 {
                t!(
                    "ui.settings.index.progress_skipped",
                    done = step.done,
                    total = step.total,
                    skipped = step.skipped,
                    failed = step.failed
                )
            } else {
                t!("ui.settings.index.progress", done = step.done, total = step.total, failed = step.failed)
            };
            div().text_xs().text_color(crate::theme::text_dim()).child(said.into_owned())
        });
        let outcome =
            self.index_outcome.clone().map(|text| div().text_xs().text_color(crate::theme::text_dim()).child(text));

        v_flex().gap_2().child(buttons).children(progress).children(outcome).into_any_element()
    }

    /// The auto-export controls: whether to write a file per battle, in which
    /// format, and where.
    ///
    /// The directory is shown rather than typed: a path typed into a field is
    /// only checked when it loses focus, and one that does not exist reads as
    /// working until the first battle fails to write.
    fn auto_export_control(&self, replay: &ReplaySettings, cx: &mut Context<Self>) -> AnyElement {
        let enabled = replay.auto_export_data;
        let chosen = replay.auto_export_format;
        let directory = replay.auto_export_path.clone();
        // A directory that is not there cannot be written to, and the reader
        // has no other way to find that out until a battle is lost.
        let missing = !directory.is_empty() && !std::path::Path::new(&directory).is_dir();

        let format = h_flex()
            .flex_wrap()
            .gap_3()
            .items_center()
            .child(setting_switch("auto-export-data", Setting::AutoExport, enabled).on_click(cx.listener(
                |this, checked: &bool, _window, cx| {
                    let checked = *checked;
                    this.edit_replay_settings(cx, |replay| replay.auto_export_data = checked);
                },
            )))
            .child(h_flex().gap_1().children(EXPORT_FORMATS.map(|format| {
                selectable(
                    ("auto-export-format", format as usize),
                    chosen == format,
                    Button::new(("auto-export-format-button", format as usize))
                        .label(format.as_str().to_string())
                        .compact()
                        .selected(chosen == format)
                        .disabled(!enabled)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.edit_replay_settings(cx, |replay| replay.auto_export_format = format);
                        })),
                )
            })));

        let place = h_flex()
            .gap_2()
            .items_center()
            .child(
                Button::new("auto-export-choose")
                    .label(t!("ui.buttons.choose").to_string())
                    .compact()
                    .disabled(!enabled)
                    .on_click(cx.listener(|this, _event, _window, cx| this.choose_export_directory(cx))),
            )
            .child(
                div()
                    .id("settings-auto-export-path")
                    .test_support()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle()
                    .text_sm()
                    .when(missing, |path| path.text_color(rgb(crate::theme::semantic().error)))
                    .map(|path| {
                        if directory.is_empty() {
                            path.child(t!("ui.settings.not_set").into_owned())
                        } else {
                            path.tooltip(path_tooltip(directory.clone().into())).child(directory)
                        }
                    }),
            );

        v_flex().gap_2().child(format).child(place).into_any_element()
    }

    fn twitch_token_control(&self, cx: &mut Context<Self>) -> AnyElement {
        // The label wraps rather than pushing the button past the card; the
        // button grows in height to hold it.
        let label = div().min_w_0().whitespace_normal().child(t!(self.twitch_paste_label_key()).to_string());
        h_flex()
            .child(
                Button::new("twitch-paste-token")
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_1()
                            .child(crate::icons::icon(crate::icons::Glyph::ClipboardList))
                            .child(label)
                            .child(
                                div()
                                    .text_color(rgb(self.twitch_status_tone()))
                                    .child(crate::icons::icon(self.twitch_status_glyph())),
                            ),
                    )
                    .compact()
                    .max_w_full()
                    .h_auto()
                    .min_h_6()
                    .py_0p5()
                    .tooltip(t!("ui.settings.twitch.paste_token_tooltip").to_string())
                    .on_click(cx.listener(|this, _event, _window, cx| this.paste_twitch_token(cx))),
            )
            .into_any_element()
    }
}

/// A switch for an on/off setting, named for assistive technology by the
/// setting's title, which is the label beside it.
fn setting_switch(id: &'static str, setting: Setting, on: bool) -> Switch {
    Switch::new(id).checked(on).small().accessibility_label(setting.title().into_owned())
}

/// A field for a directory. While it is not focused, the path is drawn over
/// it shortened in the middle, since a path is told apart by its ends, with
/// the whole path in a tooltip. An empty field shows its placeholder that way.
///
/// The field itself stays under the drawing, transparent rather than hidden,
/// so that a click still places the caret and Tab still reaches it.
fn path_field(
    setting: Setting,
    input_id: &'static str,
    state: &Entity<InputState>,
    window: &Window,
    cx: &gpui_kit::App,
) -> AnyElement {
    let field = state.read(cx);
    let editing = field.focus_handle(cx).is_focused(window);
    let shown = (!editing).then(|| {
        let value = field.value();
        let (path, placeholder) =
            if value.is_empty() { (field.presentation().placeholder().clone(), true) } else { (value, false) };
        h_flex()
            .id(("settings-path", setting as usize))
            .test_support()
            .absolute()
            .inset_0()
            .input_px(Size::Small)
            .input_text_size(Size::Small)
            .bg(cx.theme().input_background())
            .border_1()
            .border_color(cx.theme().input)
            .rounded(cx.theme().radius)
            .cursor_text()
            .when(placeholder, |this| this.text_color(cx.theme().muted_foreground))
            .tooltip(path_tooltip(path.clone()))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis_middle().child(path))
    });

    div()
        .relative()
        .w_full()
        .child(div().when(!editing, |input| input.opacity(0.)).child(Input::new(state).id(input_id).small().w_full()))
        .children(shown)
        .into_any_element()
}

/// A tooltip with the whole of a path that is drawn shortened.
fn path_tooltip(path: SharedString) -> impl Fn(&mut Window, &mut gpui_kit::App) -> AnyView + 'static {
    move |window, cx| {
        let path = path.clone();
        Tooltip::element(move |_window, _cx| Label::new(path.clone())).build(window, cx)
    }
}

/// A section's name over its card, which the strip's button for it jumps to.
fn settings_heading(section: SettingsSection, first: bool) -> AnyElement {
    h_flex()
        .id(("settings-section", section as usize))
        .test_support()
        .gap_2()
        .items_center()
        .pb_2()
        .when(!first, |heading| heading.pt_5())
        .text_xs()
        .child(crate::icons::icon(section.glyph()).text_color(crate::theme::icon_accent()))
        .child(crate::ui::section_label(section.name()))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui_kit::AppContext as _;
    use gpui_kit::Bounds;
    use gpui_kit::ElementId;
    use gpui_kit::Entity;
    use gpui_kit::Pixels;
    use gpui_kit::TestAppContext;
    use gpui_kit::Window;
    use gpui_kit::WindowHandle;
    use gpui_kit::component::ActiveTheme as _;
    use gpui_kit::component::Root;
    use gpui_kit::point;
    use gpui_kit::px;
    use gpui_kit::rems;
    use gpui_kit::test::TestWindowExt as _;
    use rust_i18n::t;

    use super::DRAWER_WIDTH;
    use super::Setting;
    use super::SettingsSection;
    use crate::app::App;
    use crate::app::AppTab;

    const OPEN: &str = "settings-open";
    const DRAWER: &str = "settings-drawer";
    const SCROLL: &str = "settings-drawer-scroll";

    /// The app as `main.rs` mounts it, with settings loaded so the rows are
    /// drawn.
    fn open(cx: &mut TestAppContext) -> (WindowHandle<Root>, Entity<App>) {
        let (window, app) = crate::interaction_tests::open_app_in_root(cx);
        cx.update_window(window.into(), |_, window, cx| {
            app.update(cx, |app, cx| app.apply_settings(crate::interaction_tests::test_settings(), window, cx));
            window.render_frame(cx);
        })
        .expect("the test window stays open");
        (window, app)
    }

    /// Turns the slide off, for tests about what the drawer holds rather than
    /// how it moves.
    fn hold_still(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
    }

    fn row(setting: Setting) -> (&'static str, usize) {
        ("settings-row", setting as usize)
    }

    fn top(window: &Window, id: impl Into<ElementId>) -> Pixels {
        window.find(id).bounds().top()
    }

    fn at_top_of_scroll(window: &Window, id: impl Into<ElementId>) -> bool {
        let scroll = top(window, SCROLL);
        let shown = top(window, id);
        shown > scroll - px(1.) && shown < scroll + px(1.)
    }

    #[gpui_kit::test]
    fn the_gear_slides_the_drawer_in_and_the_close_button_slides_it_out(cx: &mut TestAppContext) {
        let (window, _app) = open(cx);

        cx.update_window(window.into(), |_, window, cx| {
            assert!(window.try_find(DRAWER).is_none(), "the drawer is shut at startup");
            window.click(OPEN, cx);
            window.render_frame(cx);
            assert!(window.try_find(DRAWER).is_some(), "the gear opens it");
        })
        .expect("the test window stays open");
        cx.executor().advance_clock(std::time::Duration::from_millis(400));

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            let drawer = window.find(DRAWER).bounds();
            assert_eq!(drawer.right(), window.viewport_size().width, "it comes to rest against the right edge");
            assert_eq!(drawer.size.width, DRAWER_WIDTH);
            window.click("settings-close", cx);
            window.render_frame(cx);
            assert!(window.try_find(DRAWER).is_some(), "it slides out rather than vanishing");
        })
        .expect("the test window stays open");
        cx.executor().advance_clock(std::time::Duration::from_millis(400));

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find(DRAWER).is_none(), "the close button shuts it");
        })
        .expect("the test window stays open");
    }

    #[gpui_kit::test]
    fn the_shortcut_toggles_the_drawer_and_escape_shuts_it(cx: &mut TestAppContext) {
        let (window, _app) = open(cx);
        hold_still(cx);

        step(cx, window, &|window, cx| window.press("ctrl-,", cx));
        step(cx, window, &|window, cx| {
            assert!(window.try_find(DRAWER).is_some(), "ctrl+, opens it");
            assert_eq!(window.find("settings-search").focused(), Some(true), "with the search ready for typing");
            window.press("escape", cx);
        });
        step(cx, window, &|window, cx| {
            assert!(window.try_find(DRAWER).is_none(), "escape shuts it, from the search field");
            window.press("ctrl-,", cx);
        });
        step(cx, window, &|window, cx| {
            assert!(window.try_find(DRAWER).is_some());
            window.press("ctrl-,", cx);
        });
        step(cx, window, &|window, _cx| assert!(window.try_find(DRAWER).is_none(), "the shortcut shuts it as well"));
    }

    #[gpui_kit::test]
    fn a_click_beside_the_drawer_shuts_it_and_one_inside_does_not(cx: &mut TestAppContext) {
        let (window, _app) = open(cx);
        hold_still(cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.click(OPEN, cx);
            window.render_frame(cx);
            window.click_at(DRAWER, point(px(200.), px(20.)), cx);
            window.render_frame(cx);
            assert!(window.try_find(DRAWER).is_some(), "a click on the drawer itself leaves it open");

            let beside = point(DRAWER_WIDTH + px(100.), px(100.));
            window.click_at("settings-drawer-scrim", beside, cx);
            window.render_frame(cx);
            assert!(window.try_find(DRAWER).is_none(), "a click on the page beside it shuts it");
        })
        .expect("the test window stays open");
    }

    /// The search reads names and descriptions across every section, and says
    /// how many settings it kept.
    #[gpui_kit::test]
    fn the_search_filters_every_section_and_counts_what_it_kept(cx: &mut TestAppContext) {
        let (window, _app) = open(cx);
        hold_still(cx);

        step(cx, window, &|window, cx| window.click(OPEN, cx));
        step(cx, window, &|window, cx| {
            assert!(window.try_find("settings-count").is_none(), "nothing is counted before a search");
            // Only in the descriptions of the four column switches.
            window.input("results table", cx);
        });
        step(cx, window, &|window, _cx| {
            assert_eq!(window.find("settings-count").label(), Some("4 settings"));
            for kept in [Setting::ShowRawXp, Setting::ShowObservedDamage, Setting::ShowEntityId, Setting::ShowHeals] {
                assert!(window.try_find(row(kept)).is_some(), "{kept:?} is kept");
            }
            assert!(window.try_find(row(Setting::Language)).is_none(), "the rest are not");
            assert!(window.try_find(("settings-section", SettingsSection::General as usize)).is_none());
            assert!(window.try_find(("settings-section", SettingsSection::Replays as usize)).is_some());
            assert_eq!(
                window.find(("settings-category-button", SettingsSection::General as usize)).disabled(),
                Some(true),
                "a section with nothing left cannot be jumped to"
            );
        });

        cx.update_window(window.into(), |_, window, cx| {
            for _ in "results table".chars() {
                window.press("backspace", cx);
            }
            // A title, in another case than it is written in.
            crate::text_filter::take_marks();
            window.input("RAW XP", cx);
            window.render_frame(cx);
            assert_eq!(window.find("settings-count").label(), Some("1 setting"));
            assert!(window.try_find(row(Setting::ShowRawXp)).is_some());
            assert_eq!(
                crate::text_filter::marks_for("Show Raw XP"),
                Some(std::iter::once(5..11).collect()),
                "the title marks the match"
            );
            assert_eq!(
                crate::text_filter::marks_for("Add a raw XP column to the results table"),
                Some(std::iter::once(6..12).collect()),
                "so does the description"
            );

            window.input("zzz", cx);
            window.render_frame(cx);
            assert_eq!(window.find("settings-count").label(), Some("0 settings"));
            assert!(window.try_find("settings-no-match").is_some(), "an empty result says so");
        })
        .expect("the test window stays open");
    }

    #[gpui_kit::test]
    fn a_category_button_scrolls_its_section_to_the_top(cx: &mut TestAppContext) {
        let (window, _app) = open(cx);
        hold_still(cx);
        let section = SettingsSection::GameDataCache;
        let heading = ("settings-section", section as usize);

        cx.update_window(window.into(), |_, window, cx| {
            window.click(OPEN, cx);
            window.render_frame(cx);
            assert!(!at_top_of_scroll(window, heading), "the section starts further down");
            assert_eq!(window.find(("settings-category", section as usize)).selected(), Some(false));

            window.click(("settings-category", section as usize), cx);
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(at_top_of_scroll(window, heading), "its heading is at the top of the list");
            assert_eq!(
                window.find(("settings-category", section as usize)).selected(),
                Some(true),
                "and its button is marked"
            );
        })
        .expect("the test window stays open");
    }

    /// A message about a setting opens the drawer with that setting at the
    /// top.
    #[gpui_kit::test]
    fn a_message_about_a_setting_opens_the_drawer_at_it(cx: &mut TestAppContext) {
        let (window, app) = open(cx);
        hold_still(cx);
        let not_an_install = std::env::temp_dir().join("wows-toolkit-not-an-install");

        cx.update_window(window.into(), |_, window, cx| {
            app.update(cx, |app, cx| app.apply_wows_dir(not_an_install.to_string_lossy().into_owned(), window, cx));
            window.render_frame(cx);
            assert!(window.try_find(DRAWER).is_none());

            window.click("toast-action", cx);
            window.render_frame(cx);
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(window.try_find(DRAWER).is_some(), "the message's button opens the drawer");
            assert!(at_top_of_scroll(window, row(Setting::WowsDirectory)), "at the directory it is about");
        })
        .expect("the test window stays open");
    }

    /// Runs one step in its own update, then lets what it typed or confirmed
    /// reach the palette, which happens when effects flush after the update.
    fn step(cx: &mut TestAppContext, window: WindowHandle<Root>, act: &dyn Fn(&mut Window, &mut gpui_kit::App)) {
        cx.update_window(window.into(), |_, window, cx| act(window, cx)).expect("the test window stays open");
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
    }

    /// The palette's entry opens the drawer ready for typing, and while it is
    /// open, a tab chosen from the palette shuts it.
    #[gpui_kit::test]
    fn the_palette_opens_the_drawer_and_a_tab_chosen_there_shuts_it(cx: &mut TestAppContext) {
        let (window, app) = open(cx);
        hold_still(cx);
        let open_settings = t!("ui.settings.panel.open").into_owned();
        let go_to_stats = t!("ui.palette.go_to", tab = AppTab::Stats.label()).into_owned();

        step(cx, window, &|window, cx| window.press("ctrl-k", cx));
        step(cx, window, &|window, cx| window.input(&open_settings, cx));
        step(cx, window, &|window, cx| window.press("enter", cx));
        step(cx, window, &|window, _cx| {
            assert!(window.try_find(DRAWER).is_some(), "the entry opens the drawer");
            assert_eq!(window.find("settings-search").focused(), Some(true), "with the search ready for typing");
        });

        step(cx, window, &|window, cx| window.press("ctrl-k", cx));
        step(cx, window, &|window, cx| window.input(&go_to_stats, cx));
        step(cx, window, &|window, cx| window.press("enter", cx));
        step(cx, window, &|window, _cx| assert!(window.try_find(DRAWER).is_none(), "a tab entry shuts it"));
        cx.update(|cx| assert_eq!(app.read(cx).active_tab, AppTab::Stats, "and shows the tab"));
    }

    /// Switching tabs is a click beside the drawer, so it shuts it.
    #[gpui_kit::test]
    fn choosing_a_tab_shuts_the_drawer(cx: &mut TestAppContext) {
        let (window, _app) = open(cx);
        hold_still(cx);

        cx.update_window(window.into(), |_, window, cx| {
            window.click(OPEN, cx);
            window.render_frame(cx);
            window.within("app-tabs").click(1usize, cx);
            window.render_frame(cx);
            assert!(window.try_find(DRAWER).is_none());
        })
        .expect("the test window stays open");
    }

    /// A directory deep enough that its path is wider than the drawer.
    fn long_directory(base: &std::path::Path) -> std::path::PathBuf {
        base.join("a-directory-with-a-long-name-for-the-game-data-cache").join("and-a-second-directory-under-it")
    }

    /// Applies settings that point the cache, the export and the current
    /// replay at `directory`, and opens the drawer at the cache once it is
    /// measured.
    fn show_cache(directory: &std::path::Path, window: WindowHandle<Root>, app: &Entity<App>, cx: &mut TestAppContext) {
        let mut settings = crate::interaction_tests::test_settings();
        settings.game_data_cache_dir = directory.to_string_lossy().into_owned();
        settings.replay.auto_export_path = directory.to_string_lossy().into_owned();
        settings.current_replay_path = directory.to_path_buf();
        cx.update_window(window.into(), |_, window, cx| {
            app.update(cx, |app, cx| app.apply_settings(settings, window, cx));
            crate::interaction_tests::show_settings(window, SettingsSection::GameDataCache, cx);
        })
        .expect("the test window stays open");
        // The cache is measured off the UI thread; its card follows.
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| window.render_frame(cx)).expect("the test window stays open");
    }

    fn zoom_to(zoom: f32, window: WindowHandle<Root>, app: &Entity<App>, cx: &mut TestAppContext) {
        cx.update_window(window.into(), |_, window, cx| {
            app.update(cx, |app, cx| app.set_zoom(crate::zoom::ZoomFactor::new(zoom), window, cx));
            // The kit sets the rem size from the theme as the frame starts,
            // so the second frame is the first laid out at the new size.
            window.render_frame(cx);
            window.render_frame(cx);
        })
        .expect("the test window stays open");
    }

    /// Whether `inner` lies within `outer`, give or take rounding.
    fn contains(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
        let slack = px(0.5);

        inner.left() >= outer.left() - slack
            && inner.right() <= outer.right() + slack
            && inner.top() >= outer.top() - slack
            && inner.bottom() <= outer.bottom() + slack
    }

    /// At every zoom the slider offers, going up and coming back down, each
    /// card's text and controls stay inside it and the cards inside the
    /// drawer. A control beside its title at 1x drops under it at 2x.
    #[gpui_kit::test]
    fn every_row_keeps_its_contents_inside_its_card_at_every_zoom(cx: &mut TestAppContext) {
        use wows_data_mgr::download_repo::BuildUpdateStatus;

        // Rows that must be drawn, so one left out fails rather than passing
        // unchecked.
        const EXPECTED_ROWS: [Setting; 8] = [
            Setting::Language,
            Setting::Zoom,
            Setting::WowsDirectory,
            Setting::CacheDirectory,
            Setting::CachedBuilds,
            Setting::CurrentReplayPath,
            Setting::AutoExport,
            Setting::TwitchToken,
        ];

        let base = tempfile::tempdir().expect("a temporary directory can be made");
        let directory = long_directory(base.path());
        crate::interaction_tests::cached_build(&directory, "1.0.0", 100);
        let (window, app) = open(cx);
        show_cache(&directory, window, &app, cx);
        // What a check and a failed job leave under the cache's controls.
        app.update(cx, |app, cx| {
            app.cache.updates = vec![BuildUpdateStatus { build: 100, version: "1.0.0".to_string() }];
            app.cache.repair = vec![BuildUpdateStatus { build: 100, version: "1.0.0".to_string() }];
            app.cache.failure = Some(format!("The repository could not be read: {}", directory.display()));
            cx.notify();
        });
        zoom_to(1.0, window, &app, cx);
        let actual_size = cx.update(|cx| cx.theme().font_size);

        for zoom in [0.5, 1.0, 1.5, 2.0, 1.5, 1.0, 0.5] {
            zoom_to(zoom, window, &app, cx);
            assert_eq!(cx.update(|cx| cx.theme().font_size), actual_size * zoom, "the drawer is drawn at {zoom}x");

            cx.update_window(window.into(), |_, window, _cx| {
                for id in ["cache-stats", "cache-updates", "cache-repair-needed", "cache-failure"] {
                    assert!(window.try_find(id).is_some(), "{id} is drawn at {zoom}x");
                }
                let scroll = window.find(SCROLL).bounds();
                for setting in Setting::ALL {
                    let Some(card) = window.try_find(row(setting)) else {
                        assert!(!EXPECTED_ROWS.contains(&setting), "{setting:?} is drawn at {zoom}x");
                        continue;
                    };
                    let card = card.bounds();
                    assert!(
                        card.left() >= scroll.left() && card.right() <= scroll.right(),
                        "{setting:?} at {zoom}x: the card {card:?} runs out of the drawer {scroll:?}"
                    );
                    for element in window.within(row(setting)).elements() {
                        assert!(
                            contains(card, element.bounds()),
                            "{setting:?} at {zoom}x: {:?} at {:?} runs out of its card {card:?}",
                            element.path().last(),
                            element.bounds()
                        );
                    }
                }

                let language = window.within(row(Setting::Language)).find("settings-language").bounds();
                let said = window.within(row(Setting::Language)).find("settings-row-description").bounds();
                if zoom == 1.0 {
                    assert!(language.left() >= said.right(), "at 1x the combo sits beside its title");
                }
                if zoom == 2.0 {
                    assert!(language.top() >= said.bottom(), "at 2x the combo drops under its title");
                }
            })
            .expect("the test window stays open");
        }
    }

    /// A cache directory wider than its field is drawn shortened inside the
    /// field, and a pointer resting on it shows the whole path. Focus, by a
    /// click or by Tab, puts the field itself in front.
    #[gpui_kit::test]
    fn a_long_cache_directory_is_shortened_with_the_whole_path_in_a_tooltip(cx: &mut TestAppContext) {
        let base = tempfile::tempdir().expect("a temporary directory can be made");
        let directory = long_directory(base.path());
        let whole = directory.to_string_lossy().into_owned();
        let (window, app) = open(cx);
        show_cache(&directory, window, &app, cx);
        let shown = ("settings-path", Setting::CacheDirectory as usize);
        let whole_shown = |window: &Window| {
            gpui_kit::base::test_support::texts(window).iter().any(|text| text.text() == whole && text.visible())
        };

        cx.update_window(window.into(), |_, window, cx| {
            let field = window.find(shown).bounds();
            // The field's text size, which is `text_sm`.
            let text_size = rems(0.875).to_pixels(window.rem_size());
            let run = window.text_style().to_run(whole.len());
            let unshortened = window.text_system().layout_line(&whole, text_size, &[run], None).width;
            assert!(unshortened > field.size.width, "the path ({unshortened:?}) is wider than its field {field:?}");
            assert!(contains(window.find(row(Setting::CacheDirectory)).bounds(), field), "the field fits its card");
            assert!(!whole_shown(window), "the whole path is not shown before the pointer rests on it");
            window.hover(shown, cx);
        })
        .expect("the test window stays open");
        cx.executor().advance_clock(std::time::Duration::from_secs(1));
        cx.run_until_parked();

        cx.update_window(window.into(), |_, window, cx| {
            window.render_frame(cx);
            assert!(whole_shown(window), "the tooltip shows the whole path");

            window.click(shown, cx);
            window.render_frame(cx);
            assert!(window.try_find(shown).is_none(), "a click puts the field itself in front");
            assert_eq!(window.find("cache-dir").focused(), Some(true), "ready for typing");
            assert_eq!(window.find("cache-dir").value(), Some(whole.as_str()), "holding the whole path");

            window.press("tab", cx);
            window.render_frame(cx);
            assert_eq!(window.find("cache-dir").focused(), Some(false));
            assert!(window.try_find(shown).is_some(), "leaving the field shortens the path again");

            window.press("shift-tab", cx);
            window.render_frame(cx);
            assert_eq!(window.find("cache-dir").focused(), Some(true), "Tab reaches the field");
            assert!(window.try_find(shown).is_none());
        })
        .expect("the test window stays open");
    }
}
