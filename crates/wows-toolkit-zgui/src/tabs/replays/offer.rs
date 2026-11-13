//! The offer to fetch the game data a directory of replays needs and this machine does not have:
//! one row per build, with what the repository publishes for it, ticked where it can be fetched.

use std::collections::BTreeSet;

use rust_i18n::t;
use wows_data_mgr::download_repo::BuildUpdateStatus;
use wows_data_mgr::download_repo::DownloadPlan;
use wows_data_mgr::download_repo::RemoteAvailability;
use wows_toolkit_replay_data::replay_dir::MissingBuild;
use wows_toolkit_services::game_data_cache;
use wows_toolkit_services::game_data_cache::CacheOutcome;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogDescriptionProps;
use zgui_ui::DialogFooterProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;
use zgui_ui::prelude::Binding;

use crate::services::Job;
use crate::shell::HostedDialogProps;
use crate::shell::Shell;
use crate::shell::settings::Setting;
use crate::tabs::replays::browser::cache_base;
use crate::tabs::replays::browser::proxy_url;

const SHEET: &str = "wt-game-data-offer";

const CSS: &str = css!(
    ".wt-offer { display: flex; flex-direction: column; gap: 6px; max-width: 560px; }
    .wt-offer__row { display: flex; flex-direction: row; align-items: center; gap: 8px; font-size: 13px; }
    .wt-offer__count { font-size: 12px; color: var(--wt-color-text-dim); }"
);

/// What this session has offered, so each build is put to the reader once.
#[derive(Clone, Copy)]
pub struct GameDataOffer {
    offered: StoredValue<BTreeSet<u32>, LocalStorage>,
    /// Builds reported while an offer or a cache job was up, offered once it is over.
    pending: StoredValue<Vec<MissingBuild>, LocalStorage>,
    showing: StoredValue<bool, LocalStorage>,
    /// Set while a download the reader accepted runs, so its end relists the directories.
    pub fetching: StoredValue<bool, LocalStorage>,
    /// The tab, which the planning runs under.
    owner: StoredValue<Owner, LocalStorage>,
}

impl GameDataOffer {
    /// Made in the replays tab's body.
    pub fn new() -> Self {
        Self {
            offered: StoredValue::new_local(BTreeSet::new()),
            pending: StoredValue::new_local(Vec::new()),
            showing: StoredValue::new_local(false),
            fetching: StoredValue::new_local(false),
            owner: StoredValue::new_local(Owner::current().expect("the offer is made inside a component")),
        }
    }

    /// The reader asked which builds the listing needs: says so when none, and offers every one
    /// again, declined before or not.
    pub fn check(self, shell: Shell, missing: Vec<MissingBuild>) {
        if missing.is_empty() {
            shell.toasts().info(t!("ui.replay_folders.game_data_available").into_owned());
            return;
        }
        self.offered.update_value(|offered| {
            for build in &missing {
                offered.remove(&build.build);
            }
        });
        self.offer(shell, missing);
    }

