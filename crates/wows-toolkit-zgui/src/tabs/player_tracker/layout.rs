//! Saving the tracker dock's arrangement and putting it back when the tab is built again.

use landaire_ui::DockState;
use landaire_ui::SavedLayout;
use wows_toolkit_config::queries;
use zgui::reactive::RenderEffect;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::spawn_local;

use crate::services::Services;
use crate::tabs::player_tracker::Section;

/// Not egui's `player_tracker` dock layout row, whose format is egui_dock's.
pub const DOCK_LAYOUT_KEY: &str = "player_tracker_dock_layout_zgui";

#[cfg(test)]
thread_local! {
    /// Whether the dock reads and saves its layout. Off in a test unless it asks: the tests share
    /// one settings database, and a section one test brings to the front would open the next on it.
    static PERSISTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Lets the docks built on this thread read and save their layout until the guard is dropped.
#[cfg(test)]
#[must_use = "the layout is left alone again as soon as the guard is dropped"]
pub struct PersistenceAllowed {
    before: bool,
}

#[cfg(test)]
impl Drop for PersistenceAllowed {
    fn drop(&mut self) {
        PERSISTS.set(self.before);
    }
}

#[cfg(test)]
pub fn allow_persistence() -> PersistenceAllowed {
    PersistenceAllowed { before: PERSISTS.replace(true) }
}

/// Reads the saved layout once the settings are open and puts it back, then saves every change.
/// Nothing is put back once the reader has changed the dock, which then wins.
pub fn persist(services: Services, dock: DockState) {
    #[cfg(test)]
    if !PERSISTS.get() {
        return;
    }
    // Off until the saved layout was read, so the default dock does not overwrite it.
    let enabled = StoredValue::new(false);
    let saved: StoredValue<Option<SavedLayout>> = StoredValue::new(None);
    let started = StoredValue::new(false);
    let default = StoredValue::new(dock.dump_untracked());

    let loading = RenderEffect::new(move |_| {
        if services.settings.read(|_| ()).is_none() || started.get_value() {
            return;
        }
        let Some(pool) = services.settings.pool() else { return };
        started.set_value(true);
        let _task = spawn_local(async move {
            let read = services
                .on_tokio(async move { queries::try_get_setting::<SavedLayout>(&pool, DOCK_LAYOUT_KEY).await })
                .await;
            // An unreadable row is written over by the next change rather than never again.
            let layout = match read {
                Ok(Ok(layout)) => layout,
                Ok(Err(err)) => {
                    tracing::warn!("player tracker: the saved dock layout could not be read: {err}");
                    None
                }
                Err(err) => {
                    tracing::warn!("player tracker: the saved dock layout was not read: {err}");
                    None
                }
            };
            let current = dock.dump_untracked();
            if default.with_value(|default| *default != current) {
                services.settings.save(DOCK_LAYOUT_KEY, &current);
            } else if let Some(layout) = layout {
                restore(dock, &layout);
            }
            saved.set_value(Some(dock.dump_untracked()));
            enabled.set_value(true);
        });
    });
    let saving = RenderEffect::new(move |_| {
        let layout = dock.dump();
        if !enabled.get_value() || saved.with_value(|saved| saved.as_ref() == Some(&layout)) {
            return;
        }
        services.settings.save(DOCK_LAYOUT_KEY, &layout);
        saved.set_value(Some(layout));
    });
    on_cleanup_local(move || {
        drop(loading);
        drop(saving);
    });
}

/// Puts `layout` back if it holds each section exactly once; otherwise the dock keeps the default.
fn restore(dock: DockState, layout: &SavedLayout) {
    let default = dock.dump_untracked();
    if let Err(err) = dock.restore(layout) {
        tracing::warn!("player tracker: the saved dock layout was not restored: {err}");
        return;
    }

    let mut panels: Vec<Option<Section>> =
        dock.panels_untracked().iter().map(|entry| Section::from_id(&entry.id)).collect();
    panels.sort_by_key(|section| section.map(Section::id));
    let mut expected: Vec<Option<Section>> = Section::ALL.into_iter().map(Some).collect();
    expected.sort_by_key(|section| section.map(Section::id));
    if panels == expected {
        return;
    }

    tracing::warn!("player tracker: the saved dock layout does not hold every section; using the default");
    if let Err(err) = dock.restore(&default) {
        tracing::error!("player tracker: the default dock layout was not put back: {err}");
    }
}
