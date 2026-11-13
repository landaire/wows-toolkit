use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::geom::CssPx;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::geom::Rect;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;
use zgui::slot;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Children;
use zgui::view::Classes;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollTarget;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::PointerButton;
use zgui::vocab::Role;
use zgui_ui::ContextMenuContentProps;
use zgui_ui::ContextMenuPlace;
use zgui_ui::ContextMenuProps;
use zgui_ui::ContextMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::TabsContext;
use zgui_ui::TabsListProps;
use zgui_ui::TabsProps;
use zgui_ui::TabsTriggerProps;
use zgui_ui_icons::IconData;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::mark::CROSS;
use zgui_ui_primitives::Binding;

const SHEET: &str = "landaire-ui-tab-strip";

/// The tab strip's own rules outweigh the zgui-ui tab rules they restate, which reach four
/// selectors deep, whichever sheet is installed first.
const CSS: &str = css!(
    ".lui-tab-strip.zui-tabs {
        gap: 0;
        min-width: 0;
    }
    .lui-tab-strip__scroller {
        display: flex;
        flex-direction: row;
        min-width: 0;
        overflow-x: auto;
        overflow-y: hidden;
        --zgui-scrollbar-width: none;
    }
    .lui-tab-strip__track {
        position: relative;
        display: flex;
        flex-direction: row;
        flex: none;
    }
    .lui-tab-strip .lui-tab-strip__track .lui-tab-strip__list.zui-tabs__list {
        width: auto;
        height: 32px;
        padding: 0;
        gap: 0;
        justify-content: flex-start;
        border-radius: 0;
        background-color: transparent;
    }
    .lui-tab-strip .lui-tab-strip__list .lui-tab-strip__tab.zui-tabs__trigger {
        flex: none;
        height: 100%;
        gap: 4px;
        padding: 0 12px;
        border-width: 0 1px;
        border-style: solid;
        border-color: transparent;
        border-radius: 0;
        background-color: transparent;
        box-shadow: none;
        color: var(--lui-color-text-dim);
        font-family: var(--lui-font-family);
        font-size: var(--lui-font-size);
        font-weight: 400;
    }
    .lui-tab-strip .lui-tab-strip__list .lui-tab-strip__tab.zui-tabs__trigger:hover {
        color: var(--lui-color-text);
    }
    .lui-tab-strip .lui-tab-strip__list .lui-tab-strip__tab.zui-tabs__trigger[data-state=\"active\"] {
        border-color: var(--lui-color-border);
        background-color: var(--lui-color-panel);
        box-shadow: none;
        color: var(--lui-color-text-bright);
        font-weight: 600;
    }
    .lui-tab-strip .lui-tab-strip__list .lui-tab-strip__tab.zui-tabs__trigger:focus-visible {
        outline: 1px solid var(--lui-color-accent);
        outline-offset: -2px;
    }
    .lui-tab-strip__label {
        white-space: nowrap;
    }
    .lui-tab-strip__close {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 16px;
        height: 16px;
        margin-right: -4px;
        padding: 0;
        border: none;
        border-radius: var(--lui-radius);
        background-color: transparent;
        color: var(--lui-color-text-dim);
    }
    .lui-tab-strip__close:hover {
        background-color: var(--lui-color-widget-hot);
        color: var(--lui-color-text-bright);
    }
    .lui-tab-strip__menu-area {
        flex: none;
        min-width: 0;
    }
    .lui-tab-strip__bar {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        min-width: 0;
    }
    .lui-tab-strip__bar > .lui-tab-strip__menu-area {
        align-self: stretch;
        flex: 1 1 auto;
    }
    .lui-tab-strip__end {
        display: flex;
        flex-direction: row;
        flex: none;
        align-items: center;
        gap: 2px;
    }
    .lui-tab-strip__underline {
        position: absolute;
        bottom: 0;
        left: 0;
        width: 0;
        height: 2px;
        background-color: var(--lui-color-accent);
        box-shadow: var(--lui-shadow-glow);
        pointer-events: none;
    }
    .lui-tab-strip__underline[data-gliding] {
        transition: left var(--lui-motion-rearrange) var(--lui-motion-ease),
                    width var(--lui-motion-rearrange) var(--lui-motion-ease);
    }"
);

/// Where the underline is drawn along the strip, in CSS pixels from the strip's start.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Span {
    left: f32,
    width: f32,
}

