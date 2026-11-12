//! Saving the replays dock's arrangement, and putting it back with its replays when the tab is
//! built again.

use std::path::PathBuf;

use landaire_ui::SavedLayout;
use landaire_ui::SavedNode;
use wows_toolkit_config::queries;
use zgui::reactive::RenderEffect;
use zgui::reactive::StoredValue;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;

use crate::tabs::replays::workspace::Docked;
use crate::tabs::replays::workspace::ReplayWorkspace;

/// Not shared with the GPUI front end, which saves no layout for this dock and whose dock format
/// is its own.
pub const DOCK_LAYOUT_KEY: &str = "replay_inspector_dock_layout_zgui";

/// Reads the saved layout once the settings are open and puts it back, then saves every change.
///
/// A viewport, a watched playback and a replay whose file is gone are left out of what is put
/// back. Nothing is put back once the reader has opened something, which then wins.
pub fn persist(workspace: ReplayWorkspace) {
    let services = workspace.services;
    // Off until the saved layout was read, so the empty dock does not overwrite it.
    let enabled = StoredValue::new(false);
    let saved: StoredValue<Option<SavedLayout>> = StoredValue::new(None);
    let started = StoredValue::new(false);

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
                    tracing::warn!("replays: the saved dock layout could not be read: {err}");
                    None
                }
                Err(err) => {
                    tracing::warn!("replays: the saved dock layout was not read: {err}");
                    None
                }
            };
            if let Some(layout) = layout.filter(|_| !workspace.touched.get_value()) {
                let listed = layout_replays(&layout);
                let present: Vec<PathBuf> =
                    blocking(move || listed.into_iter().filter(|path| path.is_file()).collect()).await;
                if !workspace.touched.get_value() {
                    restore(workspace, &layout, present);
                }
            }
            saved.set_value(Some(workspace.dock.dump()));
            enabled.set_value(true);
        });
    });
    let saving = RenderEffect::new(move |_| {
        let layout = workspace.dock.dump();
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

/// Puts `layout` back with the replays of `present` in it, and takes out what cannot be put back.
pub(super) fn restore(workspace: ReplayWorkspace, layout: &SavedLayout, present: Vec<PathBuf>) {
    let dock = workspace.dock;
    workspace.adopt_restored(present.clone());
    if let Err(err) = dock.restore(layout) {
        tracing::warn!("replays: the saved dock layout was not restored: {err}");
    }
    for entry in dock.panels() {
        let keeps = matches!(Docked::of(&entry.id), Some(Docked::Replay(path)) if present.contains(&path));
        if !keeps {
            let _ = dock.remove(&entry.id);
        }
    }
    workspace.forget_closed();
}

/// The replays `layout` holds, in no order.
fn layout_replays(layout: &SavedLayout) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut pending: Vec<&SavedNode> = [&layout.center, &layout.left, &layout.right, &layout.bottom]
        .iter()
        .filter_map(|region| region.root.as_ref())
        .collect();
    while let Some(node) = pending.pop() {
        match node {
            SavedNode::Tabs { panels, .. } => {
                paths.extend(panels.iter().filter_map(|entry| match Docked::of(&entry.id) {
                    Some(Docked::Replay(path)) => Some(path),
                    _ => None,
                }))
            }
            SavedNode::Split { parts, .. } => pending.extend(parts.iter().map(|part| &part.node)),
        }
    }
    paths
}
