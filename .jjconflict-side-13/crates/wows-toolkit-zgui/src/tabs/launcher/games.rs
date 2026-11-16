//! The install-a-game section: each known game, the version a fresh install gets, and the install
//! itself.

use rust_i18n::t;
use wgc_catalog::known_games::KnownGame;
use zgui::component;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::SpinnerProps;

use crate::services::Services;
use crate::shell::Shell;
use crate::tabs::launcher::SectionProps;
use crate::tabs::launcher::display::DownloadTarget;
use crate::tabs::launcher::display::GameVersion;
use crate::tabs::launcher::download::DownloadStatusProps;
use crate::tabs::launcher::model::LauncherModel;

#[component]
pub fn GamesSection(model: LauncherModel, shell: Shell) -> impl IntoView {
    let locale = Services::current().locale;
    let games: Vec<AnyView> = (0..wgc_catalog::known_games::known_games().len())
        .map(|index| AnyView::new(view! { GameRow(model = model, shell = shell, index = index) }))
        .collect();

    view! {
        Section(heading = "ui.launcher.install_heading") {
            text(class = "wt-launcher__dim") {{locale.text("ui.launcher.install_hint")}}
            {games}
        }
    }
}

/// The known game at `index` in `wgc_catalog::known_games`.
#[component]
fn GameRow(model: LauncherModel, shell: Shell, index: usize) -> impl IntoView {
    let locale = Services::current().locale;
    let game: &'static KnownGame = &wgc_catalog::known_games::known_games()[index];
    let version = Memo::new(move |_| model.with(|state| GameVersion::of(state.launcher.available_game(game))));
    let can_install = Memo::new(move |_| model.with(|state| state.launcher.can_install(game)));
    let unavailable = Signal::derive_local(move || !can_install.get());

    view! {
        column(class = "wt-launcher__card", attr:data-testid = format!("launcher-game-{index}")) {
            row(class = "wt-launcher__row") {
                text(class = "wt-launcher__strong") {{game.display_name}}
                {move || match version.get() {
                    GameVersion::Checking => AnyView::new(view! {
                        row(class = "wt-launcher__row wt-launcher__row--tight") {
                            Spinner()
                            text(class = "wt-launcher__dim") {{locale.text("ui.launcher.checking_version")}}
                        }
                    }),
                    GameVersion::Version(version) => AnyView::new(view! {
                        text(class = "wt-launcher__text") {{move || {
                            locale.track();
                            t!("ui.launcher.version", version = version.as_str()).into_owned()
                        }}}
                    }),
                    GameVersion::Unreported => AnyView::new(view! {
                        text(class = "wt-launcher__dim") {{locale.text("ui.launcher.version_unreported")}}
                    }),
                    GameVersion::Unavailable(detail) => AnyView::new(view! {
                        row(class = "wt-launcher__row") {
                            text(class = "wt-launcher__dim") {{move || {
                                locale.track();
                                t!("ui.launcher.unavailable", error = detail.as_str()).into_owned()
                            }}}
                            Button(
                                variant = ButtonVariant::Ghost,
                                size = ButtonSize::Sm,
                                attr:data-testid = format!("launcher-retry-game-{index}"),
                                on:click = move |_| model.op(|launcher, _, env| {
                                    launcher.start_available_games_check(env);
                                    false
                                })
                            ) {
                                {locale.text("ui.launcher.retry")}
                            }
                        }
                    }),
                    GameVersion::Nothing => AnyView::new(view! { box() }),
                }}
            }
            DownloadStatus(model = model, shell = shell, target = DownloadTarget::Game(index))
            row(class = "wt-launcher__row") {
                Button(
                    variant = ButtonVariant::Outline,
                    size = ButtonSize::Sm,
                    disabled = unavailable,
                    attr:data-testid = format!("launcher-install-game-{index}"),
                    on:click = move |_| install_game(model, shell, game)
                ) {
                    {locale.text("ui.launcher.install")}
                }
            }
        }
    }
}

/// Asks where to install `game`, and installs it there.
fn install_game(model: LauncherModel, shell: Shell, game: &'static KnownGame) {
    let asked = wows_toolkit_services::dialog::pick_folder_in(
        &t!("ui.launcher.install_title"),
        &wgc_download::default_install_dir(),
    );
    spawn_local(async move {
        let Some(dest) = shell.chosen_path(asked).await else { return };
        let started = model.run(move |launcher, _, env| (launcher.install_known_game(game, dest, env), false));
        if let Some(Err(err)) = started {
            shell.toasts().failed(err.to_string());
        }
    });
}