impl Span {
    /// The underline beneath a tab at `tab` in a strip at `strip`, both in device pixels at
    /// `scale`, kept clear of the tab's ends.
    fn under(tab: Rect<DevicePx, Device>, strip: Rect<DevicePx, Device>, scale: f32) -> Self {
        const INSET: f32 = 8.0;

        let left = (tab.origin.x.0 - strip.origin.x.0) / scale;
        let width = tab.size.width.0 / scale;
        let inset = INSET.min(width / 4.0);

        Self { left: left + inset, width: width - inset * 2.0 }
    }
}

/// Where the selected tab was laid out, as the tab itself measured it.
#[derive(Clone, Debug, PartialEq)]
struct SelectedTab {
    value: String,
    bounds: Rect<DevicePx, Device>,
}

/// Where the underline was last put, and under which tab.
#[derive(Clone, Debug, PartialEq)]
struct Placed {
    value: String,
    span: Span,
}

/// What the tabs of one strip report to it.
#[derive(Clone, Copy)]
struct StripContext {
    selected: RwSignal<Option<SelectedTab>, LocalStorage>,
    tabs: StoredValue<Vec<KnownTab>, LocalStorage>,
    serials: StoredValue<TabSerial, LocalStorage>,
    /// The tab the context menu was last asked for over.
    menu_target: RwSignal<Option<String>, LocalStorage>,
    /// Whether a tab named itself the target of the menu being asked for. A request over the
    /// strip's empty end opens nothing.
    menu_claimed: StoredValue<bool, LocalStorage>,
}

impl StripContext {
    /// Makes the tab called `value` known to the strip until the calling scope is cleaned up.
    fn register(self, value: String, trigger: NodeRef, on_close: Option<UnsyncCallback<()>>) {
        let serial = self.serials.get_value();
        self.serials.set_value(TabSerial(serial.0 + 1));
        self.tabs.update_value(|tabs| tabs.push(KnownTab { serial, value, trigger, on_close }));

        // The strip may be cleaned up first, taking the list with it.
        let tabs = self.tabs;
        on_cleanup_local(move || tabs.update_value(|tabs| tabs.retain(|tab| tab.serial != serial)));
    }

    /// The known tabs in the order they are laid out along the strip.
    fn tabs_in_order(self) -> Vec<KnownTab> {
        let mut placed: Vec<(f32, KnownTab)> = self
            .tabs
            .get_value()
            .into_iter()
            .map(|tab| {
                // A tab not laid out yet has no place along the strip and goes after the rest.
                let left = tab.trigger.window_bounds().map_or(f32::INFINITY, |bounds| bounds.origin.x.0);
                (left, tab)
            })
            .collect();
        placed.sort_by(|(a, _), (b, _)| a.total_cmp(b));

        placed.into_iter().map(|(_, tab)| tab).collect()
    }

    /// Names the tab called `value` as the one the context menu is for, as the pointer asks for
    /// it.
    fn claim_menu(self, value: &str) {
        self.menu_target.set(Some(value.to_owned()));
        self.menu_claimed.set_value(true);
    }

    /// Whether a tab claimed the menu being asked for, forgetting the claim.
    fn take_claim(self) -> bool {
        self.menu_claimed.try_update_value(std::mem::take) == Some(true)
    }
}

/// One tab as its strip knows it.
#[derive(Clone)]
struct KnownTab {
    /// Tells apart two tabs built in turn under one value.
    serial: TabSerial,
    value: String,
    trigger: NodeRef,
    on_close: Option<UnsyncCallback<()>>,
}

/// Which registration with its strip a tab is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TabSerial(u64);

/// What a [`TabStrip`] shows under its tabs: a `TabsContent` for each tab whose panel it switches
/// to.
#[slot]
pub struct TabStripPanels {
    children: Children,
}

/// Controls at the end of a [`TabStrip`]'s row, after its tabs, such as a menu button.
#[slot]
pub struct TabStripEnd {
    children: Children,
}

/// The words a [`TabStrip`]'s context menu shows.
#[derive(Clone, Copy, Debug)]
pub struct TabStripStrings {
    pub close: Signal<String, LocalStorage>,
    pub close_others: Signal<String, LocalStorage>,
    pub close_to_the_right: Signal<String, LocalStorage>,
}

impl TabStripStrings {
    /// The words in English.
    pub fn new() -> Self {
        Self {
            close: "Close".into(),
            close_others: "Close others".into(),
            close_to_the_right: "Close to the right".into(),
        }
    }
}

impl Default for TabStripStrings {
    fn default() -> Self {
        Self::new()
    }
}

