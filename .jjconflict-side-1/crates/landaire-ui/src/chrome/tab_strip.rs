use std::cell::RefCell;
use std::rc::Rc;

use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Rect;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Children;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ScrollBehavior;
use zgui::view::ScrollTarget;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui_ui::TabsContext;
use zgui_ui::TabsListProps;
use zgui_ui::TabsProps;
use zgui_ui::TabsTriggerProps;
use zgui_ui_icons::IconData;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::mark::CROSS;

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
#[component]
pub fn TabStrip(
    /// The selected tab's value, written when a tab is chosen.
    value: RwSignal<String, LocalStorage>,
    /// What the strip is called, for a screen reader.
    #[prop(into)]
    label: String,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    /// The [`TabStripTab`]s.
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let selected = RwSignal::new_local(None::<SelectedTab>);
    provide_local_context(StripContext { selected });

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

    let class = Classes::from("lui-tab-strip").merged(class);

    view! {
        Tabs(value = value, label = label, class = class, {..attrs}) {
            box(class = "lui-tab-strip__scroller") {
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
    /// Run when the tab's close button is pressed or <kbd>Delete</kbd> is pressed on it. Without
    /// one, the tab has no close button.
    #[prop(optional)]
    on_close: Option<UnsyncCallback<()>>,
    /// What the close button is called, for a screen reader.
    #[prop(into, default = String::from("Close tab"))]
    close_label: String,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    if let Some(tabs) = TabsContext::current()
        && let Some(strip) = use_local_context::<StripContext>()
    {
        report_bounds(tabs, strip, value.clone());
    }

    let mark = icon.map(|icon| AnyView::new(view! { Icon(icon = icon, size = IconSize::Sm) }));
    let close = on_close.map(|on_close| {
        let semantics = A11yBinding::new(Role::Button).label(close_label);
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
    let deletes = StoredValue::new_local(on_close);
    let class = Classes::from("lui-tab-strip__tab").merged(class);

    view! {
        TabsTrigger(
            value = value,
            class = class,
            on:key_down = move |ev| {
                if ev.key != Key::Named(NamedKey::Delete) {
                    return;
                }
                let Some(on_close) = deletes.get_value() else {
                    return;
                };

                on_close.run(());
                ev.prevent_default();
                ev.stop_propagation();
            },
            {..attrs}
        ) {
            {mark}
            text {{children.into_view_once()}}
            {close}
        }
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
