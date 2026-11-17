//! The launcher's palette entries, offered while the launcher is enabled: Play as each saved
//! account, Game settings, Add account, Update and Open replay in game.

use rust_i18n::t;
use wows_toolkit_launcher::LoginTarget;
use zgui::component;
use zgui::reactive::Memo;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;

use crate::launcher::account::open_sign_in;
use crate::launcher::display::AccountLabel;
use crate::launcher::display::PlayState;
use crate::launcher::model::LauncherModel;
use crate::launcher::model::PlayOrigin;
use crate::launcher::play::play;
use crate::launcher::play::start_update;
use crate::launcher::realm::realm_glyph;
use crate::launcher::realm::realm_name;
use crate::launcher::replay;
use crate::shell::Shell;
use crate::shell::palette::LauncherCommand;
use crate::shell::palette::OfferGlyph;
use crate::shell::palette::PaletteRun;
use crate::shell::palette::RootCommand;

/// Offers the entries for as long as it is built.
#[component]
pub fn LauncherOffers(shell: Shell, model: LauncherModel) -> impl IntoView {
    let palette = shell.palette;
    palette.offer(
        RootCommand::Launcher(LauncherCommand::GameSettings),
        PaletteRun::action(move || crate::launcher::game_settings::open(shell, model)),
    );
    palette.offer(
        RootCommand::Launcher(LauncherCommand::AddAccount),
        PaletteRun::action(move || open_sign_in(shell, model, LoginTarget::NewAccount)),
    );
    palette.offer(
        RootCommand::Launcher(LauncherCommand::OpenReplayInGame),
        PaletteRun::action(move || replay::pick_and_open(shell, model)),
    );

    let accounts = Memo::new(move |_| {
        model
            .with(|state| {
                state
                    .settings
                    .as_ref()
                    .map(|settings| settings.accounts.iter().map(AccountLabel::of).collect::<Vec<_>>())
            })
            .unwrap_or_default()
    });
    let update = Memo::new(move |_| {
        let wows_dir = model.wows_dir();
        model.with(|state| match PlayState::of(state, wows_dir.as_deref(), false) {
            PlayState::Update { install, .. } => Some(install),
            _ => None,
        })
    });

    view! {
        for account in move || accounts.get(), key = |account: &AccountLabel| account.clone() {
            PlayAsOffer(shell = shell, model = model, account = account)
        }
        Show(when = move || update.get().is_some()) {
            UpdateOffer(shell = shell, model = model)
        }
    }
}

#[component]
fn PlayAsOffer(shell: Shell, model: LauncherModel, account: AccountLabel) -> impl IntoView {
    let key = account.key;
    let label = t!("ui.launcher.palette.play_as", nickname = account.nickname, realm = account.realm).into_owned();
    let glyph = OfferGlyph { glyph: realm_glyph(key.realm), label: realm_name(key.realm) };
    shell.palette.offer_labelled(
        RootCommand::Launcher(LauncherCommand::PlayAs(key)),
        label,
        Some(glyph),
        PaletteRun::action(move || play(shell, model, key, PlayOrigin::Chosen)),
    );
}

#[component]
fn UpdateOffer(shell: Shell, model: LauncherModel) -> impl IntoView {
    shell.palette.offer(
        RootCommand::Launcher(LauncherCommand::UpdateGame),
        PaletteRun::action(move || {
            let wows_dir = model.wows_dir_untracked();
            let install = model.with_untracked(|state| match PlayState::of(state, wows_dir.as_deref(), false) {
                PlayState::Update { install, .. } => Some(install),
                _ => None,
            });
            if let Some(install) = install {
                start_update(shell, model, install);
            }
        }),
    );
}