    /// Offers to fetch `missing`, leaving out builds already put to the reader this session: a
    /// walk after a download reports what it still cannot read, and offering that again would be
    /// a loop rather than a question.
    pub fn offer(self, shell: Shell, missing: Vec<MissingBuild>) {
        #[cfg(test)]
        if !crate::tabs::replays::testing::fetches_allowed() {
            return;
        }

        let missing: Vec<MissingBuild> = self
            .offered
            .with_value(|offered| missing.into_iter().filter(|build| !offered.contains(&build.build)).collect());
        if missing.is_empty() {
            return;
        }
        if shell.cache.busy_untracked() || self.showing.get_value() {
            self.pending.update_value(|pending| {
                for build in missing {
                    if !pending.iter().any(|held| held.build == build.build) {
                        pending.push(build);
                    }
                }
            });
            return;
        }
        let services = shell.services();
        let Some(base) = cache_base(services) else {
            let open = shell.settings_drawer;
            shell.toasts().show(
                landaire_ui::Notice::warn(t!("ui.dialogs.download_plan_no_cache_dir").into_owned())
                    .action(t!("ui.settings.panel.open").into_owned(), move || open.open_at(Setting::CacheDirectory)),
            );
            return;
        };
        let Some(runtime) = services.runtime() else { return };

        self.showing.set_value(true);
        shell.toasts().info(t!("ui.dialogs.download_plan_pending").into_owned());
        let builds: Vec<(u32, Option<String>)> =
            missing.iter().map(|build| (build.build, build.version.clone())).collect();
        let proxy = proxy_url(services);
        let asked = base.clone();
        let job = services.jobs.start(Job { name: t!("ui.app.status_cache_planning").into_owned(), progress: None });
        let owner = self.owner.get_value();
        let _planning = owner.with(|| {
            spawn_local(async move {
                // The repository's futures are not `Send`, so each is built and driven on one worker.
                let outcome = blocking(move || {
                    match wows_toolkit_services::http::client(
                        crate::http::USER_AGENT,
                        &proxy,
                        reqwest::redirect::Policy::default(),
                    ) {
                        Ok(client) => runtime.block_on(game_data_cache::plan(&client, &asked, &builds)),
                        Err(err) => CacheOutcome::Failed(err.to_string()),
                    }
                })
                .await;
                drop(job);
                let plan = match outcome {
                    CacheOutcome::Planned { plan } => Some(plan),
                    outcome => {
                        if let CacheOutcome::Failed(reason) = &outcome {
                            tracing::warn!("game data: the download could not be planned: {reason}");
                        }
                        shell.toasts().warn(t!("ui.dialogs.download_plan_failed").into_owned());
                        None
                    }
                };
                self.show(shell, missing, plan, base);
            })
        });
    }

    fn show(self, shell: Shell, missing: Vec<MissingBuild>, plan: Option<DownloadPlan>, base: std::path::PathBuf) {
        let rows = StoredValue::new_local(describe_missing_builds(&missing, plan.as_ref()));
        let total = plan.as_ref().map(|plan| plan.unique_missing_objects);
        let eligible = StoredValue::new_local(eligible_builds(plan.as_ref()));
        let missing = StoredValue::new_local(missing);
        let base = StoredValue::new_local(base);

        shell.dialogs.open(move |handle| {
            install_stylesheet(SHEET, CSS);
            let ticked = RwSignal::new_local(eligible.get_value());
            // Every build in the dialog is answered for, ticked or not, once: the cleanup that
            // follows an accept or decline must not clear `showing` for the offer started after it.
            let done = StoredValue::new_local(false);
            let answered = move || {
                if done.try_get_value() != Some(false) {
                    return;
                }
                done.try_set_value(true);
                self.showing.set_value(false);
                missing.with_value(|missing| {
                    self.offered.update_value(|offered| offered.extend(missing.iter().map(|build| build.build)));
                });
            };
            // Dismissed by Escape or a press outside it, the dialog still answered for its builds.
            on_cleanup_local(answered);
            let accept = move || {
                answered();
                handle.close();
                let picked = ticked.get_untracked();
                let wanted: Vec<BuildUpdateStatus> = missing.with_value(|missing| {
                    missing
                        .iter()
                        .filter(|build| picked.contains(&build.build))
                        .map(|build| BuildUpdateStatus {
                            build: build.build,
                            version: build.version.clone().unwrap_or_default(),
                        })
                        .collect()
                });
                if !wanted.is_empty() {
                    self.fetching.set_value(true);
                    shell.fetch_cache_builds(base.get_value(), wanted);
                }
                self.offer_pending(shell);
            };
            let decline = move || {
                answered();
                handle.close();
                self.offer_pending(shell);
            };
            let row_views = move || -> Vec<AnyView> {
                rows.with_value(|rows| {
                    rows.iter()
                        .map(|(build, said)| {
                            let (number, said) = (*build, said.clone());
                            let enabled = eligible.with_value(|eligible| eligible.contains(&number));
                            AnyView::new(view! {
                                OfferRow(number = number, said = said, enabled = enabled, ticked = ticked)
                            })
                        })
                        .collect()
                })
            };
            let none_eligible = eligible.with_value(BTreeSet::is_empty);

            AnyView::new(view! {
                HostedDialog(handle = handle) {
                    DialogContent(attr:data-testid = "download-offer") {
                        DialogHeader {
                            DialogTitle {{t!("ui.windows.download_game_data").into_owned()}}
                            DialogDescription {{t!("ui.dialogs.download_game_data_intro").into_owned()}}
                        }
                        column(class = "wt-offer") {
                            {row_views}
                            {total.map(|count| view! {
                                text(class = "wt-offer__count") {{t!("ui.dialogs.download_objects_to_fetch", count = count).into_owned()}}
                            })}
                        }
                        DialogFooter {
                            Button(
                                size = ButtonSize::Sm,
                                variant = ButtonVariant::Outline,
                                attr:data-testid = "download-offer-cancel",
                                on:click = move |_| decline()
                            ) {
                                {t!("ui.buttons.cancel").into_owned()}
                            }
                            Button(
                                size = ButtonSize::Sm,
                                disabled = none_eligible,
                                attr:data-testid = "download-offer-ok",
                                on:click = move |_| accept()
                            ) {
                                {t!("ui.buttons.download").into_owned()}
                            }
                        }
                    }
                }
            })
        });
    }