/// A row of tabs with one accent line under the selected tab.
///
/// When the selection changes the line moves from where it is to the tab chosen; when the
/// selected tab is laid out elsewhere, by a resize or a tab before it closing, the line goes
/// with it at once. The strip scrolls sideways when its tabs do not fit, and the line with it.
///
/// One tab stop for the whole strip. The arrow keys move between tabs and select them,
/// <kbd>Home</kbd> and <kbd>End</kbd> go to the ends, and <kbd>Delete</kbd> closes a tab that can
/// be closed.
///
/// A tab that can be closed always shows its close button, and closes on a middle click. Its
/// context menu, from the secondary button or the menu key, closes it, every other tab, or the
/// tabs after it, skipping the tabs that cannot be closed.
#[allow(clippy::too_many_arguments, reason = "each prop is a named field of the props builder")]
#[component(slot_aware)]
pub fn TabStrip(
    /// The selected tab's value, written when a tab is chosen.
    #[prop(into)]
    value: Binding<String>,
    /// What the strip is called, for a screen reader.
    #[prop(into)]
    label: Signal<String, LocalStorage>,
    /// Told the values of the tabs the context menu closes together, in strip order, to close
    /// them as one change. Without it each of those tabs is closed in turn.
    #[prop(optional)]
    on_close_tabs: Option<UnsyncCallback<Vec<String>>>,
    /// The context menu's words.
    #[prop(optional)]
    strings: Option<TabStripStrings>,
    /// The panels under the tabs.
    #[prop(optional)]
    tab_strip_panels: Option<TabStripPanels>,
    /// Controls after the tabs, outside the area that scrolls and the tabs' context menu.
    #[prop(optional)]
    tab_strip_end: Option<TabStripEnd>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    /// The [`TabStripTab`]s.
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let selected = RwSignal::new_local(None::<SelectedTab>);
    let strip = StripContext {
        selected,
        tabs: StoredValue::new_local(Vec::new()),
        serials: StoredValue::new_local(TabSerial(0)),
        menu_target: RwSignal::new_local(None),
        menu_claimed: StoredValue::new_local(false),
    };
    provide_local_context(strip);

    let track = NodeRef::new();
    let track_box = track.observe_border_box();
    let target = move || {
        let tab = selected.get()?;
        let strip = track_box.get()?;
        let scale = track.scale();
        let scale = if scale > 0.0 { scale } else { 1.0 };

        Some(Placed { span: Span::under(tab.bounds, strip, scale), value: tab.value })
    };

    let span = RwSignal::new_local(None::<Span>);
    let gliding = RwSignal::new_local(false);
    let placed: Rc<RefCell<Option<Placed>>> = Rc::new(RefCell::new(None));
    let placing = RenderEffect::new(move |_| {
        // Until the newly selected tab has measured itself, the line stays where it is.
        let Some(target) = target() else {
            return;
        };

        let mut placed = placed.borrow_mut();
        if placed.as_ref() == Some(&target) {
            return;
        }

        // A change of tab moves the line; the same tab laid out elsewhere takes it at once.
        let changed_tab = placed.as_ref().is_some_and(|placed| placed.value != target.value);
        gliding.set(changed_tab);
        span.set(Some(target.span));
        *placed = Some(target);
    });
    on_cleanup_local(move || drop(placing));

    let on_menu_press = move |ev: &mut EventCx<'_, events::PointerDown>| {
        if ev.button == Some(PointerButton::Secondary) && !strip.take_claim() {
            ev.stop_propagation();
        }
    };
    let on_menu_request = move |ev: &mut EventCx<'_, events::ContextMenu>| {
        if !strip.take_claim() {
            ev.stop_propagation();
        }
    };
    // A strip given no words of its own labels its menu in English.
    let strings = strings.unwrap_or_default();
    let menu = move || tab_menu(strip, strings, on_close_tabs);

    let class = Classes::from("lui-tab-strip").merged(class);
    let panels = tab_strip_panels.map(|panels| panels.children.into_view_once());

    let tabs = view! {
        ContextMenu {
            ContextMenuTrigger(class = "lui-tab-strip__menu-area") {
                box(
                    class = "lui-tab-strip__scroller",
                    on:pointer_down = on_menu_press,
                    on:context_menu = on_menu_request
                ) {
                    row(class = "lui-tab-strip__track", node_ref = track) {
                        TabsList(class = "lui-tab-strip__list") {
                            {children.into_view_once()}
                        }
                        box(
                            class = "lui-tab-strip__underline",
                            a11y:hidden = true,
                            attr:data-gliding = move || gliding.get().then(|| "true".to_owned()),
                            style:display = move || span.get().is_none().then(|| "none".to_owned()),
                            style:left = move || span.get().map(|span| format!("{}px", span.left)),
                            style:width = move || span.get().map(|span| format!("{}px", span.width))
                        )
                    }
                }
            }
            ContextMenuContent {
                {menu}
            }
        }
    };
    let bar = match tab_strip_end {
        Some(end) => AnyView::new(view! {
            row(class = "lui-tab-strip__bar") {
                {tabs}
                row(class = "lui-tab-strip__end") {
                    {end.children.into_view_once()}
                }
            }
        }),
        None => AnyView::new(tabs),
    };

    view! {
        Tabs(value = value, a11y:label = move || label.get(), class = class, {..attrs}) {
            {bar}
            {panels}
        }
    }
}

