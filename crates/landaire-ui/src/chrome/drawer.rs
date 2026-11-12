use std::cell::RefCell;
use std::rc::Rc;

use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::Attrs;
use zgui::view::ChildrenFn;
use zgui::view::Classes;
use zgui::view::FocusTrap;
use zgui::view::FocusTrapOptions;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::focused_node;
use zgui::view::install_stylesheet;
use zgui::vocab::Key;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

use crate::chrome::slide::PanelWidth;
use crate::chrome::slide::SideEdge;
use crate::chrome::slide::Slide;
use crate::motion::Movement;

const SHEET: &str = "landaire-ui-drawer";

const CSS: &str = css!(
    ".lui-drawer {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        z-index: 10;
        overflow: hidden;
    }
    .lui-drawer:not([data-shown]) {
        pointer-events: none;
    }
    .lui-drawer__scrim {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        background-color: var(--lui-color-scrim);
        opacity: 0;
        transition: opacity var(--lui-motion-slide) var(--lui-motion-ease);
    }
    .lui-drawer[data-shown] > .lui-drawer__scrim {
        opacity: 1;
    }
    .lui-drawer__panel {
        position: absolute;
        top: 0;
        bottom: 0;
        box-sizing: border-box;
        display: flex;
        flex-direction: column;
        width: var(--lui-drawer-width);
        max-width: 100%;
        min-height: 0;
        overflow: hidden;
        background-color: var(--lui-color-panel);
        color: var(--lui-color-text);
        box-shadow: var(--lui-shadow-overlay);
        transition: transform var(--lui-motion-slide) var(--lui-motion-ease);
    }
    .lui-drawer__panel:focus-visible {
        outline: none;
    }
    .lui-drawer[data-edge=\"right\"] > .lui-drawer__panel {
        right: 0;
        border-left: 1px solid var(--lui-color-border-bright);
        transform: translateX(100%);
    }
    .lui-drawer[data-edge=\"left\"] > .lui-drawer__panel {
        left: 0;
        border-right: 1px solid var(--lui-color-border-bright);
        transform: translateX(-100%);
    }
    .lui-drawer[data-shown] > .lui-drawer__panel {
        transform: none;
    }"
);

/// A panel that slides over its container from one edge, above a scrim that dims the rest.
///
/// The drawer covers the nearest positioned ancestor, so that ancestor decides what it dims: the
/// content area beneath a tab strip, say, rather than the whole window. A press on the scrim or
/// <kbd>Escape</kbd> closes it, and focus stays within it while it is open and goes back to
/// where it was when it closes. Its content is built each time it opens.
#[component]
pub fn Drawer(
    open: RwSignal<bool, LocalStorage>,
    /// What the drawer is called, for a screen reader.
    #[prop(into)]
    label: String,
    #[prop(default = PanelWidth(440.0))] width: PanelWidth,
    #[prop(default = SideEdge::Right)] edge: SideEdge,
    /// Classes merged after the panel's own.
    #[prop(into, optional)]
    class: Classes,
    /// Forwarded to the panel.
    #[prop(attrs)]
    attrs: Attrs,
    children: ChildrenFn,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let slide = Slide::follow(open.into(), Movement::Slide);
    let panel = NodeRef::new();
    let focused = focused_node();

    let held: Rc<RefCell<Option<FocusTrap>>> = Rc::new(RefCell::new(None));
    let trapping = {
        let held = Rc::clone(&held);
        RenderEffect::new(move |_| {
            let bound = panel.get().is_some();
            if !(open.get() && bound) {
                // Taken out of the cell before it is dropped: dropping a trap moves focus back,
                // and that can reach this effect again.
                let released = held.borrow_mut().take();
                drop(released);
                return;
            }

            if held.borrow().is_some() {
                return;
            }

            let trap = panel.trap_focus(FocusTrapOptions::MODAL);
            *held.borrow_mut() = trap;
            // Nothing in the content may take focus, and the panel itself answers Escape.
            if !focused.get_untracked().is_some_and(|node| panel.contains(node)) {
                panel.focus();
            }
        })
    };
    on_cleanup_local(move || {
        drop(trapping);
        let released = held.borrow_mut().take();
        drop(released);
    });

    let semantics = A11yBinding::new(Role::Dialog).label(label).modal(true);
    let own = StoredValue::new_local(Attrs::new().a11y_from(semantics));
    let attrs = StoredValue::new_local(attrs);
    let class = StoredValue::new_local(class);
    let children = StoredValue::new_local(children);

    view! {
        if move || slide.mounted.get() {
            box(
                class = "lui-drawer",
                attr:data-edge = edge.name(),
                attr:data-shown = move || slide.shown.get().then(|| "true".to_owned()),
                var:--lui-drawer-width = format!("{}px", width.0)
            ) {
                box(class = "lui-drawer__scrim", a11y:hidden = true, on:click = move |_| open.set(false))
                column(
                    class = "lui-drawer__panel",
                    node_ref = panel,
                    tabindex = {Focus::Programmatic},
                    on:key_down = move |ev| {
                        if ev.key != Key::Named(NamedKey::Escape) || ev.modifiers != Modifiers::NONE {
                            return;
                        }

                        open.set(false);
                        ev.prevent_default();
                        ev.stop_propagation();
                    },
                    {..own.get_value()},
                    {..attrs.get_value()},
                    class = class.get_value()
                ) {
                    {children.get_value().view()}
                }
            }
        } else {}
    }
}
