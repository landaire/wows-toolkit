use std::cell::Cell;
use std::rc::Rc;

use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::runtime::windows::Zoom;
use zgui::runtime::windows::try_use_window;
use zgui::view;
use zgui::view::Anchor;
use zgui::view::AttrName;
use zgui::view::BuildCx;
use zgui::view::Children;
use zgui::view::DomHandle;
use zgui::view::IntoView;
use zgui::view::NodeId;
use zgui::view::Stylesheet;
use zgui::view::View;
use zgui::view::install_stylesheet;
use zgui_ui_tokens::ColorScheme;
use zgui_ui_tokens::ThemeProviderProps;

use crate::motion;
use crate::motion::MotionPreference;
use crate::motion::REDUCED_MOTION_ATTRIBUTE;
use crate::scheme::ThemeChoice;
use crate::surface;
use crate::tokens;
use crate::tooltip;

/// What the `--lui-*` properties are installed under.
const SHEET: &str = "landaire-ui-theme";

/// Applies a theme choice, a motion preference, a zoom and the tooltip timing to the window it is
/// in.
///
/// One per window, around everything else: it declares its tokens on `:root`, so surfaces
/// portalled onto overlay bands are themed too.
#[component]
pub fn ThemeRoot(
    #[prop(into, default = Signal::stored_local(ThemeChoice::System))] choice: Signal<ThemeChoice, LocalStorage>,
    #[prop(into, default = Signal::stored_local(MotionPreference::Full))] motion: Signal<
        MotionPreference,
        LocalStorage,
    >,
    /// The window's zoom. Left out, the zoom is whatever the window was opened at.
    #[prop(into, optional)]
    zoom: Option<Signal<Zoom, LocalStorage>>,
    children: Children,
) -> impl IntoView {
    install_stylesheet(surface::SHEET, surface::CSS);
    motion::provide_motion_preference(motion);
    tooltip::install_tooltips();

    let sheet = Stylesheet::install(SHEET, &tokens::lui_sheet(choice.get_untracked()));
    let installed = RenderEffect::new(move |previous: Option<()>| {
        let css = tokens::lui_sheet(choice.get());
        // The first run is the install above.
        if previous.is_some()
            && let Some(sheet) = sheet.as_ref()
        {
            sheet.replace(&css);
        }
    });

    let zoomed =
        zoom.zip(try_use_window()).map(|(zoom, window)| RenderEffect::new(move |_| window.set_zoom(zoom.get())));

    on_cleanup_local(move || {
        drop(installed);
        drop(zoomed);
    });

    let light = Signal::derive_local(move || tokens::zui_theme(choice.get().slots().light));
    let dark = Signal::derive_local(move || tokens::zui_theme(choice.get().slots().dark));
    let scheme: Signal<ColorScheme, LocalStorage> = Signal::derive_local(move || choice.get().slots().scheme);
    let reduced = Signal::derive_local(move || motion.get() == MotionPreference::Reduced);

    (
        RootAttribute { name: AttrName::new(REDUCED_MOTION_ATTRIBUTE), on: reduced },
        view! {
            ThemeProvider(light = light, dark = dark, scheme = scheme) {
                {children.into_view_once()}
            }
        }
        .into_view(),
    )
}

/// Holds a valueless attribute on the window's root element while `on` is true.
///
/// A view's own elements are below the root, and an application's sheets select a document-wide
/// fact on the root, so the attribute is written there from the mount point.
struct RootAttribute {
    name: AttrName,
    on: Signal<bool, LocalStorage>,
}

struct RootAttributeState {
    marker: NodeId,
    name: AttrName,
    on: Signal<bool, LocalStorage>,
    root: Rc<Cell<Option<NodeId>>>,
    _following: RenderEffect<()>,
}

impl View for RootAttribute {
    type State = RootAttributeState;

    fn build(self, cx: &mut BuildCx<'_>) -> RootAttributeState {
        let Self { name, on } = self;
        let marker = cx.dom().create_marker();
        let root: Rc<Cell<Option<NodeId>>> = Rc::new(Cell::new(None));
        let following = cx.with_owner(|| {
            let dom = cx.dom_handle();
            let root = Rc::clone(&root);
            RenderEffect::new(move |_| {
                let on = on.get();
                if let Some(root) = root.get() {
                    write_flag(&dom, root, name, on);
                }
            })
        });

        RootAttributeState { marker, name, on, root, _following: following }
    }

    // The props are signals, so a rebuild has nothing new to apply.
    fn rebuild(self, _state: &mut RootAttributeState, _cx: &mut BuildCx<'_>) {}
}

impl Anchor for RootAttributeState {
    fn mount(&mut self, dom: &DomHandle, parent: NodeId, before: Option<NodeId>) {
        dom.insert(parent, self.marker, before);

        let root = dom.root(parent);
        self.root.set(Some(root));
        write_flag(dom, root, self.name, self.on.get_untracked());
    }

    fn unmount(&mut self, dom: &DomHandle) {
        if let Some(root) = self.root.take() {
            write_flag(dom, root, self.name, false);
        }

        dom.detach(self.marker);
    }

    fn first_node(&self) -> Option<NodeId> {
        Some(self.marker)
    }
}

fn write_flag(dom: &DomHandle, element: NodeId, name: AttrName, on: bool) {
    dom.set_attribute(element, name, on.then_some(""));
}
