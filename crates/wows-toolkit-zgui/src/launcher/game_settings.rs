//! The Game Settings dialog: the settings worth changing outside the game, read from and written
//! to the preference files of the default account's install. Read-only while the game runs,
//! since the game rewrites these files when it exits; read again once it has.

use std::num::NonZeroU32;
use std::path::Path;
use std::path::PathBuf;

use rust_i18n::t;
use wgc_launch::InstallFiles;
use wgc_launch::InstallView;
use wgc_launch::RuntimeDirs;
use wows_game_settings::BuildNumber;
use wows_game_settings::Field;
use wows_game_settings::FrameRateLimit;
use wows_game_settings::GameSettings;
use wows_game_settings::GameSettingsStore;
use wows_game_settings::GameStopped;
use wows_game_settings::MonitorIndex;
use wows_game_settings::RefreshRate;
use wows_game_settings::ReplayCount;
use wows_game_settings::Resolution;
use wows_game_settings::Section;
use wows_game_settings::SettingsChanges;
use wows_game_settings::Snapshot;
use wows_game_settings::UiScale;
use wows_game_settings::Volume;
use wows_game_settings::WindowMode;
use wows_toolkit_launcher::InstallSettingsFiles;
use wows_toolkit_launcher::installed_builds;
use wows_toolkit_replay_data::replay_folder::app_runtime_dirs;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::task::spawn_local;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogFooterProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogTitleProps;
use zgui_ui::InputProps;
use zgui_ui::SelectContentProps;
use zgui_ui::SelectItemProps;
use zgui_ui::SelectProps;
use zgui_ui::SelectTriggerProps;
use zgui_ui::SelectTriggerSize;
use zgui_ui::SelectValueProps;
use zgui_ui::SliderProps;
use zgui_ui::SpinnerProps;
use zgui_ui::SwitchProps;
use zgui_ui::SwitchSize;
use zgui_ui::prelude::Binding;

use crate::launcher::CSS;
use crate::launcher::SHEET;
use crate::launcher::model::LauncherModel;
use crate::long_job::long_job;
use crate::shell::HostedDialogProps;
use crate::shell::Shell;

const DIALOG_CSS: &str = zgui::css!(
    ".wt-game-settings {
        display: flex;
        flex-direction: row;
        gap: 12px;
        width: 100%;
        min-width: 0;
        min-height: 320px;
    }
    .wt-game-settings__groups {
        display: flex;
        flex-direction: column;
        gap: 2px;
        width: 110px;
        flex: none;
    }
    .wt-game-settings__group {
        justify-content: flex-start;
    }
    .wt-game-settings__group[data-selected] {
        background-color: var(--lui-color-selection);
        color: var(--lui-color-text-bright);
    }
    .wt-game-settings__fields {
        display: flex;
        flex-direction: column;
        gap: 10px;
        flex: 1 1 auto;
        min-width: 0;
        padding-left: 12px;
        border-left: 1px solid var(--lui-color-border);
    }
    .wt-game-settings__field {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        min-height: 28px;
    }
    .wt-game-settings__name {
        flex: 1 1 auto;
        min-width: 120px;
    }
    .wt-game-settings__control {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 6px;
        width: 190px;
        flex: none;
        min-width: 0;
    }
    .wt-game-settings__control > .zui-input { flex: 1 1 auto; min-width: 0; }
    .wt-game-settings__control .zui-slider { flex: 1 1 auto; }
    .wt-game-settings__control > .wt-game-settings__number { width: 72px; flex: none; }
    .wt-game-settings__banner {
        padding: 6px 8px;
        border: 1px solid var(--wt-color-warn);
        border-radius: 4px;
        color: var(--wt-color-warn);
        font-size: var(--zui-type-size-sm);
    }
    .wt-game-settings__note {
        font-size: var(--zui-type-size-sm);
        color: var(--wt-color-text-dim);
    }"
);

/// Which group of settings the dialog shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Group {
    Display,
    Graphics,
    Sound,
    Replays,
}

impl Group {
    const ALL: [Self; 4] = [Self::Display, Self::Graphics, Self::Sound, Self::Replays];

    fn label(self) -> String {
        match self {
            Self::Display => t!("ui.launcher.game_settings.display"),
            Self::Graphics => t!("ui.launcher.game_settings.graphics"),
            Self::Sound => t!("ui.launcher.game_settings.sound"),
            Self::Replays => t!("ui.launcher.game_settings.replays"),
        }
        .into_owned()
    }

    fn name(self) -> &'static str {
        match self {
            Self::Display => "display",
            Self::Graphics => "graphics",
            Self::Sound => "sound",
            Self::Replays => "replays",
        }
    }
}

/// The install whose files the dialog edits, and the build whose `res_mods` holds the replay
/// settings.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SettingsTarget {
    root: PathBuf,
    build: Option<BuildNumber>,
    /// Where the launcher keeps overlay state. `None` without an app data directory, in which
    /// case no overlay can exist.
    dirs: Option<RuntimeDirs>,
}

