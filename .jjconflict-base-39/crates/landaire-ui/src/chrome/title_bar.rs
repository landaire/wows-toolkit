//! A title bar for a window whose platform draws no frame.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use zgui::component;
use zgui::css;
use zgui::elements::Focus;
use zgui::geom::Css;
use zgui::geom::CssPx;
use zgui::geom::Point;
use zgui::platform::DecorationSource;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::slot;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::ChildrenFn;
use zgui::view::Classes;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::events;
use zgui::view::install_stylesheet;
use zgui::vocab::PointerButton;
use zgui::vocab::Role;
use zgui::vocab::Timestamp;
use zgui_ui_icons::IconData;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;

use crate::chrome::frame_window::FrameWindow;
use crate::chrome::frame_window::or_current;
use crate::tooltip::TooltipContentProps;
use crate::tooltip::TooltipProps;
use crate::tooltip::TooltipTriggerProps;

/// What the title bar rules are installed under.
const SHEET: &str = "landaire-ui-title-bar";

// A `WindowFrame` sets `--lui-window-grip` to the width of its resize grips while they are shown.
// The bar keeps its content that far from the window's edges, so a press on a button or slot is
// never taken for a resize.
const CSS: &str = css!(
    ".lui-title-bar {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: none;
        gap: 4px;
        height: calc(34px + var(--lui-window-grip, 0px));
        padding: var(--lui-window-grip, 0px) calc(4px + var(--lui-window-grip, 0px)) 0;
        box-sizing: border-box;
        background-color: var(--lui-color-panel);
        border-bottom: 1px solid var(--lui-color-border);
        color: var(--lui-color-text);
    }
    .lui-title-bar__title {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 1 1 auto;
        min-width: 0;
        height: 100%;
        padding: 0 8px;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
        font-weight: 600;
    }
    .lui-title-bar__slot {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: 0 1 auto;
        min-width: 0;
        height: 100%;
        overflow: hidden;
    }
    .lui-title-bar__buttons {
        display: flex;
        flex-direction: row;
        align-items: center;
        flex: none;
        gap: 2px;
    }
    .lui-title-bar__button {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 32px;
        height: 26px;
        border-radius: var(--lui-radius-lg);
        color: var(--lui-color-text-dim);
        transition: background-color var(--lui-motion-fade) var(--lui-motion-ease),
            color var(--lui-motion-fade) var(--lui-motion-ease);
    }
    .lui-title-bar__button:hover {
        background-color: var(--lui-color-widget-hot);
        color: var(--lui-color-text-bright);
    }
    .lui-title-bar__button:focus-visible {
        outline: 1px solid var(--lui-color-accent);
        outline-offset: -2px;
    }
    .lui-title-bar__button[data-kind=\"close\"]:hover {
        background-color: var(--zui-color-destructive);
        color: var(--zui-color-destructive-foreground);
    }"
);

/// A bar along the bottom edge, for minimising.
const MINIMIZE: IconData = IconData::new("window-minimize", 24.0, "M6 11.25 L18 11.25 L18 12.75 L6 12.75 Z");

/// An outlined square, for maximising.
const MAXIMIZE: IconData = IconData::new(
    "window-maximize",
    24.0,
    "M6 6 L18 6 L18 18 L6 18 Z \
     M7.5 7.5 L7.5 16.5 L16.5 16.5 L16.5 7.5 Z",
);

/// One outlined square in front of another, for restoring a maximised window.
const RESTORE: IconData = IconData::new(
    "window-restore",
    24.0,
    "M6 9 L15 9 L15 18 L6 18 Z \
     M7.5 10.5 L7.5 16.5 L13.5 16.5 L13.5 10.5 Z \
     M9 6 L18 6 L18 15 L15 15 L15 13.5 L16.5 13.5 L16.5 7.5 L10.5 7.5 L10.5 9 L9 9 Z",
);

/// A thin cross, for closing.
const CLOSE: IconData = IconData::new(
    "window-close",
    24.0,
    "M18.03 7.03 L16.97 5.97 L12 10.94 L7.03 5.97 L5.97 7.03 L10.94 12 L5.97 16.97 L7.03 18.03 \
     L12 13.06 L16.97 18.03 L18.03 16.97 L13.06 12 Z",
);