/// One tab of a [`TabStrip`]: an optional icon, its label, and a close button when it can be
/// closed.
#[component]
pub fn TabStripTab(
    /// What selecting this tab writes to the strip's value.
    #[prop(into)]
    value: String,
    #[prop(optional)] icon: Option<IconData>,
    /// Run when the tab is closed: by its close button, a middle click, <kbd>Delete</kbd> or its
    /// context menu. Without one, the tab cannot be closed and has no close button.
    #[prop(optional)]
    on_close: Option<UnsyncCallback<()>>,
    /// What the close button is called, for a screen reader.
    #[prop(into, default = "Close tab".into())]
    close_label: Signal<String, LocalStorage>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let tabs = TabsContext::current();
    let strip = use_local_context::<StripContext>();
    let place = ContextMenuPlace::current();
    if let (Some(tabs), Some(strip)) = (tabs, strip) {
        report_bounds(tabs, strip, value.clone());
        strip.register(value.clone(), tabs.trigger_of(&value), on_close);
    }

    let mark = icon.map(|icon| AnyView::new(view! { Icon(icon = icon, size = IconSize::Sm) }));
    let close = on_close.map(|on_close| {
        let semantics = A11yBinding::new(Role::Button).label(move || close_label.get());
        AnyView::new(view! {
            control(
                class = "lui-tab-strip__close",
                tabindex = {Focus::Programmatic},
                // A press on the button is not a choice of its tab.
                on:focus_in = |ev| ev.stop_propagation(),
                on:click = move |ev| {
                    ev.stop_propagation();
                    on_close.run(());
                },
                {..Attrs::new().a11y_from(semantics)}
            ) {
                Icon(icon = CROSS, size = IconSize::Sm)
            }
        })
    });

    // Whether the middle button went down on this tab, so only a click that starts and ends on
    // it closes it.
    let middle_pressed = Rc::new(Cell::new(false));
    let on_pointer_down = {
        let value = value.clone();
        let middle_pressed = Rc::clone(&middle_pressed);
        move |ev: &mut EventCx<'_, events::PointerDown>| match ev.button {
            Some(PointerButton::Middle) if on_close.is_some() => {
                // Neither a focus nor a choice of the tab.
                ev.prevent_default();
                middle_pressed.set(true);
            }
            Some(PointerButton::Secondary) => {
                if let Some(strip) = strip {
                    strip.claim_menu(&value);
                }
            }
            _ => {}
        }
    };
    // A release away from the tab is not the end of a click on it.
    let on_pointer_leave = {
        let middle_pressed = Rc::clone(&middle_pressed);
        move |_: &mut EventCx<'_, events::PointerLeave>| middle_pressed.set(false)
    };
    let on_pointer_up = move |ev: &mut EventCx<'_, events::PointerUp>| {
        if ev.button != Some(PointerButton::Middle) || !middle_pressed.replace(false) {
            return;
        }
        if let Some(on_close) = on_close {
            ev.stop_propagation();
            on_close.run(());
        }
    };
    let on_context_menu = {
        let value = value.clone();
        move |_: &mut EventCx<'_, events::ContextMenu>| {
            if let Some(strip) = strip {
                strip.claim_menu(&value);
            }
        }
    };
    let on_key_down = {
        let value = value.clone();
        move |ev: &mut EventCx<'_, events::KeyDown>| {
            if opens_menu(&ev.key, ev.modifiers) {
                let (Some(strip), Some(place), Some(tabs)) = (strip, place, tabs) else {
                    return;
                };
                let trigger = tabs.trigger_of(&value);
                let Some(bounds) = trigger.window_bounds() else {
                    return;
                };

                ev.prevent_default();
                ev.stop_propagation();
                strip.menu_target.set(Some(value.clone()));
                let scale = trigger.scale();
                let scale = if scale > 0.0 { scale } else { 1.0 };
                place.open_at(Point::new(
                    CssPx(bounds.origin.x.0 / scale),
                    CssPx((bounds.origin.y.0 + bounds.size.height.0) / scale),
                ));
                return;
            }

            if ev.key != Key::Named(NamedKey::Delete) {
                return;
            }
            let Some(on_close) = on_close else {
                return;
            };

            on_close.run(());
            ev.prevent_default();
            ev.stop_propagation();
        }
    };
    let class = Classes::from("lui-tab-strip__tab").merged(class);

    view! {
        TabsTrigger(
            value = value,
            class = class,
            on:pointer_down = on_pointer_down,
            on:pointer_up = on_pointer_up,
            on:pointer_leave = on_pointer_leave,
            on:context_menu = on_context_menu,
            on:key_down = on_key_down,
            {..attrs}
        ) {
            {mark}
            box(class = "lui-tab-strip__label") {{children.into_view_once()}}
            {close}
        }
    }
}

