//! Drawing an icon from `wows_toolkit_icons`.

use wows_toolkit_icons::Glyph;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::A11yBinding;
use zgui::view::Attrs;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui::vocab::PropValue;
use zgui::vocab::Role;

const SHEET: &str = "wt-icon";

/// An icon is a box in a line of text: one em square, so it follows the text size around it.
const CSS: &str = css!(
    ".wt-icon {
        display: inline-block;
        flex: none;
        width: 1.15em;
        height: 1.15em;
    }"
);

/// One icon, sized to and coloured like the surrounding text.
///
/// Hidden from a screen reader unless `label` names it, since an icon beside a word repeats it.
#[component]
pub fn AppIcon(
    #[prop(into)] glyph: Signal<Glyph, LocalStorage>,
    #[prop(into, optional)] label: Option<String>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let semantics = Attrs::new().a11y_from(match label {
        Some(label) => A11yBinding::new(Role::Image).label(label),
        None => A11yBinding::new(Role::Image).hidden(true),
    });

    view! {
        vector(
            class = "wt-icon",
            attr:data-icon = move || Some(glyph.get().name().to_owned()),
            prop:svg = move || PropValue::from(glyph.get().svg()),
            {..semantics},
            {..attrs},
            class = class
        )
    }
}
