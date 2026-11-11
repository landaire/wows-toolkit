use std::path::Path;

use gpui_kit::component::tooltip::Tooltip;

use super::*;
use crate::replay_inspector::load::GameDataCache;

/// A stats tab over every replay in one directory. Not saved: the tabs last
/// one run.
pub(super) struct DirectoryStatsTab {
    root: PathBuf,
    view: Entity<StatsView>,
}

impl App {
    /// Opens a stats tab over the replays in `root`, or focuses the one
    /// already open for it. Refused while game data is loading, since no
    /// replay can be read before then.
    pub(super) fn open_directory_stats(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus_directory_stats(&root, cx) {
            return;
        }

        let inspector = self.replay_inspector.read(cx);
        let ready = matches!(inspector.game_data_status(), GameDataStatus::Ready(_));
        let Some(game_data) = inspector.game_data().filter(|_| ready) else {
            crate::toast::warn(t!("ui.stats.directory_stats_unavailable").into_owned(), window, cx);
            return;
        };

        self.open_directory_stats_reading(root, game_data, window, cx);
    }

    /// The same, reading against `game_data` without waiting for the
    /// startup load.
    pub(crate) fn open_directory_stats_reading(
        &mut self,
        root: PathBuf,
        game_data: GameDataCache,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.focus_directory_stats(&root, cx) {
            return;
        }

        let root = canonical_root(root);
        let view = cx.new(|cx| StatsView::new_for_directory(root.clone(), game_data, window, cx));
        let inspector = self.replay_inspector.read(cx);
        let rating = inspector.personal_rating().cloned();
        let loaded = match inspector.game_data_status() {
            GameDataStatus::Ready(loaded) => Some(loaded),
            _ => None,
        };
        view.update(cx, |view, cx| {
            view.set_personal_rating(rating, cx);
            if let Some(loaded) = loaded {
                view.set_game_data(loaded.vfs(), loaded.provider().clone(), cx);
            }
        });

        self.active_tab = AppTab::DirectoryStats(view.entity_id());
        self.directory_stats.push(DirectoryStatsTab { root, view });
        cx.notify();
    }

    /// Focuses the tab open over `root`, if there is one.
    fn focus_directory_stats(&mut self, root: &Path, cx: &mut Context<Self>) -> bool {
        let root = canonical_root(root.to_path_buf());
        let Some(tab) = self.directory_stats.iter().find(|tab| tab.root == root) else {
            return false;
        };

        self.active_tab = AppTab::DirectoryStats(tab.view.entity_id());
        cx.notify();
        true
    }

    /// Drops the tab, which stops its read.
    fn close_directory_stats(&mut self, id: EntityId, cx: &mut Context<Self>) {
        self.directory_stats.retain(|tab| tab.view.entity_id() != id);
        if self.active_tab == AppTab::DirectoryStats(id) {
            self.active_tab = AppTab::Stats;
        }
        cx.notify();
    }

    pub(super) fn directory_stats_tabs(&self) -> impl Iterator<Item = AppTab> + '_ {
        self.directory_stats.iter().map(|tab| AppTab::DirectoryStats(tab.view.entity_id()))
    }

    pub(super) fn directory_stats_view(&self, id: EntityId) -> Option<Entity<StatsView>> {
        self.directory_stats.iter().find(|tab| tab.view.entity_id() == id).map(|tab| tab.view.clone())
    }

    pub(super) fn set_directory_stats_locale(&mut self, cx: &mut Context<Self>) {
        for tab in &self.directory_stats {
            tab.view.update(cx, |view, cx| view.set_locale(cx));
        }
    }

    pub(super) fn directory_stats_tab_label(&self, id: EntityId, selected: bool, cx: &Context<Self>) -> AnyElement {
        let Some(tab) = self.directory_stats.iter().find(|tab| tab.view.entity_id() == id) else {
            return AppTab::Stats.label().into_any_element();
        };
        let full_path = tab.root.display().to_string();
        let label =
            t!("ui.tabs.directory_stats", dir = crate::replay_inspector::view::shortened_path(&tab.root)).into_owned();
        let close = cx.entity();

        h_flex()
            .id(SharedString::from(format!("directory-stats-tab-{id:?}")))
            .gap_1()
            .items_center()
            .when(selected, |row| row.text_color(cx.theme().tab_active_foreground).font_weight(FontWeight::SEMIBOLD))
            .child(crate::icons::icon(AppTab::DirectoryStats(id).glyph()))
            .child(label)
            .tooltip(move |window, cx| Tooltip::new(full_path.clone()).build(window, cx))
            .child(
                Button::new(SharedString::from(format!("close-directory-stats-{id:?}")))
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .tooltip(t!("ui.tabs.close_directory_stats").into_owned())
                    .on_click(move |_event, _window, cx| {
                        cx.stop_propagation();
                        close.update(cx, |this, cx| this.close_directory_stats(id, cx));
                    }),
            )
            .into_any_element()
    }
}

/// Canonical, so two spellings of one directory share a tab. A path that
/// will not resolve is kept as given: the read reports it empty.
fn canonical_root(root: PathBuf) -> PathBuf {
    std::fs::canonicalize(&root).unwrap_or(root)
}