impl SettingsTarget {
    /// The default account's install, else the app's game directory.
    fn of(model: LauncherModel) -> Option<Self> {
        let wows_dir = model.wows_dir_untracked();
        let dirs = app_runtime_dirs();
        model.with_untracked(|state| {
            let settings = state.settings.as_ref()?;
            let account = settings.default_account().or_else(|| settings.accounts.first());
            let root = match account {
                Some(account) => account.install.resolve(wows_dir.as_deref()),
                None => wows_dir,
            }?;
            let found = state.launcher.installs().iter().find(|install| install.path == root);
            let build = found
                .and_then(|install| install.build.or_else(|| install.version.as_ref()?.build.map(NonZeroU32::get)))
                .map(BuildNumber)
                .or_else(|| newest_build(&root, dirs.as_ref()));
            Some(Self { root, build, dirs: dirs.clone() })
        })
    }

    /// The install as its last launch left it, for reading. Creates no overlay state, so an install
    /// the launcher never ran is read in place. Blocks.
    fn read_store(&self) -> Result<GameSettingsStore<InstallSettingsFiles>, String> {
        let files = match &self.dirs {
            Some(dirs) => InstallFiles::existing(&self.root, dirs).map_err(|err| err.to_string())?,
            None => InstallFiles::Direct { root: self.root.clone() },
        };

        Ok(GameSettingsStore::new(InstallSettingsFiles(files), self.build))
    }

    /// The install as the game will run it, for writing: a read-only install gets the overlay its
    /// game writes to. Blocks, and may probe the overlay backends.
    fn write_store(&self) -> Result<GameSettingsStore<InstallSettingsFiles>, String> {
        let files = match &self.dirs {
            Some(dirs) => InstallView::prepare(&self.root, dirs).map_err(|err| err.to_string())?.files(),
            None => InstallFiles::Direct { root: self.root.clone() },
        };

        Ok(GameSettingsStore::new(InstallSettingsFiles(files), self.build))
    }
}

/// The highest `bin/<build>` directory of the install, as its view shows it.
fn newest_build(root: &Path, dirs: Option<&RuntimeDirs>) -> Option<BuildNumber> {
    let files = match dirs {
        Some(dirs) => InstallFiles::existing(root, dirs).ok()?,
        None => InstallFiles::Direct { root: root.to_path_buf() },
    };

    installed_builds(&files).ok()?.last().copied()
}

/// What reading the files gave.
#[derive(Clone, Debug, PartialEq)]
enum Loaded {
    Reading,
    /// Boxed: a snapshot is far larger than the other states.
    Read(Box<Snapshot>),
    Failed(String),
}

/// The values on screen, as typed, one signal each.
#[derive(Clone, Copy)]
struct Form {
    window_mode: RwSignal<String, LocalStorage>,
    monitor: RwSignal<String, LocalStorage>,
    width: RwSignal<String, LocalStorage>,
    height: RwSignal<String, LocalStorage>,
    refresh: RwSignal<String, LocalStorage>,
    vsync: RwSignal<bool, LocalStorage>,
    ui_scale: RwSignal<String, LocalStorage>,
    battle_fps: RwSignal<String, LocalStorage>,
    port_fps: RwSignal<String, LocalStorage>,
    skip_intro: RwSignal<bool, LocalStorage>,
    master: RwSignal<f64, LocalStorage>,
    music_port: RwSignal<f64, LocalStorage>,
    music_battle: RwSignal<f64, LocalStorage>,
    replays_kept: RwSignal<String, LocalStorage>,
    versioned: RwSignal<bool, LocalStorage>,
}

impl Form {
    fn new() -> Self {
        Self {
            window_mode: RwSignal::new_local(String::new()),
            monitor: RwSignal::new_local(String::new()),
            width: RwSignal::new_local(String::new()),
            height: RwSignal::new_local(String::new()),
            refresh: RwSignal::new_local(String::new()),
            vsync: RwSignal::new_local(false),
            ui_scale: RwSignal::new_local(String::new()),
            battle_fps: RwSignal::new_local(String::new()),
            port_fps: RwSignal::new_local(String::new()),
            skip_intro: RwSignal::new_local(false),
            master: RwSignal::new_local(0.0),
            music_port: RwSignal::new_local(0.0),
            music_battle: RwSignal::new_local(0.0),
            replays_kept: RwSignal::new_local(String::new()),
            versioned: RwSignal::new_local(false),
        }
    }