/// Content at a [`TitleBar`]'s start, after the window buttons when they sit there, such as a
/// tab strip. Presses on it do not move the window.
#[slot]
pub struct TitleBarLeading {
    children: ChildrenFn,
}

/// Content at a [`TitleBar`]'s end, before the window buttons when they sit there. Presses on it
/// do not move the window.
#[slot]
pub struct TitleBarTrailing {
    children: ChildrenFn,
}

/// Which end of a [`TitleBar`] the window buttons sit at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleButtonSide {
    /// Close, minimise, maximise from the start, as macOS places them.
    Leading,
    /// Minimise, maximise, close to the end, as Windows and most Linux desktops place them.
    Trailing,
}

impl TitleButtonSide {
    /// Where this platform's own title bars put them.
    pub const PLATFORM: Self = if cfg!(target_os = "macos") { Self::Leading } else { Self::Trailing };

    const fn name(self) -> &'static str {
        match self {
            Self::Leading => "leading",
            Self::Trailing => "trailing",
        }
    }
}

/// The names a [`TitleBar`]'s buttons are read out and shown as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleBarStrings {
    pub minimize: String,
    pub maximize: String,
    pub restore: String,
    pub close: String,
}

impl TitleBarStrings {
    pub fn new() -> Self {
        Self {
            minimize: "Minimize".to_owned(),
            maximize: "Maximize".to_owned(),
            restore: "Restore".to_owned(),
            close: "Close".to_owned(),
        }
    }
}

impl Default for TitleBarStrings {
    fn default() -> Self {
        Self::new()
    }
}

/// One of the three window buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

impl WindowButton {
    const fn name(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
            Self::Close => "close",
        }
    }
}

/// Where and when a primary press on the bar landed.
#[derive(Clone, Copy, Debug)]
struct Press {
    at: Timestamp,
    position: Point<CssPx, Css>,
}

/// The bar across the top of a window whose platform draws no frame, such as one on a Wayland
/// desktop without server-side decorations. Elsewhere it renders nothing.
///
/// A press on the bar outside its controls and slots moves the window, and a second press soon
/// after at the same place does what the desktop does for a double click on a title bar. The
/// minimise, maximise and close buttons are always shown.
#[allow(clippy::too_many_arguments, reason = "each prop is a named field of the props builder")]
#[component(slot_aware)]
pub fn TitleBar(
    /// The window it reads and acts on. Left out, the window it is in.
    #[prop(optional)]
    window: Option<Rc<dyn FrameWindow>>,
    /// The buttons' names. Left out, English.
    #[prop(optional)]
    strings: Option<TitleBarStrings>,
    #[prop(default = TitleButtonSide::PLATFORM)] buttons: TitleButtonSide,
    #[prop(optional)] title_bar_leading: Option<TitleBarLeading>,
    #[prop(optional)] title_bar_trailing: Option<TitleBarTrailing>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    /// The title.
    children: ChildrenFn,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let window = or_current(window);
    let source = window.decoration_source();
    let fullscreen = window.fullscreen();
    let window = StoredValue::new_local(window);
    let strings = StoredValue::new_local(strings.unwrap_or_default());
    let leading = StoredValue::new_local(title_bar_leading.map(|slot| slot.children));
    let trailing = StoredValue::new_local(title_bar_trailing.map(|slot| slot.children));
    let title = StoredValue::new_local(children);
    let attrs = StoredValue::new_local(attrs);
    let class = StoredValue::new_local(Classes::from("lui-title-bar").merged(class));

    let button_group = move |side: TitleButtonSide| {
        (side == buttons).then(|| window_buttons(window.get_value(), &strings.get_value(), side))
    };
    let slot = move |content: Option<ChildrenFn>| {
        content.map(|content| {
            AnyView::new(view! {
                row(class = "lui-title-bar__slot", on:pointer_down = |ev| ev.stop_propagation()) {
                    {content.view()}
                }
            })
        })
    };

    view! {
        // A fullscreen window gives its whole surface to the content, as the frame does.
        if move || source.get() == DecorationSource::Application && fullscreen.get().is_none() {
            row(
                attr:data-buttons = buttons.name(),
                on:pointer_down = move_or_double_click(window.get_value()),
                {..attrs.get_value()},
                class = class.get_value()
            ) {
                {button_group(TitleButtonSide::Leading)}
                {slot(leading.get_value())}
                box(class = "lui-title-bar__title") {
                    {title.get_value().view()}
                }
                {slot(trailing.get_value())}
                {button_group(TitleButtonSide::Trailing)}
            }
        } else {}
    }
}