    /// Offers what was reported while the last offer was up.
    pub fn offer_pending(self, shell: Shell) {
        let pending = self.pending.try_update_value(std::mem::take).unwrap_or_default();
        if !pending.is_empty() {
            self.offer(shell, pending);
        }
    }
}

impl Default for GameDataOffer {
    fn default() -> Self {
        Self::new()
    }
}

/// One build's row: ticked to fetch it, greyed out when the repository has nothing for it.
#[component]
fn OfferRow(number: u32, said: String, enabled: bool, ticked: RwSignal<BTreeSet<u32>, LocalStorage>) -> impl IntoView {
    let checked = Signal::derive_local(move || if ticked.get().contains(&number) { Checked::Yes } else { Checked::No });
    let set = move |checked: Checked| {
        ticked.update(|ticked| {
            if checked == Checked::Yes {
                ticked.insert(number);
            } else {
                ticked.remove(&number);
            }
        });
    };

    view! {
        row(class = "wt-offer__row") {
            Checkbox(
                checked = Binding::controlled(checked, set),
                disabled = !enabled,
                attr:data-testid = format!("download-build-{number}"),
                a11y:label = said.clone()
            )
            text {{said}}
        }
    }
}

/// Each build's row text: its version, how many replays need it and what the repository has.
fn describe_missing_builds(missing: &[MissingBuild], plan: Option<&DownloadPlan>) -> Vec<(u32, String)> {
    missing
        .iter()
        .map(|build| {
            let named = t!(
                "ui.dialogs.download_build_row",
                version = build.version.clone().unwrap_or_else(|| "?".to_owned()),
                build = build.build
            );
            let needed = t!("ui.dialogs.download_replays_needing", count = build.replays);
            let said = plan
                .and_then(|plan| plan.resolved.iter().find(|resolved| resolved.requested_build == build.build))
                .map(|resolved| availability_said(&resolved.availability));
            let row = match said {
                Some(said) => format!("{named} -- {needed} -- {said}"),
                None => format!("{named} -- {needed}"),
            };
            (build.build, row)
        })
        .collect()
}

fn availability_said(availability: &RemoteAvailability) -> String {
    match availability {
        RemoteAvailability::Exact => t!("ui.dialogs.download_availability_exact").into_owned(),
        RemoteAvailability::Nearest { version, build } => {
            t!("ui.dialogs.download_availability_nearest", version = version, build = build).into_owned()
        }
        RemoteAvailability::Unpublished => t!("ui.dialogs.download_availability_unpublished").into_owned(),
        RemoteAvailability::Unreachable => t!("ui.dialogs.download_availability_unreachable").into_owned(),
    }
}

/// The builds the repository can serve. With no plan, none: an offer to download data that was
/// never published is worse than saying so.
fn eligible_builds(plan: Option<&DownloadPlan>) -> BTreeSet<u32> {
    plan.map(|plan| {
        plan.resolved
            .iter()
            .filter(|build| {
                matches!(build.availability, RemoteAvailability::Exact | RemoteAvailability::Nearest { .. })
            })
            .map(|build| build.requested_build)
            .collect()
    })
    .unwrap_or_default()
}
