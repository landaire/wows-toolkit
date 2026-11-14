//! Drawing views on the graphics device without a display, and writing what
//! they drew to a PNG, for the screenshot tests.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::AnyWindowHandle;
use gpui_kit::App;
use gpui_kit::AppContext as _;
use gpui_kit::Entity;
use gpui_kit::HeadlessAppContext;
use gpui_kit::Pixels;
use gpui_kit::Render;
use gpui_kit::Size;
use gpui_kit::Window;
use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt as _;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::ZoomFactor;

/// An app that shapes text with the system's fonts and draws through wgpu,
/// with motion reduced so every frame is a settled one.
pub fn headless_app() -> HeadlessAppContext {
    let text_system = Arc::new(gpui_wgpu::CosmicTextSystem::new("IBM Plex Sans"));
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(crate::icons::AppAssets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_kit::component::init(cx);
        cx.set_reduce_motion(true);
    });

    cx
}

/// Opens a dark window showing the view `build` makes, under the same root
/// the app draws its window in, and draws it until it settles.
pub fn open<V: Render>(
    cx: &mut HeadlessAppContext,
    size: Size<Pixels>,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    let mut view = None;
    let window = cx
        .open_window(size, |window, cx| {
            crate::theme::apply_egui_theme(ThemeChoice::Dark, ZoomFactor::DEFAULT, window, cx);
            let built = build(window, cx);
            view = Some(built.clone());
            cx.new(|cx| Root::new(built, window, cx))
        })
        .expect("the window opens");
    let window = window.into();
    settle(cx, window, Duration::from_millis(500));
    settle(cx, window, Duration::from_millis(500));

    (window, view.expect("the view was built"))
}

/// Lets timers such as a tooltip's delay run out, then draws a frame.
pub fn settle(cx: &mut HeadlessAppContext, window: AnyWindowHandle, wait: Duration) {
    cx.run_until_parked();
    cx.advance_clock(wait);
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).expect("the window is open");
}

/// Writes the window's last frame to `<WT_SHOT_DIR>/gpui-<name>.png` and
/// returns the path.
pub fn save(cx: &mut HeadlessAppContext, window: AnyWindowHandle, name: &str) -> PathBuf {
    let directory = PathBuf::from(std::env::var("WT_SHOT_DIR").expect("WT_SHOT_DIR is set"));
    std::fs::create_dir_all(&directory).expect("the screenshot directory can be made");
    let path = directory.join(format!("gpui-{name}.png"));
    let image = cx.capture_screenshot(window).expect("the frame can be read back");
    image.save(&path).expect("the screenshot is written");
    println!("{}", path.display());

    path
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use gpui_kit::AppContext as _;
    use gpui_kit::SharedString;
    use gpui_kit::px;
    use gpui_kit::size;
    use gpui_kit::test::TestWindowExt as _;
    use wows_toolkit_replay_data::load::GameDataCache;
    use wows_toolkit_services::stats::SessionData;
    use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
    use wows_toolkit_viewmodel::stats::PerGameStat;
    use wows_toolkit_viewmodel::stats::sample::SampleCatalog;
    use wows_toolkit_viewmodel::stats::sample::sample_games;

    use crate::stats::ships::StatsShipsPanel;
    use crate::stats::view::StatsView;

    /// The Stats tab over made-up games in a real build's ships, then its
    /// Ships list with the newest ship's table open, written to `WT_SHOT_DIR`.
    /// Reads the expected values in `WT_SHOT_EXPECTED_VALUES` and build
    /// `WT_SHOT_BUILD` from the game data dumps under `WT_SHOT_GAME_DATA`.
    #[test]
    #[ignore = "reads game data from the machine and draws on the graphics device"]
    fn the_stats_tab_shows_match_contribution() {
        let variable = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is set"));
        let mut table = PersonalRatingData::new();
        let bytes = std::fs::read(variable("WT_SHOT_EXPECTED_VALUES")).expect("the expected values read");
        table.load_from_bytes(&bytes).expect("the expected values parse");
        let table = Arc::new(table);
        let build = variable("WT_SHOT_BUILD").parse().expect("WT_SHOT_BUILD is a build number");
        let game_data = GameDataCache::without_install()
            .with_cache_dir(&variable("WT_SHOT_GAME_DATA"))
            .get_or_load_build(build)
            .expect("the build loads");
        let catalog = SampleCatalog::from_build(game_data.provider());
        let end = jiff::civil::date(2026, 11, 13).at(21, 0, 0, 0);
        let games = sample_games(&catalog, Some(&table), end, 80, 11);
        let newest = games.iter().max_by(|a, b| a.sort_key.cmp(&b.sort_key)).expect("games were made").ship_id;

        let mut cx = super::headless_app();
        let session =
            SessionData { games: games.clone(), personal_rating: Some(Arc::clone(&table)), ..SessionData::default() };
        let (window, _view) = super::open(&mut cx, size(px(1600.), px(950.)), |window, cx| {
            cx.new(|cx| {
                let mut view = StatsView::new(window, cx);
                view.set_game_data(game_data.vfs(), Arc::clone(game_data.provider()), cx);
                view.apply_session(session, window, cx);
                view
            })
        });
        super::save(&mut cx, window, "stats-overview-battle-impact");

        let (window, _panel) = super::open(&mut cx, size(px(900.), px(700.)), |window, cx| {
            cx.new(|cx| {
                let mut panel = StatsShipsPanel::new(window, cx);
                panel.set_game_data(game_data.vfs(), Arc::clone(game_data.provider()), cx);
                panel.set_personal_rating(Some(table), cx);
                let refs: Vec<&PerGameStat> = games.iter().collect();
                panel.set_games(&refs, cx);
                panel
            })
        });
        let toggle = SharedString::from(format!("ship-toggle-{newest}"));
        cx.update_window(window, |_, window, cx| window.click(toggle, cx)).expect("the window is open");
        super::settle(&mut cx, window, Duration::from_millis(500));
        super::save(&mut cx, window, "stats-ships-battle-impact");
    }
}