    /// Shows what `settings` holds.
    fn load(self, settings: &GameSettings) {
        if let Some(display) = settings.display.available() {
            self.window_mode
                .set(display.window_mode.known().map(|mode| window_mode_value(*mode)).unwrap_or_default().to_owned());
            self.monitor.set(display.monitor_index.known().map(|index| index.get().to_string()).unwrap_or_default());
            let resolution = display.fullscreen_resolution.known();
            self.width.set(resolution.map(|resolution| resolution.width.get().to_string()).unwrap_or_default());
            self.height.set(resolution.map(|resolution| resolution.height.get().to_string()).unwrap_or_default());
            self.refresh.set(
                display
                    .refresh_rate
                    .known()
                    .and_then(|rate| rate.get())
                    .map(|rate| rate.to_string())
                    .unwrap_or_default(),
            );
            self.vsync.set(display.vsync.known().copied().unwrap_or_default());
            self.ui_scale.set(display.ui_scale.known().map(|scale| scale_value(scale.get())).unwrap_or_default());
        }
        if let Some(frame_rate) = settings.frame_rate.available() {
            self.battle_fps.set(fps_text(&frame_rate.battle));
            self.port_fps.set(fps_text(&frame_rate.port));
        }
        if let Some(interface) = settings.interface.available() {
            self.skip_intro.set(interface.skip_intro_video.known().copied().unwrap_or_default());
        }
        if let Some(sound) = settings.sound.available() {
            self.master.set(percent(&sound.master));
            self.music_port.set(percent(&sound.music_port));
            self.music_battle.set(percent(&sound.music_battle));
        }
        if let Some(replays) = settings.replays.available() {
            self.replays_kept.set(replays.kept.known().map(|kept| kept.get().get().to_string()).unwrap_or_default());
            self.versioned.set(replays.versioned.known().copied().unwrap_or_default());
        }
    }

    /// The edits against `settings`. Only a value that differs from the file's is written, and only
    /// where the file holds a value this version knows.
    fn changes(self, settings: &GameSettings) -> Result<SettingsChanges, String> {
        let mut changes = SettingsChanges::default();

        if let Some(display) = settings.display.available() {
            if let Some(current) = display.window_mode.known() {
                let wanted = parse_window_mode(&self.window_mode.get_untracked());
                changes.window_mode = wanted.filter(|wanted| wanted != current);
            }
            if let Some(current) = display.monitor_index.known() {
                let wanted = parse_number::<u8>(&self.monitor.get_untracked(), "ui.launcher.game_settings.monitor")?;
                changes.monitor_index = Some(MonitorIndex::new(wanted)).filter(|wanted| wanted != current);
            }
            if let Some(current) = display.fullscreen_resolution.known() {
                let width = parse_number::<u16>(&self.width.get_untracked(), "ui.launcher.game_settings.resolution")?;
                let height = parse_number::<u16>(&self.height.get_untracked(), "ui.launcher.game_settings.resolution")?;
                let wanted = Resolution::new(width, height).map_err(|err| err.to_string())?;
                changes.fullscreen_resolution = Some(wanted).filter(|wanted| wanted != current);
            }
            if let Some(current) = display.refresh_rate.known() {
                let text = self.refresh.get_untracked();
                let wanted = if text.trim().is_empty() {
                    RefreshRate::display()
                } else {
                    let hz = parse_number::<u16>(&text, "ui.launcher.game_settings.refresh")?;
                    RefreshRate::hz(hz).map_err(|err| err.to_string())?
                };
                changes.refresh_rate = Some(wanted).filter(|wanted| wanted != current);
            }
            if let Some(current) = display.vsync.known() {
                changes.vsync = Some(self.vsync.get_untracked()).filter(|wanted| wanted != current);
            }
            if let Some(current) = display.ui_scale.known() {
                let text = self.ui_scale.get_untracked();
                if text != scale_value(current.get()) {
                    let scale = parse_number::<f32>(&text, "ui.launcher.game_settings.ui_scale")?;
                    changes.ui_scale = Some(UiScale::new(scale).map_err(|err| err.to_string())?);
                }
            }
        }

        if let Some(frame_rate) = settings.frame_rate.available() {
            if let Some(current) = frame_rate.battle.known() {
                let wanted = parse_fps(&self.battle_fps.get_untracked())?;
                changes.battle_frame_rate = Some(wanted).filter(|wanted| wanted != current);
            }
            if let Some(current) = frame_rate.port.known() {
                let wanted = parse_fps(&self.port_fps.get_untracked())?;
                changes.port_frame_rate = Some(wanted).filter(|wanted| wanted != current);
            }
        }

        if let Some(interface) = settings.interface.available()
            && let Some(current) = interface.skip_intro_video.known()
        {
            changes.skip_intro_video = Some(self.skip_intro.get_untracked()).filter(|wanted| wanted != current);
        }

        if let Some(sound) = settings.sound.available() {
            changes.master_volume = volume_change(&sound.master, self.master.get_untracked())?;
            changes.music_port_volume = volume_change(&sound.music_port, self.music_port.get_untracked())?;
            changes.music_battle_volume = volume_change(&sound.music_battle, self.music_battle.get_untracked())?;
        }

        if let Some(replays) = settings.replays.available() {
            if let Some(current) = replays.kept.known() {
                let kept =
                    parse_number::<u32>(&self.replays_kept.get_untracked(), "ui.launcher.game_settings.replays_kept")?;
                let kept = NonZeroU32::new(kept)
                    .ok_or_else(|| t!("ui.launcher.game_settings.replays_kept_zero").into_owned())?;
                changes.replays_kept = Some(ReplayCount::new(kept)).filter(|wanted| wanted != current);
            }
            if let Some(current) = replays.versioned.known() {
                changes.versioned_replays = Some(self.versioned.get_untracked()).filter(|wanted| wanted != current);
            }
        }

        Ok(changes)
    }
}