/// The context menu for the tab it was last asked for over: close it, close the other tabs, or
/// close the tabs after it. An item with no tab it can close is disabled.
fn tab_menu(
    strip: StripContext,
    strings: TabStripStrings,
    on_close_tabs: Option<UnsyncCallback<Vec<String>>>,
) -> Option<AnyView> {
    let target = strip.menu_target.get()?;
    let tabs = strip.tabs_in_order();
    let index = tabs.iter().position(|tab| tab.value == target)?;

    let this = tabs[index].on_close;
    let others: Vec<KnownTab> = tabs
        .iter()
        .enumerate()
        .filter(|(at, tab)| *at != index && tab.on_close.is_some())
        .map(|(_, tab)| tab.clone())
        .collect();
    let after: Vec<KnownTab> = tabs[index + 1..].iter().filter(|tab| tab.on_close.is_some()).cloned().collect();

    let none_others = others.is_empty();
    let none_after = after.is_empty();
    let close_this = UnsyncCallback::new(move |()| {
        if let Some(on_close) = this {
            // The tab may have been taken down while its menu was open.
            on_close.try_run(());
        }
    });
    let close_others = close_together(others, on_close_tabs);
    let close_after = close_together(after, on_close_tabs);

    Some(AnyView::new(view! {
        MenuItem(disabled = this.is_none(), on_select = close_this) {{move || strings.close.get()}}
        MenuItem(disabled = none_others, on_select = close_others) {{move || strings.close_others.get()}}
        MenuItem(disabled = none_after, on_select = close_after) {{move || strings.close_to_the_right.get()}}
    }))
}

/// Closes `tabs` as one change through `on_close_tabs`, or without it each in turn.
fn close_together(tabs: Vec<KnownTab>, on_close_tabs: Option<UnsyncCallback<Vec<String>>>) -> UnsyncCallback<()> {
    UnsyncCallback::new(move |()| {
        if let Some(on_close_tabs) = on_close_tabs {
            on_close_tabs.run(tabs.iter().map(|tab| tab.value.clone()).collect());
            return;
        }

        for on_close in tabs.iter().filter_map(|tab| tab.on_close) {
            // Closing one tab may have taken a later one down with it.
            on_close.try_run(());
        }
    })
}

/// Whether `key` with `modifiers` asks for a context menu from the keyboard.
fn opens_menu(key: &Key, modifiers: Modifiers) -> bool {
    match key {
        Key::Named(NamedKey::ContextMenu) => true,
        Key::Named(NamedKey::F10) => modifiers.shift(),
        _ => false,
    }
}

/// Reports where the tab called `value` is laid out to its strip while it is the selected one,
/// and brings it into view once it is laid out after becoming selected, the first selection too.
fn report_bounds(tabs: TabsContext, strip: StripContext, value: String) {
    let trigger = tabs.trigger_of(&value);
    let name = value.clone();
    let observed = trigger.observe_border_box_while(move || tabs.is_selected(&name));

    // Whether this selection of the tab has been scrolled into view.
    let reporting = RenderEffect::new(move |scrolled: Option<bool>| {
        if !tabs.is_selected(&value) {
            return false;
        }

        let Some(bounds) = observed.get() else {
            return false;
        };

        if scrolled != Some(true) {
            trigger.scroll_to(ScrollTarget::IntoView, ScrollBehavior::Instant);
        }
        strip.selected.set(Some(SelectedTab { value: value.clone(), bounds }));

        true
    });
    on_cleanup_local(move || drop(reporting));
}