/// The bar's own press handler: a primary press moves the window, and a second one soon after at
/// the same place is a double click on the title bar.
///
/// Told apart press to press, because once the desktop moves the window it holds the pointer and
/// no release arrives to form a click.
fn move_or_double_click(window: Rc<dyn FrameWindow>) -> impl Fn(&mut EventCx<'_, events::PointerDown>) {
    // The desktop convention: no platform reports its own interval or distance.
    const DOUBLE_PRESS: Duration = Duration::from_millis(400);
    const SLOP: f32 = 4.0;

    let last = Cell::new(None::<Press>);

    move |ev: &mut EventCx<'_, events::PointerDown>| {
        if ev.button != Some(PointerButton::Primary) || !ev.primary {
            return;
        }

        ev.stop_propagation();

        let press = Press { at: ev.timestamp, position: ev.position };
        let again = last.get().is_some_and(|previous| {
            press.at.saturating_since(previous.at) <= DOUBLE_PRESS
                && (press.position.x.0 - previous.position.x.0).abs() <= SLOP
                && (press.position.y.0 - previous.position.y.0).abs() <= SLOP
        });
        if again {
            // A third press starts a new gesture.
            last.set(None);
            window.title_bar_double_click();
        } else {
            last.set(Some(press));
            window.begin_move_drag();
        }
    }
}

/// The three window buttons in `side`'s order, close outermost.
fn window_buttons(window: Rc<dyn FrameWindow>, strings: &TitleBarStrings, side: TitleButtonSide) -> AnyView {
    let order = match side {
        TitleButtonSide::Leading => [WindowButton::Close, WindowButton::Minimize, WindowButton::Maximize],
        TitleButtonSide::Trailing => [WindowButton::Minimize, WindowButton::Maximize, WindowButton::Close],
    };
    let buttons: Vec<AnyView> =
        order.into_iter().map(|kind| window_button(kind, Rc::clone(&window), strings)).collect();

    AnyView::new(view! {
        row(class = "lui-title-bar__buttons") {{buttons}}
    })
}

fn window_button(kind: WindowButton, window: Rc<dyn FrameWindow>, strings: &TitleBarStrings) -> AnyView {
    let maximized = window.maximized();
    let label = match kind {
        WindowButton::Minimize => Signal::stored_local(strings.minimize.clone()),
        WindowButton::Close => Signal::stored_local(strings.close.clone()),
        WindowButton::Maximize => {
            let maximize = strings.maximize.clone();
            let restore = strings.restore.clone();
            Signal::derive_local(move || if maximized.get() { restore.clone() } else { maximize.clone() })
        }
    };
    let icon = match kind {
        WindowButton::Minimize => Signal::stored_local(MINIMIZE),
        WindowButton::Close => Signal::stored_local(CLOSE),
        WindowButton::Maximize => Signal::derive_local(move || if maximized.get() { RESTORE } else { MAXIMIZE }),
    };
    let act = move || match kind {
        WindowButton::Minimize => window.minimize(),
        WindowButton::Maximize => window.toggle_maximized(),
        WindowButton::Close => window.close(),
    };
    let semantics = A11yBinding::new(Role::Button).label(move || label.get());

    AnyView::new(view! {
        Tooltip {
            TooltipTrigger {
                control(
                    class = "lui-title-bar__button",
                    attr:data-kind = kind.name(),
                    tabindex = {Focus::Sequential},
                    // A press here is not the start of a move.
                    on:pointer_down = |ev| ev.stop_propagation(),
                    on:click = move |_| act(),
                    {..Attrs::new().a11y_from(semantics)}
                ) {
                    Icon(icon = icon, size = IconSize::Sm)
                }
            }
            TooltipContent {{move || label.get()}}
        }
    })
}