/// Opens the dialog over the default account's install.
pub fn open(shell: Shell, model: LauncherModel) {
    let Some(target) = SettingsTarget::of(model) else {
        shell.toasts().warn(t!("ui.launcher.game_settings.no_install").into_owned());
        return;
    };

    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        install_stylesheet("wt-game-settings", DIALOG_CSS);
        let target = target.clone();
        let subtitle = match target.build {
            Some(build) => format!("{}, {}", target.root.display(), t!("ui.launcher.game_settings.build", build = build.0)),
            None => target.root.display().to_string(),
        };
        let subtitle = zgui::reactive::StoredValue::new(subtitle);
        let loaded = RwSignal::new_local(Loaded::Reading);
        let form = Form::new();
        let group = RwSignal::new_local(Group::Display);
        let saving = RwSignal::new_local(false);
        let error = RwSignal::new_local(None::<String>);
        let running = Memo::new(move |_| model.with(|state| !state.launcher.game_state().is_idle()));
        let locked = Signal::derive_local(move || running.get() || saving.get());

        // Read on open, and again whenever the game exits, since it rewrites the files then.
        let read_target = target.clone();
        let reading = RenderEffect::new(move |was_running: Option<bool>| {
            let now = running.get();
            if was_running.is_none() || (was_running == Some(true) && !now) {
                read(read_target.clone(), loaded, form);
            }
            now
        });
        on_cleanup_local(move || drop(reading));

        let save_target = zgui::reactive::StoredValue::new(target.clone());
        let save = move || {
            let Loaded::Read(snapshot) = loaded.get_untracked() else { return };
            let changes = match form.changes(snapshot.settings()) {
                Ok(changes) => changes,
                Err(why) => {
                    error.set(Some(why));
                    return;
                }
            };
            if changes.is_empty() {
                handle.close();
                return;
            }
            error.set(None);
            saving.set(true);
            let gate = model.with_untracked(|state| state.launcher.game_gate());
            let target = save_target.get_value();
            spawn_local(async move {
                let saved = long_job(move || {
                    let stopped = GameStopped::check(&gate).map_err(|running| running.to_string())?;
                    target.write_store()?.save(&snapshot, &changes, stopped).map_err(|err| err.to_string())
                })
                .await;
                saving.set(false);
                match saved {
                    Ok(_) => handle.close(),
                    Err(why) => error.set(Some(why)),
                }
            });
        };

        AnyView::new(view! {
            HostedDialog(handle = handle) {
                DialogContent(attr:data-testid = "launcher-game-settings") {
                    DialogHeader {
                        DialogTitle {{t!("ui.launcher.game_settings.title").into_owned()}}
                        text(class = "wt-game-settings__note wt-launcher__truncate") {{subtitle.get_value()}}
                    }
                    Show(when = move || running.get()) {
                        text(class = "wt-game-settings__banner", attr:data-testid = "launcher-game-settings-locked") {{t!("ui.launcher.game_settings.locked").into_owned()}}
                    }
                    {move || match loaded.get() {
                        Loaded::Reading => AnyView::new(view! { Spinner() }),
                        Loaded::Failed(why) => AnyView::new(view! {
                            text(class = "wt-launcher__error", attr:data-testid = "launcher-game-settings-failed") {{why}}
                        }),
                        Loaded::Read(snapshot) => {
                            let settings = snapshot.settings().clone();
                            AnyView::new(view! {
                                row(class = "wt-game-settings") {
                                    column(class = "wt-game-settings__groups") {
                                        for each in move || Group::ALL.to_vec(), key = |each: &Group| *each {
                                            Button(
                                                variant = ButtonVariant::Ghost,
                                                size = ButtonSize::Sm,
                                                class = "wt-game-settings__group",
                                                attr:data-testid = format!("launcher-game-settings-{}", each.name()),
                                                attr:data-selected = move || (group.get() == each).then(String::new),
                                                on:click = move |_| group.set(each)
                                            ) {
                                                {each.label()}
                                            }
                                        }
                                    }
                                    column(class = "wt-game-settings__fields") {
                                        {move || fields(group.get(), &settings, form, locked)}
                                    }
                                }
                            })
                        }
                    }}
                    {move || error.get().map(|why| view! {
                        text(class = "wt-launcher__error", attr:data-testid = "launcher-game-settings-error") {{why}}
                    })}
                    DialogFooter {
                        text(class = "wt-game-settings__note") {{t!("ui.launcher.game_settings.footer").into_owned()}}
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            attr:data-testid = "launcher-game-settings-cancel",
                            on:click = move |_| handle.close()
                        ) {
                            {t!("ui.launcher.game_settings.cancel").into_owned()}
                        }
                        Button(
                            size = ButtonSize::Sm,
                            disabled = Signal::derive_local(move || locked.get() || !matches!(loaded.get(), Loaded::Read(_))),
                            attr:data-testid = "launcher-game-settings-save",
                            on:click = move |_| save()
                        ) {
                            {t!("ui.launcher.game_settings.save").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

/// Reads the files of `target` off the UI thread, and shows what they hold.
fn read(target: SettingsTarget, loaded: RwSignal<Loaded, LocalStorage>, form: Form) {
    loaded.set(Loaded::Reading);
    spawn_local(async move {
        let read = long_job(move || target.read_store()?.read().map_err(|err| err.to_string())).await;
        match read {
            Ok(snapshot) => {
                form.load(snapshot.settings());
                loaded.set(Loaded::Read(Box::new(snapshot)));
            }
            Err(why) => loaded.set(Loaded::Failed(why)),
        }
    });
}

/// The fields of `group`.
fn fields(group: Group, settings: &GameSettings, form: Form, locked: Signal<bool, LocalStorage>) -> AnyView {
    match group {
        Group::Display => display_fields(settings, form, locked),
        Group::Graphics => graphics_fields(settings),
        Group::Sound => sound_fields(settings, form, locked),
        Group::Replays => replay_fields(settings, form, locked),
    }
}

fn display_fields(settings: &GameSettings, form: Form, locked: Signal<bool, LocalStorage>) -> AnyView {
    let mut rows = Vec::new();
    match &settings.display {
        Section::Available(display) => {
            rows.push(field_row(
                "ui.launcher.game_settings.window_mode",
                "launcher-gs-window-mode",
                editable(&display.window_mode, || window_mode_select(form.window_mode, locked)),
            ));
            if let Some(name) = display.monitor_name.known() {
                rows.push(field_row(
                    "ui.launcher.game_settings.monitor_name",
                    "launcher-gs-monitor-name",
                    text_view(name.clone()),
                ));
            }
            rows.push(field_row(
                "ui.launcher.game_settings.monitor",
                "launcher-gs-monitor",
                editable(&display.monitor_index, || number_input(form.monitor, locked, "launcher-gs-monitor-input")),
            ));
            rows.push(field_row(
                "ui.launcher.game_settings.resolution",
                "launcher-gs-resolution",
                editable(&display.fullscreen_resolution, || {
                    AnyView::new(view! {
                        Input(value = form.width, disabled = locked, class = "wt-game-settings__number", label = t!("ui.launcher.game_settings.width").into_owned(), attr:data-testid = "launcher-gs-width")
                        text {"x"}
                        Input(value = form.height, disabled = locked, class = "wt-game-settings__number", label = t!("ui.launcher.game_settings.height").into_owned(), attr:data-testid = "launcher-gs-height")
                    })
                }),
            ));
            rows.push(field_row(
                "ui.launcher.game_settings.refresh",
                "launcher-gs-refresh",
                editable(&display.refresh_rate, || {
                    AnyView::new(view! {
                        Input(
                            value = form.refresh,
                            disabled = locked,
                            placeholder = t!("ui.launcher.game_settings.display_rate").into_owned(),
                            label = t!("ui.launcher.game_settings.refresh").into_owned(),
                            attr:data-testid = "launcher-gs-refresh-input"
                        )
                    })
                }),
            ));
            rows.push(field_row(
                "ui.launcher.game_settings.vsync",
                "launcher-gs-vsync",
                editable(&display.vsync, || switch(form.vsync, locked, "launcher-gs-vsync-switch")),
            ));
            rows.push(field_row(
                "ui.launcher.game_settings.ui_scale",
                "launcher-gs-ui-scale",
                editable(&display.ui_scale, || ui_scale_select(form.ui_scale, locked)),
            ));
        }
        Section::Unavailable(why) => rows.push(unavailable(why.to_string())),
    }

    match &settings.frame_rate {
        Section::Available(frame_rate) => {
            rows.push(field_row(
                "ui.launcher.game_settings.battle_fps",
                "launcher-gs-battle-fps",
                editable(&frame_rate.battle, || fps_input(form.battle_fps, locked, "launcher-gs-battle-fps-input")),
            ));
            rows.push(field_row(
                "ui.launcher.game_settings.port_fps",
                "launcher-gs-port-fps",
                editable(&frame_rate.port, || fps_input(form.port_fps, locked, "launcher-gs-port-fps-input")),
            ));
        }
        Section::Unavailable(why) => rows.push(unavailable(why.to_string())),
    }

    if let Section::Available(interface) = &settings.interface {
        rows.push(field_row(
            "ui.launcher.game_settings.skip_intro",
            "launcher-gs-skip-intro",
            editable(&interface.skip_intro_video, || switch(form.skip_intro, locked, "launcher-gs-skip-intro-switch")),
        ));
    }

    AnyView::new(rows)
}

fn graphics_fields(settings: &GameSettings) -> AnyView {
    let rows = match &settings.graphics {
        Section::Available(graphics) => vec![
            field_row("ui.launcher.game_settings.preset", "launcher-gs-preset", read_only(&graphics.preset)),
            field_row(
                "ui.launcher.game_settings.antialiasing",
                "launcher-gs-antialiasing",
                read_only(&graphics.antialiasing),
            ),
            field_row(
                "ui.launcher.game_settings.textures",
                "launcher-gs-textures",
                read_only(&graphics.texture_quality),
            ),
            field_row(
                "ui.launcher.game_settings.filtering",
                "launcher-gs-filtering",
                read_only(&graphics.texture_filtering),
            ),
            field_row("ui.launcher.game_settings.shadows", "launcher-gs-shadows", read_only(&graphics.shadow_quality)),
            AnyView::new(
                view! { text(class = "wt-game-settings__note") {{t!("ui.launcher.game_settings.graphics_note").into_owned()}} },
            ),
        ],
        Section::Unavailable(why) => vec![unavailable(why.to_string())],
    };

    AnyView::new(rows)
}

fn sound_fields(settings: &GameSettings, form: Form, locked: Signal<bool, LocalStorage>) -> AnyView {
    let rows = match &settings.sound {
        Section::Available(sound) => vec![
            field_row(
                "ui.launcher.game_settings.master",
                "launcher-gs-master",
                editable(&sound.master, || volume_slider(form.master, locked, "launcher-gs-master-slider")),
            ),
            field_row(
                "ui.launcher.game_settings.music_port",
                "launcher-gs-music-port",
                editable(&sound.music_port, || volume_slider(form.music_port, locked, "launcher-gs-music-port-slider")),
            ),
            field_row(
                "ui.launcher.game_settings.music_battle",
                "launcher-gs-music-battle",
                editable(&sound.music_battle, || {
                    volume_slider(form.music_battle, locked, "launcher-gs-music-battle-slider")
                }),
            ),
        ],
        Section::Unavailable(why) => vec![unavailable(why.to_string())],
    };

    AnyView::new(rows)
}

fn replay_fields(settings: &GameSettings, form: Form, locked: Signal<bool, LocalStorage>) -> AnyView {
    let rows = match &settings.replays {
        Section::Available(replays) => vec![
            field_row(
                "ui.launcher.game_settings.replays_kept",
                "launcher-gs-replays-kept",
                editable(&replays.kept, || number_input(form.replays_kept, locked, "launcher-gs-replays-kept-input")),
            ),
            field_row(
                "ui.launcher.game_settings.versioned",
                "launcher-gs-versioned",
                editable(&replays.versioned, || switch(form.versioned, locked, "launcher-gs-versioned-switch")),
            ),
            AnyView::new(
                view! { text(class = "wt-game-settings__note") {{t!("ui.launcher.game_settings.replays_note").into_owned()}} },
            ),
        ],
        Section::Unavailable(why) => vec![unavailable(why.to_string())],
    };

    AnyView::new(rows)
}

/// How many replays the game keeps and whether it sorts them by version, saved on its own. The
/// setup dialog's last step.
#[component]
pub fn ReplayRetention(shell: Shell, model: LauncherModel) -> impl IntoView {
    let target = SettingsTarget::of(model);
    let loaded = RwSignal::new_local(Loaded::Reading);
    let form = Form::new();
    let saving = RwSignal::new_local(false);
    let error = RwSignal::new_local(None::<String>);
    let running = Memo::new(move |_| model.with(|state| !state.launcher.game_state().is_idle()));
    let locked = Signal::derive_local(move || running.get() || saving.get());

    let Some(target) = target else {
        return AnyView::new(view! {
            text(class = "wt-launcher__dim", attr:data-testid = "launcher-retention-no-install") {{t!("ui.launcher.game_settings.no_install").into_owned()}}
        });
    };
    read(target.clone(), loaded, form);

    let save = move || {
        let Loaded::Read(snapshot) = loaded.get_untracked() else { return };
        let all = match form.changes(snapshot.settings()) {
            Ok(changes) => changes,
            Err(why) => {
                error.set(Some(why));
                return;
            }
        };
        let changes = SettingsChanges {
            replays_kept: all.replays_kept,
            versioned_replays: all.versioned_replays,
            ..SettingsChanges::default()
        };
        if changes.is_empty() {
            return;
        }
        error.set(None);
        saving.set(true);
        let gate = model.with_untracked(|state| state.launcher.game_gate());
        let target = target.clone();
        spawn_local(async move {
            let saved = long_job(move || {
                let stopped = GameStopped::check(&gate).map_err(|running| running.to_string())?;
                target.write_store()?.save(&snapshot, &changes, stopped).map_err(|err| err.to_string())
            })
            .await;
            saving.set(false);
            match saved {
                Ok(snapshot) => {
                    form.load(snapshot.settings());
                    loaded.set(Loaded::Read(Box::new(snapshot)));
                    shell.toasts().ok(t!("ui.launcher.game_settings.saved").into_owned());
                }
                Err(why) => error.set(Some(why)),
            }
        });
    };

    AnyView::new(view! {
        column(class = "wt-launcher__column wt-launcher__column--wide", attr:data-testid = "launcher-retention") {
            {move || match loaded.get() {
                Loaded::Reading => AnyView::new(view! { Spinner() }),
                Loaded::Failed(why) => AnyView::new(view! { text(class = "wt-launcher__error") {{why}} }),
                Loaded::Read(snapshot) => {
                    install_stylesheet("wt-game-settings", DIALOG_CSS);
                    replay_fields(snapshot.settings(), form, locked)
                }
            }}
            {move || error.get().map(|why| view! { text(class = "wt-launcher__error") {{why}} })}
            row(class = "wt-launcher__row") {
                Button(
                    size = ButtonSize::Sm,
                    disabled = Signal::derive_local(move || locked.get() || !matches!(loaded.get(), Loaded::Read(_))),
                    attr:data-testid = "launcher-retention-save",
                    on:click = move |_| save()
                ) {
                    {t!("ui.launcher.game_settings.save").into_owned()}
                }
            }
        }
    })
}

/// One field: its name, then its control.
fn field_row(name_key: &'static str, test_id: &'static str, control: AnyView) -> AnyView {
    AnyView::new(view! {
        row(class = "wt-game-settings__field", attr:data-testid = test_id) {
            text(class = "wt-game-settings__name") {{t!(name_key).into_owned()}}
            row(class = "wt-game-settings__control") {{control}}
        }
    })
}

/// The control `make` builds for a value the file holds and this version knows; else the value as
/// text, which a save never overwrites.
fn editable<T>(field: &Field<T>, make: impl FnOnce() -> AnyView) -> AnyView {
    match field {
        Field::Known(_) => make(),
        Field::Absent => text_view(t!("ui.launcher.game_settings.absent").into_owned()),
        Field::Unrecognized(found) => {
            text_view(t!("ui.launcher.game_settings.unrecognized", value = found).into_owned())
        }
    }
}

fn read_only<T: std::fmt::Debug>(field: &Field<T>) -> AnyView {
    match field {
        Field::Known(value) => text_view(format!("{value:?}")),
        Field::Absent => text_view(t!("ui.launcher.game_settings.absent").into_owned()),
        Field::Unrecognized(found) => text_view(found.clone()),
    }
}

fn text_view(words: String) -> AnyView {
    AnyView::new(view! { text(class = "wt-game-settings__note") {{words}} })
}

fn unavailable(why: String) -> AnyView {
    AnyView::new(view! { text(class = "wt-game-settings__note", attr:data-testid = "launcher-gs-unavailable") {{why}} })
}

fn number_input(
    value: RwSignal<String, LocalStorage>,
    locked: Signal<bool, LocalStorage>,
    test_id: &'static str,
) -> AnyView {
    AnyView::new(
        view! { Input(value = value, disabled = locked, class = "wt-game-settings__number", attr:data-testid = test_id) },
    )
}

fn fps_input(
    value: RwSignal<String, LocalStorage>,
    locked: Signal<bool, LocalStorage>,
    test_id: &'static str,
) -> AnyView {
    AnyView::new(view! {
        Input(
            value = value,
            disabled = locked,
            placeholder = t!("ui.launcher.game_settings.unlimited").into_owned(),
            attr:data-testid = test_id
        )
    })
}

fn switch(value: RwSignal<bool, LocalStorage>, locked: Signal<bool, LocalStorage>, test_id: &'static str) -> AnyView {
    let on = Signal::derive_local(move || value.get());
    AnyView::new(view! {
        Switch(
            checked = Binding::controlled(on, move |wanted: bool| {
                if !locked.get_untracked() {
                    value.set(wanted);
                }
            }),
            size = SwitchSize::Sm,
            attr:data-testid = test_id
        )
    })
}

fn volume_slider(
    value: RwSignal<f64, LocalStorage>,
    locked: Signal<bool, LocalStorage>,
    test_id: &'static str,
) -> AnyView {
    AnyView::new(view! {
        Slider(value = value, min = 0.0, max = 100.0, step = 1.0, disabled = locked, attr:data-testid = test_id)
        text(class = "wt-game-settings__note") {{move || format!("{:.0}%", value.get())}}
    })
}

fn window_mode_select(value: RwSignal<String, LocalStorage>, locked: Signal<bool, LocalStorage>) -> AnyView {
    let chosen = Signal::derive_local(move || value.get());
    let modes = [WindowMode::Windowed, WindowMode::Borderless, WindowMode::Fullscreen];
    AnyView::new(view! {
        Select(value = Binding::controlled(chosen, move |mode: String| {
            if !locked.get_untracked() {
                value.set(mode);
            }
        })) {
            SelectTrigger(size = SelectTriggerSize::Sm, label = t!("ui.launcher.game_settings.window_mode").into_owned(), attr:data-testid = "launcher-gs-window-mode-select") {
                SelectValue()
            }
            SelectContent {
                for mode in move || modes.to_vec(), key = |mode: &WindowMode| window_mode_value(*mode) {
                    SelectItem(value = window_mode_value(mode), text = window_mode_label(mode)) {
                        {window_mode_label(mode)}
                    }
                }
            }
        }
    })
}

fn ui_scale_select(value: RwSignal<String, LocalStorage>, locked: Signal<bool, LocalStorage>) -> AnyView {
    const SCALES: [f32; 7] = [0.75, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

    let chosen = Signal::derive_local(move || value.get());
    let current = value.get_untracked();
    let mut choices: Vec<String> = SCALES.iter().map(|scale| scale_value(*scale)).collect();
    if !current.is_empty() && !choices.contains(&current) {
        choices.push(current);
    }
    let choices = zgui::reactive::StoredValue::new(choices);
    AnyView::new(view! {
        Select(value = Binding::controlled(chosen, move |scale: String| {
            if !locked.get_untracked() {
                value.set(scale);
            }
        })) {
            SelectTrigger(size = SelectTriggerSize::Sm, label = t!("ui.launcher.game_settings.ui_scale").into_owned(), attr:data-testid = "launcher-gs-ui-scale-select") {
                SelectValue()
            }
            SelectContent {
                for scale in move || choices.get_value(), key = |scale: &String| scale.clone() {
                    SelectItem(value = scale.clone(), text = scale_label(&scale)) {
                        {scale_label(&scale)}
                    }
                }
            }
        }
    })
}

fn window_mode_value(mode: WindowMode) -> &'static str {
    match mode {
        WindowMode::Windowed => "windowed",
        WindowMode::Borderless => "borderless",
        WindowMode::Fullscreen => "fullscreen",
    }
}

fn window_mode_label(mode: WindowMode) -> String {
    match mode {
        WindowMode::Windowed => t!("ui.launcher.game_settings.windowed"),
        WindowMode::Borderless => t!("ui.launcher.game_settings.borderless"),
        WindowMode::Fullscreen => t!("ui.launcher.game_settings.fullscreen"),
    }
    .into_owned()
}

fn parse_window_mode(value: &str) -> Option<WindowMode> {
    [WindowMode::Windowed, WindowMode::Borderless, WindowMode::Fullscreen]
        .into_iter()
        .find(|mode| window_mode_value(*mode) == value)
}

/// A scale as the select keeps it: two decimals, so 1.1 and 1.10 are one choice.
fn scale_value(scale: f32) -> String {
    format!("{scale:.2}")
}

fn scale_label(value: &str) -> String {
    match value.parse::<f32>() {
        Ok(scale) => format!("{:.0}%", scale * 100.0),
        Err(_) => value.to_owned(),
    }
}

fn fps_text(field: &Field<FrameRateLimit>) -> String {
    field.known().and_then(|limit| limit.get()).map(|fps| fps.to_string()).unwrap_or_default()
}

/// A frame rate limit as typed: blank or 0 is unlimited, as the game stores it.
fn parse_fps(text: &str) -> Result<FrameRateLimit, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(FrameRateLimit::unlimited());
    }
    let fps: u16 = parse_number(text, "ui.launcher.game_settings.battle_fps")?;
    if fps == 0 {
        return Ok(FrameRateLimit::unlimited());
    }

    FrameRateLimit::limited(fps).map_err(|err| err.to_string())
}

fn percent(field: &Field<Volume>) -> f64 {
    field.known().map(|volume| f64::from(volume.get()) * 100.0).unwrap_or_default().round()
}

/// The volume to write, when the slider moved off the file's value by at least one step.
fn volume_change(field: &Field<Volume>, shown: f64) -> Result<Option<Volume>, String> {
    let Some(current) = field.known() else {
        return Ok(None);
    };
    if (f64::from(current.get()) * 100.0).round() == shown.round() {
        return Ok(None);
    }

    Volume::new((shown / 100.0) as f32).map(Some).map_err(|err| err.to_string())
}

fn parse_number<T: std::str::FromStr>(text: &str, name_key: &'static str) -> Result<T, String> {
    text.trim().parse().map_err(|_| t!("ui.launcher.game_settings.not_a_number", field = t!(name_key)).into_owned())
}
