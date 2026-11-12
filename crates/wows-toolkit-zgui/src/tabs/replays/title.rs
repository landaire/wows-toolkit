//! A replay tab's title, as the GPUI tab draws it: a spinner while the battle is read, then the
//! recording player's flag and ship class beside "{ship} - {map}".

use rust_i18n::t;
use wowsunpack::vfs::VfsPath;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui::vocab::PropValue;
use zgui_image::ImageBytes;
use zgui_ui::SpinnerProps;

use crate::game_art;
use crate::services::GameDataState;
use crate::services::Services;
use crate::tabs::replays::panel::PanelControl;
use crate::tabs::replays::panel::PanelTitle;
use crate::tabs::replays::panel::TitleShip;

const SHEET: &str = "wt-replay-title";

const CSS: &str = css!(
    ".wt-replay-title { display: flex; flex-direction: row; align-items: center; gap: 4px; min-width: 0; max-width: 300px; }
    .wt-replay-title__flag { flex: none; width: 23px; height: 16px; }
    .wt-replay-title__class { flex: none; width: 16px; height: 16px; }
    .wt-replay-title__text { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }"
);

/// One ship's flag and class icon, as the loaded build draws them.
struct ShipArt {
    ship: TitleShip,
    flag: Option<ImageBytes>,
    /// The white class icon, as SVG markup.
    class: Option<String>,
}

#[component]
pub fn ReplayTabTitle(control: PanelControl) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let title = Memo::new(move |_| control.title());
    let art: RwSignal<Option<ShipArt>, LocalStorage> = RwSignal::new_local(None);

    // Read again for another ship or another build. A read the next run overtakes ends with it.
    let reading = RenderEffect::new(move |_| {
        let PanelTitle::Read { ship: Some(ship), .. } = title.get() else { return };
        let GameDataState::Ready(data) = services.game_data.state() else { return };
        let vfs = data.vfs().clone();
        let _read = spawn_local(async move {
            let read = blocking(move || read_art(&vfs, ship)).await;
            art.set(Some(read));
        });
    });
    on_cleanup_local(move || drop(reading));

    let body = move || match title.get() {
        PanelTitle::Reading => AnyView::new(view! {
            Spinner(label = t!("ui.replay.loading").into_owned())
            text(class = "wt-replay-title__text") {{t!("ui.replay.loading").into_owned()}}
        }),
        PanelTitle::Failed => AnyView::new(view! {
            text(class = "wt-replay-title__text") {{t!("ui.messages.replay_load_failed").into_owned()}}
        }),
        PanelTitle::Read { title, ship } => {
            // Art read for the ship shown before this one is not drawn beside it.
            let drawn = move |pick: fn(&ShipArt) -> Option<AnyView>| {
                art.with(|art| art.as_ref().filter(|art| ship.as_ref() == Some(&art.ship)).and_then(pick))
            };
            let flag = {
                let drawn = drawn.clone();
                move || drawn(flag_view)
            };
            let class = move || drawn(class_view);
            AnyView::new(view! {
                {flag}
                {class}
                text(class = "wt-replay-title__text") {{title}}
            })
        }
    };

    view! {
        row(class = "wt-replay-title", attr:data-testid = "replay-tab-title") {{body}}
    }
}

fn flag_view(art: &ShipArt) -> Option<AnyView> {
    let src = art.flag.as_ref()?.url();

    Some(AnyView::new(view! { image(class = "wt-replay-title__flag", src = src, a11y:hidden = true) }))
}

fn class_view(art: &ShipArt) -> Option<AnyView> {
    let svg = art.class.clone()?;

    Some(AnyView::new(view! {
        vector(class = "wt-replay-title__class", prop:svg = PropValue::from(svg), a11y:hidden = true)
    }))
}

/// `ship`'s flag and white class icon in `vfs`. What the build lacks is left out.
fn read_art(vfs: &VfsPath, ship: TitleShip) -> ShipArt {
    let flag = ship.nation.as_deref().and_then(|nation| game_art::nation_flag_png(vfs, nation)).map(ImageBytes::new);
    let class = game_art::class_icon_svg(vfs, ship.class).and_then(|bytes| String::from_utf8(bytes).ok());

    ShipArt { ship, flag, class }
}
