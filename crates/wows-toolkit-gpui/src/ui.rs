//! Small rendering helpers shared across the tabs.

use std::sync::Arc;
use std::sync::LazyLock;

use gpui_kit::base::SelectableText;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::IconName;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants;
use gpui_kit::component::h_flex;
use gpui_kit::component::list::ListItem;
use gpui_kit::*;

pub fn toolbar(cx: &App) -> Div {
    h_flex()
        .flex_none()
        .min_w_0()
        .gap_2()
        .items_center()
        .px_2()
        .py_1()
        .bg(crate::theme::surface())
        .border_b_1()
        .border_color(cx.theme().border)
}

pub fn workspace(content: impl IntoElement, cx: &App) -> Div {
    div().size_full().min_w_0().min_h_0().p_1().bg(crate::theme::surface()).child(
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .overflow_hidden()
            .child(content),
    )
}

/// The background for row `index` of a list or table, or `None` for the rows
/// that keep the panel's own.
///
/// Every other row is lifted, starting with the first, which is what egui
/// stripes (`egui_extras::Table`, `row_index.is_multiple_of(2)`) at the
/// strength egui lifts them by (see `theme::Palette::striped`). A long table
/// of numbers is read across a row, and the lift is what keeps an eye on one.
pub fn stripe(index: usize, cx: &App) -> Option<Hsla> {
    index.is_multiple_of(2).then(|| cx.theme().table_even)
}

/// Draws the indent guides for a tree row at `depth`, so a deep row reads as
/// belonging to the group above it rather than floating at an arbitrary
/// offset.
///
/// One hairline per level, at the left edge of the space that level indents
/// by, matching the connector lines `egui_ltreeview` draws.
pub fn indent_guides(depth: usize, indent: Pixels, cx: &App) -> impl IntoElement {
    let color = cx.theme().border.opacity(0.7);
    h_flex().flex_none().children((0..depth).map(move |_| {
        // The line sits at the right edge of each level's indent, which is
        // where the next level's rows begin.
        div().w(indent).h_full().flex_none().border_r_1().border_color(color)
    }))
}

/// The metrics every tree row is drawn at.
///
/// `ListItem` renders at `text_base` (a full rem, 16px) inside its own
/// `py_1 px_3`, while the app's body text is `theme.font_size` (12.5px). A
/// tree left at those defaults draws a quarter larger than the panel around
/// it and a third taller than it needs to be, which is what made the
/// listings look padded out. The component applies its own style last, so
/// setting these on the item wins.
pub fn tree_row(item: ListItem, cx: &App) -> ListItem {
    item.text_size(cx.theme().font_size).py_0().px_1()
}

/// Wraps a control that shows a selected state so the selection is announced.
///
/// `gpui-component`'s `Button::selected` is presentation only -- it styles the
/// button and deliberately sets no aria state -- so a row of selectable
/// buttons reads to assistive technology, and to the UI tests, as a row of
/// plain buttons with no indication of which one is active. Wrapping them
/// carries that state without giving up the button's look.
pub fn selectable(id: impl Into<ElementId>, selected: bool, control: impl IntoElement) -> impl IntoElement {
    div().id(id).test_support().aria_selected(selected).rounded(px(2.)).child(control)
}

/// One run of text the reader can select and copy with the pointer.
///
/// Plain `div().child(text)` paints glyphs the pointer cannot touch. Anything
/// a reader would reasonably lift out of the window -- a name, a figure, a
/// line of JSON -- goes through this instead. `Root` already mounts the
/// selection layer the runs register with (`main.rs`).
pub fn selectable_text(id: impl Into<ElementId>, text: impl Into<SharedString>) -> SelectableText {
    SelectableText::new(id, text)
}

/// Sets `element`'s figures to one width, for numbers read down a column.
pub fn tabular<E: Styled>(element: E) -> E {
    element.font_features(tabular_figures())
}

/// Figures of one width, so numbers in a column line up digit for digit.
pub fn tabular_figures() -> FontFeatures {
    static TABULAR: LazyLock<FontFeatures> = LazyLock::new(|| FontFeatures(Arc::new(vec![("tnum".into(), 1)])));

    TABULAR.clone()
}

