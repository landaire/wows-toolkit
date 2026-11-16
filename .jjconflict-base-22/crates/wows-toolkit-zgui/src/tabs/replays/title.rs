//! A replay tab's title, as the GPUI tab draws it: a spinner while the battle is read, then the
//! recording player's flag and ship class, pointing right, beside "{ship} - {map}".

use rust_i18n::t;
use wowsunpack::vfs::VfsPath;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
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
    /// The white class icon, pointing right, as SVG markup.
    class: Option<String>,
}

#[component]
pub fn ReplayTabTitle(#[prop(into)] title: Signal<PanelTitle, LocalStorage>) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let title = Memo::new(move |_| title.get());
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
    let class = game_art::class_icon_svg(vfs, ship.class).ok();

    ShipArt { ship, flag, class }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use landaire_ui::Appearance;
    use landaire_ui::ThemeRootProps;
    use landaire_ui::test_support::Drawing;
    use landaire_ui::test_support::HeadlessOptions;
    use landaire_ui::test_support::HeadlessWindow;
    use landaire_ui::test_support::WindowSize;
    use wows_toolkit_replay_data::load::LoadedGameData;
    use wows_toolkit_viewmodel::settings::ThemeChoice;
    use wowsunpack::game_params::provider::GameMetadataProvider;
    use wowsunpack::game_params::types::Species;
    use zgui::component;
    use zgui::reactive::Signal;
    use zgui::view;
    use zgui::view::IntoView;

    use super::ReplayTabTitleProps;
    use crate::game_art;
    use crate::interaction_tests::settle_in_real_time;
    use crate::services::Services;
    use crate::tabs::replays::panel::PanelTitle;
    use crate::tabs::replays::panel::TitleShip;
    use crate::theme::SemanticTokensProps;

    /// The title of a battle in a battleship, over a build whose only file is the battleship
    /// class icon, pointing up.
    #[component]
    fn TitleRoot() -> impl IntoView {
        let services = Services::provide(None);
        let provider = GameMetadataProvider::from_params_no_specs(Vec::new()).expect("an empty provider builds");
        let vfs = game_art::testing::upright_class_icon(Species::Battleship);
        services.game_data.set_ready_for_test(Arc::new(LoadedGameData::for_test(provider, vfs, 1)));
        let ship = TitleShip { nation: None, class: Species::Battleship };
        let title = PanelTitle::Read { title: "Yamato - Ocean".to_owned(), ship: Some(ship) };

        view! {
            ThemeRoot {
                SemanticTokens(choice = ThemeChoice::Dark)
                ReplayTabTitle(title = Signal::stored_local(title))
            }
        }
    }

    /// The game draws a class icon pointing up; a replay tab's title shows it pointing right.
    #[test]
    fn the_class_icon_points_right() {
        let options = HeadlessOptions {
            drawing: Drawing::Device,
            desktop: Some(Appearance::Dark),
            ..HeadlessOptions::new(WindowSize { width: 300.0, height: 60.0 })
        };
        let mut window = match HeadlessWindow::open(options, || view! { TitleRoot() }) {
            Ok(window) => window,
            Err(err) if game_art::testing::no_device(&err) => {
                eprintln!("skipped: no graphics adapter to draw the title on");
                return;
            }
            Err(err) => panic!("the title opens: {err}"),
        };
        settle_in_real_time(&mut window);

        game_art::testing::assert_points_right(&mut window, "wt-replay-title__class");
    }
}