/// The height a `Select` occupies in a row, by its size.
///
/// `Select` puts its trigger inside a `size_full` root, so in a row taller
/// than the trigger the trigger top-anchors rather than centring. Boxing it
/// at its own height is what makes a combo line up with the buttons beside
/// it. Small is `input_h(Small)` = 24px; the default is 32px.
pub const SELECT_SMALL_HEIGHT: Pixels = px(24.);

/// Boxes a control at a definite size so a row can centre it.
pub fn boxed(width: Pixels, height: Pixels) -> Div {
    div().flex_none().w(width).h(height)
}

/// A vertical rule between two groups of controls on one row.
///
/// The egui toolbars separate their groups with `ui.separator()`; without it
/// a row of buttons reads as one undifferentiated strip.
pub fn rule_v(cx: &App) -> impl IntoElement {
    div().flex_none().w(px(1.)).h(px(16.)).mx_1().bg(cx.theme().border)
}

/// A horizontal rule between two groups of controls in one column.
pub fn rule_h(cx: &App) -> impl IntoElement {
    div().flex_none().h(px(1.)).w_full().my_1().bg(cx.theme().border)
}

/// The title over a group of rows: small, semibold and muted, so it names the
/// group without competing with the rows. `content` is the translated text as
/// drawn, or an element such as selectable text that carries it; the platform
/// UI font need not have small capitals, and GPUI has no letter spacing.
pub fn section_label(content: impl IntoElement) -> Div {
    div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(crate::theme::text_dim()).child(content)
}

/// The card level: one surface step above the panel, edged by a 1px rule.
pub fn card_surface<E: Styled>(element: E, cx: &App) -> E {
    let element =
        element.bg(crate::theme::card()).border_1().border_color(cx.theme().border).rounded(cx.theme().radius_lg);
    match crate::theme::card_highlight() {
        Some(highlight) => element.shadow(vec![highlight]),
        None => element,
    }
}

/// The overlay level, for a popover or tooltip the app builds itself: the
/// popover surface, a 1px rule and a small shadow.
pub fn overlay_surface<E: Styled>(element: E, cx: &App) -> E {
    element
        .bg(cx.theme().popover)
        .text_color(cx.theme().popover_foreground)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg)
        .shadow(crate::theme::overlay_shadow())
}

/// The edge of the window a collapsible side panel is docked against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelSide {
    Left,
    Right,
    Bottom,
}

impl PanelSide {
    /// Element id fragment, so the toggles for one area's docks do not collide.
    pub(crate) fn id_fragment(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Bottom => "bottom",
        }
    }
}

/// The button in a side panel's header that collapses the panel.
///
/// Pair with `panel_expand_rail` under the same `side`: the rail puts the
/// expand control where this button was, so collapsing does not move the
/// control under the pointer.
pub fn panel_collapse_button(
    id: impl Into<ElementId>,
    side: PanelSide,
    tooltip: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Button {
    // The bundled icon set has no panel-bottom-close; the plain bottom panel
    // glyph is the one the dock skin collapses a bottom dock with.
    let icon = match side {
        PanelSide::Left => IconName::PanelLeftClose,
        PanelSide::Right => IconName::PanelRightClose,
        PanelSide::Bottom => IconName::PanelBottom,
    };
    Button::new(id).icon(icon).ghost().small().tooltip(tooltip).on_click(on_click)
}

/// The narrow column a collapsed side panel leaves behind, holding the
/// control that expands it again.
pub fn panel_expand_rail(
    id: impl Into<ElementId>,
    side: PanelSide,
    tooltip: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let icon = match side {
        PanelSide::Left => IconName::PanelLeftOpen,
        PanelSide::Right => IconName::PanelRightOpen,
        PanelSide::Bottom => IconName::PanelBottomOpen,
    };
    div()
        .flex_none()
        .px_2()
        .py_1()
        .child(Button::new(id).icon(icon).ghost().small().tooltip(tooltip).on_click(on_click))
}
