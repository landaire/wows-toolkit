mod damage_types;
mod listing_row;
mod models;
mod sorting;
mod workspace;

use workspace::ReplayRequestSlot;
pub(crate) use workspace::ReplayWorkspace;
use workspace::alt_perspective_slot_id;
use workspace::request_slot_id;
pub(crate) use workspace::shorten_root;
use workspace::workspace_group_salt;
use workspace::workspace_leaf_salt;
use workspace::workspace_salt;

use std::path::PathBuf;

pub use models::Achievement;
pub use models::ClanColor;
pub use models::Damage;
pub use models::DamageInteraction;
pub use models::Hits;
pub use models::PlayerReport;
pub use models::PlayerTint;
pub use models::PotentialDamage;
pub use models::SkillInfo;
pub use models::TranslatedBuild;
pub use models::ship_class_icon_from_species;
use rust_i18n::t;
pub use sorting::ReplayColumn;
pub use sorting::SortColumn;
use sorting::SortKey;
pub use sorting::SortOrder;
use wows_replays::analyzer::battle_controller::ConnectionChangeKind;
use wt_translations::keys;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io::BufWriter;
use std::io::Write;

use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::Sender;

use rootcause::Report;

use crate::collab::Permissions;
use crate::collab::SessionCommand;
use crate::collab::SessionStatus;
use crate::data::settings::ReplayGrouping;
use crate::data::settings::ReplaySettings;
use crate::data::wows_data::GameAsset;
use crate::data::wows_data::SharedWoWsData;
use crate::db::index::rows::WorkspaceId;
use crate::icons;
use crate::replay::timeline::TimelineExtractionError;
use crate::replay::timeline::TimelineState;
use crate::replay::timeline::extract_timeline_events;
use crate::replay::timeline::format_timeline_event;
use crate::replay::timeline::merge_timelines;
use crate::replay::timeline::ui::TimelineFilter;
use crate::replay::timeline::ui::timeline_filter_bar;
use crate::replay::timeline::ui::timeline_list;
use crate::task::BackgroundTask;
use crate::task::BackgroundTaskKind;
use crate::task::ReplayExportFormat;
use crate::task::ReplaySource;
use crate::task::ToastMessage;
use crate::ui::theme::semantic::SemanticExt;
use crate::update_background_task;
use crate::util::replay_export::FlattenedVehicle;
use crate::util::replay_export::Match;

use damage_types::*;
use egui::Color32;
use egui::ComboBox;
use egui::Context;
use egui::FontId;
use egui::Id;
use egui::Image;
use egui::ImageSource;
use egui::Label;
use egui::Margin;
use egui::OpenUrl;
use egui::PopupCloseBehavior;
use egui::RichText;
use egui::Sense;
use egui::Separator;
use egui::Style;
use egui::TextFormat;
use egui::Tooltip;
use egui::UiKind;
use egui::Vec2;
use egui::text::LayoutJob;

use escaper::decode_html;
use jiff::Timestamp;
use parking_lot::Mutex;
use parking_lot::RwLock;
use tracing::debug;

use tracing::error;
use wows_battle_world::BattleWorld;
use wows_battle_world::merged::MergedReplays;
use wows_battle_world::report::BattleReport;
use wows_replays::ReplayFile;
use wows_replays::VehicleInfoMeta;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::types::AccountId;

use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wows_replay_insights::fire_chance::analysis::ExclusionReason;
use wows_replay_insights::fire_chance::analysis::FormulaOp;
use wows_replay_insights::fire_chance::analysis::PerShipFireChance;
use wows_replay_insights::fire_chance::analysis::UnattributedFireReason;

use itertools::Itertools;
use wows_minimap_renderer::renderer::weapon_group_label;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::TranslationKey;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::CrewSkillName;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::skill_translation_keys_for;
use wowsunpack::game_types::DamageStatCategory;
use wowsunpack::recognized::Recognized;

use crate::app::ToolkitTabViewer;
use crate::ui::plaintext_viewer;
use crate::ui::plaintext_viewer::FileType;
use crate::util;
use crate::util::build_ship_config_url;
use crate::util::build_short_ship_config_url;
use crate::util::build_wows_numbers_url;
use crate::util::error::ToolkitError;
use crate::util::player_color_for_team_relation;
use crate::util::separate_number;

const CHAT_VIEW_WIDTH: f32 = 500.0;

/// Minimum width the replay listing is allowed to occupy, and the clip-rect
/// width below which the tree is skipped for a frame. Below this the tree
/// cannot lay out a row, and egui_ltreeview panics on the degenerate
/// (possibly inverted) clip rect that `show_collapsible` produces mid-animation.
const REPLAY_LISTING_MIN_WIDTH: f32 = 100.0;

/// Above this many replays the grouped listing opens with every group closed,
/// so the first frame does not lay out every row in the directory.
const LARGE_LISTING_THRESHOLD: usize = 500;

/// A single replay viewer tab inside the Replay Inspector dock area.
#[derive(Clone)]
pub struct ReplayTab {
    pub replay: Arc<RwLock<Replay>>,
    /// Unique identifier for this tab instance.
    pub id: u64,
}

/// A replay file path paired with its parsed replay data.
type ReplayEntry = (std::path::PathBuf, Arc<RwLock<Replay>>);
/// A named group of replay entries (e.g., grouped by date or ship name).
type ReplayGroup = (String, Vec<ReplayEntry>);

use std::cmp::Reverse;

/// Paints `text` rotated a quarter turn counter-clockwise, reading bottom-to-top,
/// horizontally centred in `rect` and starting `top_offset` below its top edge.
fn paint_vertical_caption(ui: &egui::Ui, rect: egui::Rect, top_offset: f32, text: &str) {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().weak_text_color(),
    );
    // With a -90deg angle the galley grows downward from its anchor, so the
    // anchor is the top of the run and the text width becomes its height.
    let pos = egui::pos2(rect.center().x + galley.size().y / 2.0, rect.top() + top_offset);
    ui.painter().add(
        egui::epaint::TextShape::new(pos, galley, ui.visuals().weak_text_color())
            .with_angle(-std::f32::consts::FRAC_PI_2),
    );
}

/// Calculate a win/loss rate summary string like " - 5W/3L (63%)".
fn win_rate_label(replays: &[ReplayEntry]) -> String {
    let (wins, losses) = replays.iter().fold((0u32, 0u32), |(w, l), (_, replay)| match replay.read().battle_result() {
        Some(BattleResult::Win(_)) => (w + 1, l),
        Some(BattleResult::Loss(_)) => (w, l + 1),
        _ => (w, l),
    });
    let total = wins + losses;
    if total > 0 {
        format!(" - {}W/{}L ({:.0}%)", wins, losses, (wins as f64 / total as f64) * 100.0)
    } else {
        String::new()
    }
}

/// Show context menu items for a single replay leaf node.
fn show_leaf_context_menu(
    ui: &mut egui::Ui,
    replay_weak: &Weak<RwLock<Replay>>,
    path: &std::path::PathBuf,
    wows_dir: &str,
    ws_id: WorkspaceId,
) {
    if ui.button(wt_translations::icon_t(icons::BROWSER, &t!("ui.replay.context.open_in_new_tab"))).clicked() {
        if let Some(r) = replay_weak.upgrade() {
            ui.ctx().data_mut(|data| {
                data.insert_temp(request_slot_id(ws_id, ReplayRequestSlot::OpenReplayNewTab), Arc::downgrade(&r));
            });
        }
        ui.close_kind(UiKind::Menu);
    }
    ui.separator();
    if ui.button(wt_translations::icon_t(icons::CLIPBOARD, &t!("ui.replay.context.copy_path"))).clicked() {
        ui.ctx().copy_text(path.to_string_lossy().into_owned());
        ui.close_kind(UiKind::Menu);
    }
    if ui.button(wt_translations::icon_t(icons::CLIPBOARD, &t!("ui.replay.context.copy_replay"))).clicked() {
        copy_files_to_clipboard(std::slice::from_ref(path));
        ui.close_kind(UiKind::Menu);
    }
    if ui.button(wt_translations::icon_t(icons::FOLDER, &t!("ui.replay.context.show_in_explorer"))).clicked() {
        util::open_file_explorer(path);
        ui.close_kind(UiKind::Menu);
    }
    if !wows_dir.is_empty() {
        let alt_held = ui.input(|i| i.modifiers.alt);
        let label = if alt_held {
            wt_translations::icon_t(icons::KEYBOARD, &t!("ui.replay.context.show_replay_controls"))
        } else {
            wt_translations::icon_t(icons::GAME_CONTROLLER, &t!("ui.replay.context.open_in_game"))
        };
        if ui.button(label).clicked() {
            if alt_held {
                ui.ctx().data_mut(|data| {
                    // App-wide: opens the single reference window regardless of workspace.
                    data.insert_temp(egui::Id::new("open_replay_controls_window"), true);
                });
            } else {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("pending_confirmation_request"),
                        Some(crate::tab_state::ConfirmableAction::OpenInGame { replay_path: path.clone() }),
                    );
                });
            }
            ui.close_kind(UiKind::Menu);
        }
    }
    if ui.button(wt_translations::icon_t(icons::PLAY, &t!("ui.replay.context.render_replay"))).clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(request_slot_id(ws_id, ReplayRequestSlot::ContextMenuRenderReplay), replay_weak.clone());
        });
        ui.close_kind(UiKind::Menu);
    }
    if ui.button(t!("ui.replay.context.render_to_video")).clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(request_slot_id(ws_id, ReplayRequestSlot::BatchRenderReplays), vec![replay_weak.clone()]);
        });
        ui.close_kind(UiKind::Menu);
    }
    if ui.button(t!("ui.replay.context.render_to_clipboard")).clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                request_slot_id(ws_id, ReplayRequestSlot::BatchRenderClipboard),
                vec![replay_weak.clone()],
            );
        });
        ui.close_kind(UiKind::Menu);
    }
    ui.separator();
    if ui.button(t!("ui.replay.context.set_session_stats_one")).clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("pending_confirmation_request"),
                Some(crate::tab_state::ConfirmableAction::SetAsSessionStats { replays: vec![replay_weak.clone()] }),
            );
        });
        ui.close_kind(UiKind::Menu);
    }
    if ui.button(t!("ui.replay.context.add_session_stats_one")).clicked() {
        ui.ctx().data_mut(|data| {
            // App-wide: feeds the one global session-stats total, not a per-workspace one.
            data.insert_temp(egui::Id::new("add_to_session_stats_request"), vec![replay_weak.clone()]);
        });
        ui.close_kind(UiKind::Menu);
    }
}

/// Show context menu items for a group node (date or ship).
fn show_group_context_menu(
    ui: &mut egui::Ui,
    paths: &[std::path::PathBuf],
    replays: &[Weak<RwLock<Replay>>],
    ws_id: WorkspaceId,
) {
    let count = replays.len();

    // Batch render
    let render_label: String = if count == 1 {
        t!("ui.replay.context.render_to_video").into()
    } else {
        t!("ui.replay.context.render_to_video_many", count = count).into()
    };
    if ui.button(render_label).clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(request_slot_id(ws_id, ReplayRequestSlot::BatchRenderReplays), replays.to_vec());
        });
        ui.close_kind(UiKind::Menu);
    }
    let clipboard_label: String = if count == 1 {
        t!("ui.replay.context.render_to_clipboard").into()
    } else {
        t!("ui.replay.context.render_to_clipboard_many", count = count).into()
    };
    if ui.button(clipboard_label).clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(request_slot_id(ws_id, ReplayRequestSlot::BatchRenderClipboard), replays.to_vec());
        });
        ui.close_kind(UiKind::Menu);
    }
    ui.separator();
    let copy_label: String = if count == 1 {
        t!("ui.replay.context.copy_replay").into()
    } else {
        t!("ui.replay.context.copy_replays", count = count).into()
    };
    if ui.button(copy_label).clicked() {
        copy_files_to_clipboard(paths);
        ui.close_kind(UiKind::Menu);
    }
    let session_label: String = if count == 1 {
        t!("ui.replay.context.set_session_stats_one").into()
    } else {
        t!("ui.replay.context.set_session_stats_many", count = count).into()
    };
    if ui.button(session_label).clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("pending_confirmation_request"),
                Some(crate::tab_state::ConfirmableAction::SetAsSessionStats { replays: replays.to_vec() }),
            );
        });
        ui.close_kind(UiKind::Menu);
    }
    let add_label: String = if count == 1 {
        t!("ui.replay.context.add_session_stats_one").into()
    } else {
        t!("ui.replay.context.add_session_stats_many", count = count).into()
    };
    if ui.button(add_label).clicked() {
        ui.ctx().data_mut(|data| {
            // App-wide: feeds the one global session-stats total, not a per-workspace one.
            data.insert_temp(egui::Id::new("add_to_session_stats_request"), replays.to_vec());
        });
        ui.close_kind(UiKind::Menu);
    }
}

/// Lookup maps for a grouped tree view, bundling leaf and group ID mappings.
#[derive(Clone)]
struct GroupedTreeMaps {
    /// Leaf node ID -> replay (weak ref)
    leaf_replays: HashMap<egui::Id, Weak<RwLock<Replay>>>,
    /// Leaf node ID -> file path
    leaf_paths: HashMap<egui::Id, std::path::PathBuf>,
    /// Group node ID -> child replays (weak refs)
    group_replays: HashMap<egui::Id, Vec<Weak<RwLock<Replay>>>>,
    /// Group node ID -> child node IDs
    group_child_ids: HashMap<egui::Id, Vec<egui::Id>>,
    /// Group node ID -> child file paths
    group_paths: HashMap<egui::Id, Vec<std::path::PathBuf>>,
}

impl GroupedTreeMaps {
    /// Collect replays and paths from a set of selected node IDs, deduplicating
    /// leaf nodes that are already covered by a selected group.
    fn collect_selected(&self, selected_ids: &[egui::Id]) -> (Vec<Weak<RwLock<Replay>>>, Vec<std::path::PathBuf>) {
        let mut covered_by_group: std::collections::HashSet<egui::Id> = std::collections::HashSet::new();
        let mut replays: Vec<Weak<RwLock<Replay>>> = Vec::new();
        let mut paths: Vec<std::path::PathBuf> = Vec::new();
        for id in selected_ids {
            if let Some(group_replays) = self.group_replays.get(id) {
                replays.extend(group_replays.iter().cloned());
                if let Some(child_ids) = self.group_child_ids.get(id) {
                    covered_by_group.extend(child_ids.iter().copied());
                }
            }
            if let Some(group_paths) = self.group_paths.get(id) {
                paths.extend(group_paths.iter().cloned());
            }
            if !covered_by_group.contains(id) {
                if let Some(replay_weak) = self.leaf_replays.get(id) {
                    replays.push(replay_weak.clone());
                }
                if let Some(path) = self.leaf_paths.get(id) {
                    paths.push(path.clone());
                }
            }
        }
        (replays, paths)
    }

    /// Show the fallback (multi-selection) context menu for tree views.
    fn show_multi_selection_context_menu(&self, ui: &mut egui::Ui, selected_ids: &[egui::Id], ws_id: WorkspaceId) {
        let (selected_replays, selected_paths) = self.collect_selected(selected_ids);

        if !selected_paths.is_empty() {
            let copy_label = if selected_paths.len() == 1 {
                "Copy Replay".to_string()
            } else {
                format!("Copy {} Replays", selected_paths.len())
            };
            if ui.button(copy_label).clicked() {
                copy_files_to_clipboard(&selected_paths);
                ui.close_kind(UiKind::Menu);
            }
        }

        if !selected_replays.is_empty() {
            let count = selected_replays.len();

            // Batch render
            let render_label: String = if count == 1 {
                t!("ui.replay.context.render_to_video").into()
            } else {
                t!("ui.replay.context.render_to_video_many", count = count).into()
            };
            if ui.button(render_label).clicked() {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        request_slot_id(ws_id, ReplayRequestSlot::BatchRenderReplays),
                        selected_replays.clone(),
                    );
                });
                ui.close_kind(UiKind::Menu);
            }
            let clipboard_label: String = if count == 1 {
                t!("ui.replay.context.render_to_clipboard").into()
            } else {
                t!("ui.replay.context.render_to_clipboard_many", count = count).into()
            };
            if ui.button(clipboard_label).clicked() {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        request_slot_id(ws_id, ReplayRequestSlot::BatchRenderClipboard),
                        selected_replays.clone(),
                    );
                });
                ui.close_kind(UiKind::Menu);
            }
            ui.separator();

            let set_label = if count == 1 {
                "Set as Session Stats (1 replay)".to_string()
            } else {
                format!("Set as Session Stats ({} replays)", count)
            };
            if ui.button(set_label).clicked() {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("pending_confirmation_request"),
                        Some(crate::tab_state::ConfirmableAction::SetAsSessionStats {
                            replays: selected_replays.clone(),
                        }),
                    );
                });
                ui.close_kind(UiKind::Menu);
            }
            let add_label = if count == 1 {
                "Add to Session Stats (1 replay)".to_string()
            } else {
                format!("Add to Session Stats ({} replays)", count)
            };
            if ui.button(add_label).clicked() {
                ui.ctx().data_mut(|data| {
                    // App-wide: feeds the one global session-stats total, not a per-workspace one.
                    data.insert_temp(egui::Id::new("add_to_session_stats_request"), selected_replays);
                });
                ui.close_kind(UiKind::Menu);
            }
        }
    }
}

/// Resolve raw battle results arrays into named JSON objects.
///
/// Delegates to [`wows_replay_insights::battle_report::resolve_battle_results`].
fn resolve_battle_results(results: serde_json::Value, constants: &serde_json::Value) -> serde_json::Value {
    wows_replay_insights::battle_report::resolve_battle_results(results, constants)
}

/// Why fire-section geometry could not be read from the game install.
#[derive(Debug, thiserror::Error)]
enum FireSectionSourceError {
    #[error("could not resolve assets.bin's path in the game vfs: {0}")]
    Path(String),
    #[error("could not open assets.bin: {0}")]
    Open(String),
    #[error("could not read assets.bin: {0}")]
    Read(#[from] std::io::Error),
}

/// Read `content/assets.bin` from the resolved build's game data.
fn open_assets_bin(wows_data: &crate::data::wows_data::WorldOfWarshipsData) -> Result<Vec<u8>, FireSectionSourceError> {
    let assets_path =
        wows_data.vfs.join("content/assets.bin").map_err(|e| FireSectionSourceError::Path(e.to_string()))?;
    let mut file = assets_path.open_file().map_err(|e| FireSectionSourceError::Open(e.to_string()))?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes)?;
    Ok(bytes)
}

/// Resolve fire-section geometry for every hull among `victims`, backed by a
/// per-build on-disk cache so `assets.bin` is parsed once per hull per game
/// build rather than once per replay.
///
/// A hull that fails to resolve, or an unreadable/unparseable `assets.bin`,
/// simply has no entry in the returned map: `analyze` treats a missing
/// geometry as `NoSectionGeometry` for that victim rather than the whole
/// statistic failing. `cache_dir` being `None` (the cache location could not
/// be resolved) is the same story: resolution still runs, it just is not
/// persisted.
fn resolve_fire_section_geometry(
    wows_data: &crate::data::wows_data::WorldOfWarshipsData,
    cache_dir: Option<&std::path::Path>,
    build_number: u32,
    victims: &HashMap<wows_replays::types::EntityId, wows_replay_insights::fire_chance::analysis::VictimContext>,
) -> HashMap<String, wowsunpack::models::fire_nodes::FireSectionGeometry> {
    let mut expected_nodes: HashMap<&str, usize> = HashMap::new();
    for victim in victims.values() {
        expected_nodes.entry(victim.hull_model_path.as_str()).or_insert_with(|| victim.hull_section_count());
    }

    let mut cache =
        cache_dir.map(|dir| wowsunpack::models::fire_nodes_cache::FireSectionCache::load(dir, build_number));
    let mut resolved = HashMap::new();
    let mut misses = Vec::new();
    for (&path, &nodes) in &expected_nodes {
        match cache.as_ref().and_then(|cache| cache.get(path, nodes)) {
            Some(geom) => {
                resolved.insert(path.to_string(), geom);
            }
            None => misses.push((path, nodes)),
        }
    }

    if misses.is_empty() {
        return resolved;
    }

    match open_assets_bin(wows_data) {
        Ok(bytes) => match wowsunpack::models::assets_bin::parse_assets_bin(&bytes) {
            Ok(db) => {
                let self_id_index = db.build_self_id_index();
                for (path, nodes) in misses {
                    match wowsunpack::models::fire_nodes::resolve_fire_sections(&db, &self_id_index, path, nodes) {
                        Ok(geom) => {
                            if let Some(cache) = cache.as_mut()
                                && let Err(error) = cache.insert(path, nodes, &geom)
                            {
                                tracing::warn!(
                                    hull = path,
                                    %error,
                                    "freshly resolved fire-section geometry disagreed with the cache"
                                );
                            }
                            resolved.insert(path.to_string(), geom);
                        }
                        Err(error) => {
                            tracing::debug!(hull = path, %error, "fire-section geometry unresolved");
                        }
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "could not parse assets.bin for fire-section geometry"),
        },
        Err(error) => tracing::debug!(%error, "assets.bin unavailable for fire-section geometry"),
    }

    if let (Some(cache), Some(dir)) = (&cache, cache_dir)
        && let Err(error) = cache.save(dir)
    {
        tracing::warn!(%error, "could not save fire-section cache");
    }

    resolved
}

/// Compute effective fire chance for the recording player of `report`.
///
/// `None` whenever the underlying facts do not resolve: no self vehicle, no
/// resolvable build, an unresolved secondary battery, or no victim hull with
/// fire-section geometry. A result with no eligible hits is still `Some`, and
/// the render path shows it as an unknown rate rather than a zero one. Never
/// panics: an unreadable
/// `assets.bin` degrades to a geometry lookup that always misses, which
/// `analyze` reports as no result rather than an approximation.
fn compute_fire_chance(
    report: &BattleReport,
    params: &GameMetadataProvider,
    wows_data: &crate::data::wows_data::WorldOfWarshipsData,
    deps: &crate::data::wows_data::ReplayDependencies,
) -> Option<wows_replay_insights::fire_chance::analysis::EffectiveFireChance> {
    let resolved = match wows_replay_insights::fire_chance::resolve::resolve_fire_chance_input(report, params) {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::debug!(%error, "fire chance input did not resolve");
            return None;
        }
    };

    let build_number = wows_data.build_number;
    let cache_dir = crate::task::replays::game_data_dump_base_with_override(deps.wows_data_map.game_data_cache_dir())
        .map(|base| base.join("fire_sections").join(build_number.to_string()));

    let geometry_map = resolve_fire_section_geometry(wows_data, cache_dir.as_deref(), build_number, resolved.victims());
    let geometry = |path: &str| geometry_map.get(path).cloned();

    let input = resolved.input(report, params, &geometry);
    let result = wows_replay_insights::fire_chance::analysis::analyze(&input);
    if let Some(result) = &result {
        tracing::debug!(
            eligible_hits = result.eligible_hits,
            set_fire_ribbons = result.set_fire_ribbons,
            fires = result.fires,
            unattributed_fires = result.unattributed_fires,
            unattributed_reasons = ?result.unattributed_reasons,
            "computed effective fire chance"
        );
    }
    result
}

#[allow(non_camel_case_types)]
pub struct UiReport {
    match_timestamp: Timestamp,
    /// The replay's game version, used to select version-specific parsing (e.g.
    /// the captain-skill translation key style).
    version: wowsunpack::data::Version,
    self_player: Option<Arc<Player>>,
    /// Salts the row-expansion animation ids so expanding a row in one
    /// report's table does not animate the same row number in another.
    /// Assigned from a process-wide counter rather than the replay's arena id
    /// because arena id falls back to a shared sentinel when the arena-state
    /// packet was never observed (spectator recordings, truncated captures).
    report_salt: u64,
    player_reports: Vec<PlayerReport>,
    sorted: bool,
    is_row_expanded: BTreeMap<u64, bool>,
    wows_data: SharedWoWsData,
    twitch_state: Arc<RwLock<crate::twitch::TwitchState>>,
    replay_sort: Arc<Mutex<SortOrder>>,
    columns: Vec<ReplayColumn>,
    row_heights: BTreeMap<u64, f32>,
    background_task_sender: Option<Sender<BackgroundTask>>,
    selected_row: Option<(u64, bool)>,
    debug_mode: bool,
    /// `true` when this UiReport was built from a Replay with merged alt
    /// perspectives. Unmasks columns like enemy builds that are otherwise
    /// hidden out-of-NDA.
    merge_active: bool,
    battle_result: Option<BattleResult>,
    resolved_results: Option<serde_json::Value>,
    /// Ribbon icons from the newest loaded build, used to fill gaps when this
    /// replay's own build ships none (Flash-era and older). See
    /// [`crate::data::wows_data::WoWsDataMap::newest_ribbon_icons`].
    fallback_ribbon_icons: HashMap<String, Arc<GameAsset>>,
    fallback_subribbon_icons: HashMap<String, Arc<GameAsset>>,
    /// Decoded icon textures, cached per build so they stay version-correct
    /// (icons can change between game versions). Mutex for interior mutability
    /// from `&self` while staying Send+Sync.
    icon_textures: Mutex<HashMap<String, egui::TextureHandle>>,
}

/// Source for `UiReport::report_salt`. Every constructed report gets a
/// distinct value, so it stays unique even across reports that share an
/// arena id (or lack one).
static NEXT_REPORT_SALT: AtomicU64 = AtomicU64::new(0);

impl UiReport {
    pub fn new(
        replay_file: &ReplayFile,
        report: &BattleReport,
        wows_data: &SharedWoWsData,
        deps: &crate::data::wows_data::ReplayDependencies,
        merge_active: bool,
    ) -> Self {
        // Captured before locking the replay's build below (sequential, avoids a
        // re-entrant read on the same data). Used to borrow class icons when the
        // replay's own (pre-12.0) build shipped none.
        let fallback_ship_icons = deps.wows_data_map.newest_ship_icons();

        let wows_data_inner = wows_data.read();
        let metadata_provider = wows_data_inner.game_metadata.as_ref().expect("no game metadata?");
        let constants_inner = wows_data_inner.replay_constants.read();

        let match_timestamp = util::replay_timestamp(&replay_file.meta);

        let players = report.players().to_vec();

        let self_player = players.iter().find(|player| player.relation().is_self()).cloned();

        // Computed once for the recording player and attached only to that
        // player's report below: `analyze` already refuses any other attacker,
        // so per-player computation would be wasted work.
        let self_fire_chance = compute_fire_chance(report, metadata_provider, &wows_data_inner, deps);

        let resolved_results: Option<serde_json::Value> = report
            .battle_results()
            .and_then(|s| serde_json::from_str(s).ok())
            .map(|raw| resolve_battle_results(raw, &constants_inner));

        let battle_result = resolved_results.as_ref().and_then(|results| {
            let self_team_id = self_player.as_ref().map(|player| player.initial_state().team_id())?;
            let winning_team_id = results.pointer("/commonList/winner_team_id")?.as_i64()?;

            if winning_team_id == self_team_id {
                Some(BattleResult::Win(self_team_id as i8))
            } else if winning_team_id >= 0 {
                Some(BattleResult::Loss(winning_team_id as i8))
            } else {
                Some(BattleResult::Draw)
            }
        });

        let locale = "en-US";

        // Single source of truth for per-player numbers: the egui-free normalized
        // report. PlayerReport below is presentation rebuilt over these values.
        let normalized = wows_replay_insights::battle_report::NormalizedBattleReport::from_battle_report(
            report,
            &replay_file.meta,
            metadata_provider,
            &constants_inner,
        );

        let self_normalized = normalized.players.iter().find(|np| np.is_self);
        let self_division_id = self_normalized.and_then(|np| np.division_id);
        let self_db_id = self_normalized.map(|np| np.db_id);

        // normalized.players is built positionally from report.players() (plain
        // .map(), no filter/reorder), so zip it against `players` instead of
        // joining on db_id, which is not unique (bots are all AccountId(0)).
        let player_reports: Vec<PlayerReport> = players
            .iter()
            .zip(normalized.players.iter())
            .map(|(player, np)| {
                debug_assert_eq!(player.initial_state().db_id(), np.db_id, "normalized/entity player order drifted");
                let vehicle = player.vehicle_entity();
                let vehicle_param = player.vehicle();
                let server = np.server_results.as_ref();

                let mut tint = PlayerTint::from_relation(np.relation);
                if let (Some(self_div), Some(self_id)) = (self_division_id, self_db_id)
                    && self_id != np.db_id
                    && np.division_id == Some(self_div)
                {
                    tint = PlayerTint::DivisionMate;
                }

                let known_species = vehicle_param.species().and_then(|r| r.known().cloned());
                let ship_species_text: String = known_species
                    .as_ref()
                    .and_then(|species| {
                        metadata_provider
                            .localized_name_from_id(&TranslationKey::new(species.translation_id()))
                            .or_else(|| Some(species.name().to_string()))
                    })
                    .unwrap_or_default();
                let icon = known_species.as_ref().and_then(|species| {
                    ship_class_icon_from_species(*species, &wows_data_inner)
                        .or_else(|| fallback_ship_icons.get(species).cloned())
                });

                let (clan_tag, clan_color) = if !np.clan.is_empty() {
                    (Some(np.clan.clone()), clan_color_for_player(player))
                } else {
                    (None, None)
                };
                let name_text = np.display_name.clone();

                let (base_xp, base_xp_text) = match server.and_then(|sr| sr.xp) {
                    Some(base_xp) => (Some(base_xp), Some(separate_number(base_xp, Some(locale)))),
                    None => (None, None),
                };

                let (raw_xp, raw_xp_text) = match server.and_then(|sr| sr.raw_xp) {
                    Some(raw_xp) => (Some(raw_xp), Some(separate_number(raw_xp, Some(locale)))),
                    None => (None, None),
                };

                let observed_damage = np.observed_results.damage;
                let observed_damage_text = separate_number(observed_damage, Some(locale));

                let (actual_damage, actual_damage_report, actual_damage_text, actual_damage_hover_text) = match server {
                    Some(sr) if sr.damage.is_some() => {
                        let damage_number = sr.damage.expect("damage present");
                        let text = separate_number(damage_number, Some(locale));
                        let hover = RichText::new(breakdown_hover_string(&DAMAGE_DESCRIPTIONS, locale, |key| {
                            sr.damage_by_type.get(key).copied().unwrap_or(0)
                        }))
                        .font(FontId::monospace(12.0));
                        (Some(damage_number), Some(sr.damage_details.clone()), Some(text), Some(hover))
                    }
                    _ => (None, None, None, None),
                };

                let (hits, hits_report, hits_text, hits_hover_text) = match server {
                    Some(sr) => {
                        let hits_number = sr.hits.unwrap_or(0);
                        let text = separate_number(hits_number, Some(locale));
                        let hover = RichText::new(breakdown_hover_string(&HITS_DESCRIPTIONS, locale, |key| {
                            sr.hits_by_type.get(key).copied().unwrap_or(0)
                        }))
                        .font(FontId::monospace(12.0));
                        (sr.hits, Some(sr.hits_details.clone()), Some(text), Some(hover))
                    }
                    None => (None, None, None, None),
                };

                let (received_damage, received_damage_text, received_damage_hover_text, received_damage_report) =
                    match server {
                        Some(sr) => {
                            let total = sr.received_damage;
                            let text = separate_number(total, Some(locale));
                            let hover =
                                RichText::new(breakdown_hover_string(&RECEIVED_DAMAGE_DESCRIPTIONS, locale, |key| {
                                    sr.received_damage_by_type.get(key).copied().unwrap_or(0)
                                }))
                                .font(FontId::monospace(12.0));
                            (Some(total), Some(text), Some(hover), Some(sr.received_damage_details.clone()))
                        }
                        None => (None, None, None, None),
                    };

                // Spotting: prefer server scouting_damage, fall back to the self
                // player's controller total. Hover is always the controller breakdown.
                let (spotting_damage, spotting_damage_text, spotting_damage_hover_text) = if let Some(damage_number) =
                    server.and_then(|sr| sr.spotting_damage)
                {
                    let hover = if np.is_self {
                        build_damage_stat_hover_text(report.self_damage_stats(), DamageStatCategory::Spot, locale)
                    } else {
                        None
                    };
                    (Some(damage_number), Some(separate_number(damage_number, Some(locale))), hover)
                } else if np.is_self {
                    match np.controller_spotting_damage {
                        Some(total) => (
                            Some(total),
                            Some(separate_number(total, Some(locale))),
                            build_damage_stat_hover_text(report.self_damage_stats(), DamageStatCategory::Spot, locale),
                        ),
                        None => (None, None, None),
                    }
                } else {
                    (None, None, None)
                };

                let (potential_damage, potential_damage_text, potential_damage_hover_text, potential_damage_report) =
                    match server {
                        Some(sr) => {
                            let total = sr.potential_damage;
                            let art = sr.potential_damage_details.artillery;
                            let tpd = sr.potential_damage_details.torpedoes;
                            let air = sr.potential_damage_details.planes;
                            // Depth-charge agro is the only potential key the 3-field
                            // report drops; recover it from the total so the hover keeps
                            // its line (total == art + tpd + air + dbomb by construction).
                            let dbomb = total.saturating_sub(art + tpd + air);
                            let hover =
                                RichText::new(breakdown_hover_string(&POTENTIAL_DAMAGE_DESCRIPTIONS, locale, |key| {
                                    match key {
                                        "agro_art" => art,
                                        "agro_tpd" => tpd,
                                        "agro_air" => air,
                                        "agro_dbomb" => dbomb,
                                        _ => 0,
                                    }
                                }))
                                .font(FontId::monospace(12.0));
                            (
                                Some(total),
                                Some(separate_number(total, Some(locale))),
                                Some(hover),
                                Some(sr.potential_damage_details.clone()),
                            )
                        }
                        None => {
                            if np.is_self {
                                match np.controller_potential_damage {
                                    Some(total) => (
                                        Some(total),
                                        Some(separate_number(total, Some(locale))),
                                        build_damage_stat_hover_text(
                                            report.self_damage_stats(),
                                            DamageStatCategory::Agro,
                                            locale,
                                        ),
                                        None,
                                    ),
                                    None => (None, None, None, None),
                                }
                            } else {
                                (None, None, None, None)
                            }
                        }
                    };

                let time_lived_secs = np.time_lived_secs;
                let time_lived_text = time_lived_secs.map(|secs| format!("{}:{:02}", secs / 60, secs % 60));

                // Skill counts come from the normalized report; the label and hover
                // still need the entity's commander-skill list.
                let species = vehicle_param.species().and_then(|r| r.known()).cloned().expect("ship has no species?");
                let (tier, label_text, hover_text) = util::colorize_captain_points(
                    np.skill_info.skill_points,
                    np.skill_info.num_skills,
                    np.skill_info.highest_tier,
                    np.skill_info.num_tier_1_skills,
                    vehicle.and_then(|v| v.commander_skills(species)),
                );
                let skill_info = SkillInfo {
                    skill_points: np.skill_info.skill_points,
                    num_skills: np.skill_info.num_skills,
                    highest_tier: np.skill_info.highest_tier,
                    num_tier_1_skills: np.skill_info.num_tier_1_skills,
                    hover_text,
                    label_text,
                    tier,
                };

                // `fires_dealt` is `Some` exactly when the resolved object carried an
                // interactions map, matching the old `interactions`-key gate.
                let (damage_interactions, fires, floods, citadels, crits) = match server {
                    Some(sr) if sr.fires_dealt.is_some() => {
                        let interactions: HashMap<AccountId, DamageInteraction> = sr
                            .damage_interactions
                            .iter()
                            .map(|(id, interaction)| (*id, damage_interaction_from_normalized(interaction, locale)))
                            .collect();
                        (Some(interactions), sr.fires_dealt, sr.floods_dealt, sr.citadels_dealt, sr.crits_dealt)
                    }
                    _ => (None, None, None, None, None),
                };

                let distance_traveled = server.and_then(|sr| sr.distance_traveled);
                let kills = server.and_then(|sr| sr.kills);

                let achievements: Vec<Achievement> = np
                    .achievements
                    .iter()
                    .filter_map(|achievement| {
                        let game_param = <GameMetadataProvider as GameParamProvider>::game_param_by_name(
                            metadata_provider,
                            &achievement.name,
                        )?;
                        Some(Achievement {
                            game_param,
                            display_name: achievement.display_name.clone(),
                            description: achievement.description.clone(),
                            icon_key: achievement.icon_key.clone(),
                            count: achievement.count,
                        })
                    })
                    .collect();

                let ribbons: HashMap<String, models::Ribbon> = np
                    .ribbons
                    .iter()
                    .map(|ribbon| {
                        (
                            ribbon.name.clone(),
                            models::Ribbon {
                                name: ribbon.name.clone(),
                                display_name: ribbon.display_name.clone(),
                                description: ribbon.description.clone(),
                                icon_key: ribbon.icon_key.clone(),
                                is_subribbon: ribbon.is_subribbon,
                                count: ribbon.count,
                            },
                        )
                    })
                    .collect();

                let consumables: Vec<models::PlayerConsumable> = np
                    .consumables
                    .iter()
                    .map(|consumable| models::PlayerConsumable {
                        display_name: consumable.display_name.clone(),
                        description: consumable.description.clone(),
                        icon_key: consumable.icon_key.clone(),
                        charges_used: consumable.charges_used,
                        total_charges: consumable.total_charges,
                    })
                    .collect();

                PlayerReport {
                    player: Arc::clone(player),
                    tint,
                    is_abuser: np.is_abuser,
                    name_text,
                    clan_tag,
                    clan_color,
                    icon,
                    division_label: np.division_label.clone(),
                    base_xp,
                    base_xp_text,
                    raw_xp,
                    raw_xp_text,
                    observed_damage,
                    observed_damage_text,
                    actual_damage,
                    actual_damage_report,
                    actual_damage_text,
                    actual_damage_hover_text,
                    ship_name: np.ship_name.clone(),
                    spotting_damage,
                    spotting_damage_text,
                    spotting_damage_hover_text,
                    potential_damage,
                    potential_damage_hover_text,
                    potential_damage_report,
                    time_lived_secs,
                    time_lived_text,
                    skill_info,
                    potential_damage_text,
                    ship_species_text,
                    received_damage,
                    received_damage_text,
                    received_damage_hover_text,
                    fires,
                    floods,
                    citadels,
                    crits,
                    distance_traveled,
                    is_test_ship: np.is_test_ship,
                    relation: np.relation,
                    manual_stat_hide_toggle: false,
                    received_damage_report,
                    kills,
                    observed_kills: np.observed_results.kills,
                    translated_build: np.build.clone(),
                    hits,
                    hits_report,
                    hits_text,
                    hits_hover_text,
                    damage_interactions,
                    achievements,
                    ribbons,
                    consumables,
                    heal_count: np.heal_count,
                    personal_rating: None,
                    has_vehicle_entity: vehicle.is_some(),
                    fire_chance: np.is_self.then(|| self_fire_chance.clone()).flatten(),
                }
            })
            .collect();

        drop(constants_inner);
        drop(wows_data_inner);

        Self {
            match_timestamp,
            version: report.version(),
            report_salt: NEXT_REPORT_SALT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            player_reports,
            self_player,
            replay_sort: Arc::clone(&deps.replay_sort),
            wows_data: Arc::clone(wows_data),
            twitch_state: Arc::clone(&deps.twitch_state),
            battle_result,
            is_row_expanded: Default::default(),
            sorted: false,
            columns: vec![
                ReplayColumn::Actions,
                ReplayColumn::Name,
                ReplayColumn::ShipName,
                ReplayColumn::PersonalRating,
                ReplayColumn::BaseXp,
                ReplayColumn::RawXp,
                ReplayColumn::Kills,
                ReplayColumn::ObservedDamage,
                ReplayColumn::ActualDamage,
                ReplayColumn::Hits,
                ReplayColumn::Heals,
                ReplayColumn::ReceivedDamage,
                ReplayColumn::PotentialDamage,
                ReplayColumn::SpottingDamage,
                ReplayColumn::TimeLived,
                ReplayColumn::DistanceTraveled,
                ReplayColumn::Skills,
            ],
            row_heights: Default::default(),
            background_task_sender: Some(deps.background_task_sender.clone()),
            selected_row: None,
            debug_mode: deps.is_debug_mode,
            merge_active,
            resolved_results,
            fallback_ribbon_icons: deps.wows_data_map.newest_ribbon_icons(),
            fallback_subribbon_icons: deps.wows_data_map.newest_subribbon_icons(),
            icon_textures: Mutex::new(HashMap::new()),
        }
    }

    fn sort_players(&mut self, sort_order: SortOrder) {
        let self_player_team_id = self.self_player.as_ref().expect("no self player?").initial_state().team_id();

        let sort_key = |report: &PlayerReport, column: &SortColumn| {
            let player = report.player();
            let player_state = player.initial_state();
            let team_id = player_state.team_id() != self_player_team_id;
            let db_id = player_state.db_id().raw();

            let key = match column {
                SortColumn::Name => SortKey::String(player_state.username().to_string()),
                SortColumn::BaseXp => SortKey::I64(report.base_xp),
                SortColumn::RawXp => SortKey::I64(report.raw_xp),
                SortColumn::ShipName => SortKey::String(report.ship_name.clone()),
                SortColumn::ShipClass => SortKey::Species(
                    player.vehicle().species().and_then(|r| r.known()).cloned().expect("no species for vehicle?"),
                ),
                SortColumn::ObservedDamage => SortKey::U64(Some(if report.should_hide_stats() && !self.debug_mode {
                    0
                } else {
                    report.observed_damage
                })),
                SortColumn::ActualDamage => SortKey::U64(if report.should_hide_stats() && !self.debug_mode {
                    None
                } else {
                    report.actual_damage
                }),
                SortColumn::SpottingDamage => SortKey::U64(report.spotting_damage),
                SortColumn::PotentialDamage => SortKey::U64(if report.should_hide_stats() && !self.debug_mode {
                    None
                } else {
                    report.potential_damage
                }),
                SortColumn::TimeLived => SortKey::U64(report.time_lived_secs),
                SortColumn::Fires => {
                    SortKey::U64(if report.should_hide_stats() && !self.debug_mode { None } else { report.fires })
                }
                SortColumn::Floods => {
                    SortKey::U64(if report.should_hide_stats() && !self.debug_mode { None } else { report.floods })
                }
                SortColumn::Citadels => {
                    SortKey::U64(if report.should_hide_stats() && !self.debug_mode { None } else { report.citadels })
                }
                SortColumn::Crits => {
                    SortKey::U64(if report.should_hide_stats() && !self.debug_mode { None } else { report.crits })
                }
                SortColumn::ReceivedDamage => SortKey::U64(if report.should_hide_stats() && !self.debug_mode {
                    None
                } else {
                    report.received_damage
                }),
                SortColumn::DistanceTraveled => SortKey::F64(report.distance_traveled),
                SortColumn::Kills => SortKey::I64(report.kills.or(Some(report.observed_kills))),
                SortColumn::Hits => {
                    SortKey::U64(if report.should_hide_stats() && !self.debug_mode { None } else { report.hits })
                }
                SortColumn::Heals => SortKey::U64(report.heal_count.map(|c| c as u64)),
                SortColumn::PersonalRating => SortKey::F64(report.personal_rating.as_ref().map(|pr| pr.pr)),
            };

            (team_id, key, db_id)
        };

        match sort_order {
            SortOrder::Desc(column) => {
                self.player_reports.sort_unstable_by_key(|report| {
                    let key = sort_key(report, &column);
                    (key.0, Reverse(key.1), key.2)
                });
            }
            SortOrder::Asc(column) => {
                self.player_reports.sort_unstable_by_key(|report| sort_key(report, &column));
            }
        }

        self.sorted = true;
    }

    fn update_visible_columns(&mut self, settings: &ReplaySettings) {
        let optional_columns = [
            (ReplayColumn::RawXp, settings.show_raw_xp),
            (ReplayColumn::ObservedDamage, settings.show_observed_damage),
            (ReplayColumn::Heals, settings.show_heals),
            (ReplayColumn::ReceivedDamage, settings.show_received_damage),
            (ReplayColumn::DistanceTraveled, settings.show_distance_traveled),
        ];

        let mut optional_columns: HashMap<ReplayColumn, bool> = optional_columns.iter().copied().collect();

        let mut remove_columns = Vec::with_capacity(optional_columns.len());
        // For each column in our existing set, check to see if it's been disabled.
        // If so,
        for (i, column) in self.columns.iter().enumerate() {
            if optional_columns.contains_key(column)
                && let Some(false) = optional_columns.remove(column)
            {
                remove_columns.push(i);
            }
        }

        // Remove columns in reverse order so that we don't invalidate indices
        for i in remove_columns.into_iter().rev() {
            self.columns.remove(i);
        }

        // The optional_columns set above is the remaining columns which are enabled,
        // but not in the existing set, or disabled and not in the existing set. Add the former.
        for (column, enabled) in optional_columns {
            if enabled {
                self.columns.push(column);
            }
        }

        // Finally, sort the remaining columns by their order in the enum.
        self.columns.sort_unstable_by_key(|column| *column as u8);
    }

    fn received_damage_details(&self, report: &PlayerReport, ui: &mut egui::Ui) {
        let style = Style::default();
        let alt_held = ui.input(|i| i.modifiers.alt);

        ui.vertical(|ui| {
            if let Some(received_hover_text) = report.received_damage_hover_text() {
                ui.label(received_hover_text.clone());

                if report.damage_interactions.is_some() {
                    ui.separator();
                }
            }

            if let Some(interactions) = report.damage_interactions.as_ref() {
                // TODO: this sucks, it allocates for each sort
                for interaction in
                    interactions.iter().sorted_by(|a, b| Ord::cmp(&b.1.damage_received, &a.1.damage_received))
                {
                    if interaction.1.damage_received == 0 {
                        continue;
                    }

                    let Some(interaction_player) = self
                        .player_reports()
                        .iter()
                        .find(|report| report.player().initial_state().db_id() == *interaction.0)
                    else {
                        // TODO: Handle bots?
                        continue;
                    };

                    // Build hover text with clan tag and player name
                    let mut hover_layout = LayoutJob::default();
                    if let Some(clan_text) = interaction_player.clan_text(ui.visuals()) {
                        clan_text.append_to(
                            &mut hover_layout,
                            &style,
                            egui::FontSelection::Default,
                            egui::Align::Center,
                        );
                        hover_layout.append(" ", 0.0, Default::default());
                    }
                    interaction_player.name_text(ui.visuals()).append_to(
                        &mut hover_layout,
                        &style,
                        egui::FontSelection::Default,
                        egui::Align::Center,
                    );

                    // ALT: show % of attacker's total dealt damage; default: % of this player's received damage
                    let pct_text = if alt_held {
                        &interaction.1.damage_received_inverse_percentage_text
                    } else {
                        &interaction.1.damage_received_percentage_text
                    };

                    let resp = ui.label(format!(
                        "{}: {} ({})",
                        interaction_player.ship_name(),
                        interaction.1.damage_received_text,
                        pct_text
                    ));
                    if interaction.1.damage_received_hover_text.is_empty() {
                        resp.on_hover_text(hover_layout);
                    } else {
                        resp.on_hover_ui(|ui| {
                            ui.label(hover_layout);
                            ui.separator();
                            ui.label(
                                RichText::new(&interaction.1.damage_received_hover_text).font(FontId::monospace(12.0)),
                            );
                        });
                    }
                }
            };
        });
    }

    fn render_skill_grid(&self, ui: &mut egui::Ui, rows: &[wowsunpack::game_params::skill_grid_data::SkillGridRow]) {
        const ICON_SIZE: f32 = 28.0;
        let all_skills: Vec<&wowsunpack::game_params::skill_grid_data::SkillGridSkill> =
            rows.iter().flat_map(|r| r.skills.iter()).collect();
        let icons: Vec<Option<Arc<GameAsset>>> = {
            let wows_data = self.wows_data.read();
            all_skills.iter().map(|s| wows_data.cached_crew_skill_icon(&s.internal_name)).collect()
        };
        let icons: Vec<Option<Arc<GameAsset>>> = if icons.iter().any(|i| i.is_none()) {
            let mut wows_data = self.wows_data.write();
            all_skills
                .iter()
                .zip(icons)
                .map(|(s, cached)| cached.or_else(|| wows_data.load_crew_skill_icon(&s.internal_name)))
                .collect()
        } else {
            icons
        };
        let mut icon_idx = 0;
        for row in rows {
            ui.horizontal(|ui| {
                let row_label = row.point_cost.map(|c| c.get().to_string()).unwrap_or_default();
                ui.add_sized([14.0, ICON_SIZE], egui::Label::new(RichText::new(row_label).weak().small()));
                for skill in &row.skills {
                    let icon = &icons[icon_idx];
                    icon_idx += 1;
                    let display_name = skill.name.clone().unwrap_or_else(|| skill.internal_name.to_string());
                    let cost = skill.point_cost.map(|c| format!(" ({} pt)", c.get())).unwrap_or_default();
                    let tooltip = match skill.description.as_deref() {
                        Some(desc) if !desc.is_empty() => format!("{}{}\n\n{}", display_name, cost, desc),
                        _ => format!("{}{}", display_name, cost),
                    };
                    match icon {
                        Some(icon) => {
                            let tint = if skill.learned { ui.sem().text_strong } else { ui.sem().text_dim };
                            if let Some(tex) = self.icon_texture(ui.ctx(), icon) {
                                let image =
                                    egui::Image::new((tex.id(), egui::Vec2::new(ICON_SIZE, ICON_SIZE))).tint(tint);
                                ui.add(image).on_hover_text(tooltip);
                            } else {
                                // Fallback for builds whose skill icons are absent.
                                let label = match skill.point_cost {
                                    Some(c) => format!("({}) {}", c.get(), display_name),
                                    None => display_name,
                                };
                                let mut text = RichText::new(label);
                                if !skill.learned {
                                    text = text.weak();
                                }
                                ui.label(text).on_hover_text(tooltip);
                            }
                        }
                        None => {
                            // Fallback for builds whose skill icons are absent.
                            let label = match skill.point_cost {
                                Some(c) => format!("({}) {}", c.get(), display_name),
                                None => display_name,
                            };
                            let mut text = RichText::new(label);
                            if !skill.learned {
                                text = text.weak();
                            }
                            ui.label(text).on_hover_text(tooltip);
                        }
                    }
                }
            });
        }
    }

    /// Decode + cache an icon's texture once per build. The cache key and the
    /// egui texture name include the build so icons from different game versions
    /// stay distinct. Returns None if the bytes fail to decode.
    fn icon_texture(&self, ctx: &egui::Context, asset: &GameAsset) -> Option<egui::TextureHandle> {
        let key = format!("{:?}:{}", self.version.build, asset.path);
        if let Some(tex) = self.icon_textures.lock().get(&key) {
            return Some(tex.clone());
        }
        let img = image::load_from_memory(&asset.data).ok()?.to_rgba8();
        let size = [img.width() as usize, img.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
        let tex = ctx.load_texture(key.clone(), color, egui::TextureOptions::LINEAR);
        self.icon_textures.lock().insert(key, tex.clone());
        Some(tex)
    }

    fn render_modernization_slots(&self, ui: &mut egui::Ui, slots: &[Option<models::TranslatedModule>]) {
        const ICON_SIZE: f32 = 28.0;
        let cached: Vec<Option<Arc<GameAsset>>> = {
            let wows_data = self.wows_data.read();
            slots
                .iter()
                .map(|s| s.as_ref().and_then(|m| wows_data.cached_modernization_icon(&m.game_params_name)))
                .collect()
        };
        let icons: Vec<Option<Arc<GameAsset>>> = if slots.iter().zip(&cached).any(|(s, c)| s.is_some() && c.is_none()) {
            let mut wows_data = self.wows_data.write();
            slots
                .iter()
                .zip(cached)
                .map(|(s, c)| match s {
                    Some(m) => c.or_else(|| wows_data.load_modernization_icon(&m.game_params_name)),
                    None => None,
                })
                .collect()
        } else {
            cached
        };
        ui.horizontal_wrapped(|ui| {
            for (slot, icon) in slots.iter().zip(icons) {
                match slot {
                    Some(module) => {
                        let display_name = module.name.clone().unwrap_or_else(|| module.game_params_name.clone());
                        let tooltip = match module.description.as_deref() {
                            Some(d) if !d.is_empty() => format!("{}\n\n{}", display_name, d),
                            _ => display_name.clone(),
                        };
                        match icon.as_ref().and_then(|a| self.icon_texture(ui.ctx(), a)) {
                            Some(tex) => {
                                ui.add(egui::Image::new((tex.id(), egui::Vec2::splat(ICON_SIZE))))
                                    .on_hover_text(tooltip);
                            }
                            None => {
                                ui.label(RichText::new(&display_name).small()).on_hover_text(tooltip);
                            }
                        }
                    }
                    None => {
                        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(ICON_SIZE), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, ui.visuals().faint_bg_color);
                    }
                }
            }
        });
    }

    fn render_signals(&self, ui: &mut egui::Ui, signals: &[models::TranslatedModule]) {
        const ICON_SIZE: f32 = 28.0;
        let cached: Vec<Option<Arc<GameAsset>>> = {
            let wows_data = self.wows_data.read();
            signals.iter().map(|m| wows_data.cached_signal_flag_icon(&m.game_params_name)).collect()
        };
        let icons: Vec<Option<Arc<GameAsset>>> = if cached.iter().any(|c| c.is_none()) {
            let mut wows_data = self.wows_data.write();
            signals
                .iter()
                .zip(cached)
                .map(|(m, c)| c.or_else(|| wows_data.load_signal_flag_icon(&m.game_params_name)))
                .collect()
        } else {
            cached
        };
        ui.horizontal_wrapped(|ui| {
            for (signal, icon) in signals.iter().zip(icons) {
                let display_name = signal.name.clone().unwrap_or_else(|| signal.game_params_name.clone());
                let tooltip = match signal.description.as_deref() {
                    Some(d) if !d.is_empty() => format!("{}\n\n{}", display_name, d),
                    _ => display_name.clone(),
                };
                match icon.as_ref().and_then(|a| self.icon_texture(ui.ctx(), a)) {
                    Some(tex) => {
                        ui.add(egui::Image::new((tex.id(), egui::Vec2::splat(ICON_SIZE)))).on_hover_text(tooltip);
                    }
                    None => {
                        ui.label(RichText::new(&display_name).small()).on_hover_text(tooltip);
                    }
                }
            }
        });
    }

    fn render_consumable_inventory(&self, ui: &mut egui::Ui, consumables: &[models::PlayerConsumable]) {
        const NAME_COL_WIDTH: f32 = 170.0;
        const COUNT_COL_WIDTH: f32 = 64.0;
        const ICON_SIZE: f32 = 20.0;
        const ROW_HEIGHT: f32 = 22.0;

        let icons: Vec<Option<Arc<GameAsset>>> = {
            let wows_data = self.wows_data.read();
            consumables.iter().map(|c| wows_data.cached_consumable_icon(&c.icon_key)).collect()
        };
        let icons: Vec<Option<Arc<GameAsset>>> = if icons.iter().any(|i| i.is_none()) {
            let mut wows_data = self.wows_data.write();
            consumables
                .iter()
                .zip(icons)
                .map(|(c, cached)| cached.or_else(|| wows_data.load_consumable_icon(&c.icon_key)))
                .collect()
        } else {
            icons
        };

        ui.horizontal(|ui| {
            ui.add_sized(
                [NAME_COL_WIDTH, ROW_HEIGHT],
                egui::Label::new(RichText::new(t!("ui.replay.consumable_header_consumable")).strong()),
            );
            ui.add_sized(
                [COUNT_COL_WIDTH, ROW_HEIGHT],
                egui::Label::new(RichText::new(t!("ui.replay.consumable_header_remaining")).strong()),
            );
            ui.add_sized(
                [COUNT_COL_WIDTH, ROW_HEIGHT],
                egui::Label::new(RichText::new(t!("ui.replay.consumable_header_total")).strong()),
            );
        });

        for (consumable, icon) in consumables.iter().zip(icons) {
            let (remaining_text, total_text, hover_text) = match consumable.total_charges {
                wowsunpack::game_types::ChargeCount::Unlimited => (
                    consumable.charges_used.to_string(),
                    t!("ui.replay.consumable_charges_infinite").to_string(),
                    t!("ui.replay.consumable_charges_unlimited", used = consumable.charges_used).to_string(),
                ),
                wowsunpack::game_types::ChargeCount::Finite(total) => {
                    let remaining = total.saturating_sub(consumable.charges_used);
                    (
                        remaining.to_string(),
                        total.to_string(),
                        t!("ui.replay.consumable_charges_remaining", remaining = remaining, total = total).to_string(),
                    )
                }
            };

            ui.horizontal(|ui| {
                let (name_rect, _) =
                    ui.allocate_exact_size(Vec2::new(NAME_COL_WIDTH, ROW_HEIGHT), egui::Sense::hover());
                let mut name_ui = ui.new_child(
                    egui::UiBuilder::new().max_rect(name_rect).layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                if let Some(icon) = icon.as_ref()
                    && let Some(tex) = self.icon_texture(name_ui.ctx(), icon)
                {
                    let image = egui::Image::new((tex.id(), egui::Vec2::new(ICON_SIZE, ICON_SIZE)));
                    let response = name_ui.add(image);
                    if !consumable.description.is_empty() {
                        response.on_hover_text(&consumable.description);
                    }
                }
                let name_label = name_ui.label(&consumable.display_name);
                if !consumable.description.is_empty() {
                    name_label.on_hover_text(&consumable.description);
                }

                ui.add_sized([COUNT_COL_WIDTH, ROW_HEIGHT], egui::Label::new(&remaining_text))
                    .on_hover_text(&hover_text);
                ui.add_sized([COUNT_COL_WIDTH, ROW_HEIGHT], egui::Label::new(&total_text)).on_hover_text(&hover_text);
            });
        }
    }

    fn dealt_damage_details(&self, report: &PlayerReport, ui: &mut egui::Ui) {
        let style = Style::default();
        let alt_held = ui.input(|i| i.modifiers.alt);

        ui.vertical(|ui| {
            if let Some(received_hover_text) = report.actual_damage_hover_text() {
                ui.label(received_hover_text.clone());

                if report.damage_interactions.is_some() {
                    ui.separator();
                }
            }

            if let Some(interactions) = report.damage_interactions.as_ref() {
                // TODO: this sucks, it allocates for each sort
                for interaction in interactions.iter().sorted_by(|a, b| Ord::cmp(&b.1.damage_dealt, &a.1.damage_dealt))
                {
                    if interaction.1.damage_dealt == 0 {
                        continue;
                    }

                    let Some(interaction_player) = self
                        .player_reports()
                        .iter()
                        .find(|report| report.player().initial_state().db_id() == *interaction.0)
                    else {
                        // In co-op, you may not have an interaction
                        continue;
                    };

                    // Build hover text with clan tag and player name
                    let mut hover_layout = LayoutJob::default();
                    if let Some(clan_text) = interaction_player.clan_text(ui.visuals()) {
                        clan_text.append_to(
                            &mut hover_layout,
                            &style,
                            egui::FontSelection::Default,
                            egui::Align::Center,
                        );
                        hover_layout.append(" ", 0.0, Default::default());
                    }
                    interaction_player.name_text(ui.visuals()).append_to(
                        &mut hover_layout,
                        &style,
                        egui::FontSelection::Default,
                        egui::Align::Center,
                    );

                    // ALT: show % of victim's total received damage; default: % of this player's dealt damage
                    let pct_text = if alt_held {
                        &interaction.1.damage_dealt_inverse_percentage_text
                    } else {
                        &interaction.1.damage_dealt_percentage_text
                    };

                    let resp = ui.label(format!(
                        "{}: {} ({})",
                        interaction_player.ship_name(),
                        interaction.1.damage_dealt_text,
                        pct_text
                    ));
                    if interaction.1.damage_dealt_hover_text.is_empty() {
                        resp.on_hover_text(hover_layout);
                    } else {
                        resp.on_hover_ui(|ui| {
                            ui.label(hover_layout);
                            ui.separator();
                            ui.label(
                                RichText::new(&interaction.1.damage_dealt_hover_text).font(FontId::monospace(12.0)),
                            );
                        });
                    }
                }
            };
        });
    }

    /// Render the effective fire chance block for the recording player's row:
    /// a clickable headline, a breakdown expander, and a per-target-ship
    /// expander. `report.fire_chance` is `None` for every other row, so callers
    /// only reach this once per replay.
    ///
    /// Both expanders start collapsed. This block is one row of a table of
    /// players, so it has to stay a couple of lines tall until the reader asks
    /// for more; the hover carries only the copy hint, since the breakdown it
    /// used to hold is longer than a tooltip can usefully be.
    ///
    /// `hide_stats` replaces the whole block with the NDA placeholder, as the
    /// sibling sections do: the breakdown carries the ship's raw `burnProb`,
    /// every modifier its build applies and its per-target rates, which is the
    /// data the test-ship hide exists to cover.
    fn render_fire_chance(&self, ui: &mut egui::Ui, fire_chance: &EffectiveFireChance, hide_stats: bool) {
        ui.strong(t!("ui.replay.sections.fire_chance"));
        if hide_stats {
            ui.label(t!("ui.replay.nda"));
            return;
        }

        // Laid out rather than space-padded: the figures carry their own
        // emphasis and every listing under them gets real columns, so none of
        // it depends on a fixed-width font to line up. The formula keeps
        // monospace because it genuinely is a fixed-width listing.
        // Vertical: the counts on one line and the context beneath, so the
        // block stays narrow. The name column clips rather than wraps, so a
        // single wide line loses its tail.
        let headline = ui
            .vertical(|ui| {
                // Labels are selectable by default, which makes each one consume
                // the pointer and leaves the row's own response reachable only
                // through the gaps between them. The hover and click belong to
                // the row as a whole, so the parts must not sense input.
                let plain = |ui: &mut egui::Ui, text: RichText| {
                    ui.add(Label::new(text).selectable(false));
                };
                if fire_chance.eligible_hits == 0 {
                    plain(ui, RichText::new(t!("ui.replay.sections.fire_chance_no_eligible_hits")).weak());
                } else {
                    // The flame is what says the leading figure is fires; the
                    // clipboard form spells it out in words instead, since an
                    // icon-font codepoint pasted elsewhere is a blank box.
                    // Observed and expected sit together: comparing them is the
                    // point of the statistic, so they must not be split apart.
                    ui.horizontal(|ui| {
                        plain(
                            ui,
                            RichText::new(wt_translations::icon_t(
                                icons::FIRE,
                                &fire_chance_counts_text(fire_chance.fires, fire_chance.eligible_hits),
                            ))
                            .strong(),
                        );
                        if let Some(expected) = fire_chance.expected_fires {
                            plain(
                                ui,
                                RichText::new(t!(
                                    "ui.replay.sections.fire_chance_expected_fires",
                                    fires = format!("{expected:.1}")
                                ))
                                .weak(),
                            );
                        }
                    });
                    plain(ui, RichText::new(fire_chance_ships_text(fire_chance)).weak());
                }
            })
            .response
            .interact(Sense::click())
            .on_hover_text(t!("ui.replay.sections.fire_chance_click_to_copy"));

        if headline.clicked() {
            ui.ctx().copy_text(self.fire_chance_copy_text(fire_chance));
            let _ = self.background_task_sender.as_ref().map(|sender| {
                sender.send(BackgroundTask {
                    receiver: None,
                    kind: BackgroundTaskKind::UpdateTimedMessage(ToastMessage::success(t!(
                        "ui.replay.sections.fire_chance_copied"
                    ))),
                })
            });
        }

        egui::CollapsingHeader::new(t!("ui.replay.sections.fire_chance_breakdown")).show(ui, |ui| {
            let formula = fire_chance_formula_lines(fire_chance, &|source| self.localize_modifier_source(source));
            if !formula.is_empty() {
                ui.label(RichText::new(formula.join("\n")).monospace());
                ui.add_space(4.0);
            }
            fire_chance_rows_ui(ui, "fire_chance_tally", &fire_chance_battle_tally_rows(fire_chance));
            let ribbons = fire_chance_ribbon_rows(fire_chance);
            if !ribbons.is_empty() {
                ui.add_space(4.0);
                fire_chance_rows_ui(ui, "fire_chance_ribbons", &ribbons);
            }
        });

        if !fire_chance.per_ship.is_empty() {
            egui::CollapsingHeader::new(t!("ui.replay.sections.fire_chance_per_ship")).show(ui, |ui| {
                // Two rows per ship: the name and its rate on one, the counts
                // indented beneath. One line carrying all four would set the
                // width of the name column, which clips rather than wraps.
                egui::Grid::new(ui.id().with("fire_chance_per_ship"))
                    .num_columns(3)
                    .spacing([12.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for ship in sorted_per_ship(fire_chance) {
                            ui.label(self.localize_ship_name(ship));
                            match ship.rate() {
                                Some(rate) => ui.label(format!("{:.1}%", rate * 100.0)),
                                None => ui.weak(t!("ui.replay.sections.fire_chance_no_eligible_hits")),
                            };
                            match ship.expected_rate() {
                                Some(expected) => ui.weak(format!(
                                    "{} {:.1}%",
                                    t!("ui.replay.sections.fire_chance_expected"),
                                    expected * 100.0
                                )),
                                None => ui.label(""),
                            };
                            ui.end_row();

                            if ship.rate().is_some() {
                                ui.label("");
                                ui.weak(wt_translations::icon_t(
                                    icons::FIRE,
                                    &fire_chance_counts_text(ship.fires, ship.eligible_hits),
                                ));
                                ui.label("");
                                ui.end_row();
                            }
                        }
                        // The rows do not otherwise add up to the totals above
                        // them, because a hit keyed to our own ship or to a
                        // player whose hull never resolved has no row to sit in.
                        if let Some(line) = fire_chance_no_target_ship_line(fire_chance) {
                            ui.weak(line);
                            ui.label("");
                            ui.label("");
                            ui.end_row();
                        }
                    });
            });
        }
    }

    /// The whole effective-fire-chance breakdown as text: the attacker-side
    /// formula, then what became of our HE shells and of the fire ribbons, then
    /// the same accounting per target ship.
    ///
    /// Built independently of which expanders are open, because this is what
    /// clicking the headline copies and a reader pasting it elsewhere expects
    /// the whole document.
    fn fire_chance_breakdown_text(&self, fire_chance: &EffectiveFireChance) -> String {
        let mut lines = fire_chance_formula_lines(fire_chance, &|source| self.localize_modifier_source(source));
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(fire_chance_breakdown_lines(fire_chance));
        let per_ship = fire_chance_per_ship_lines(fire_chance, &|ship| self.localize_ship_name(ship));
        if !per_ship.is_empty() {
            lines.push(String::new());
            lines.extend(per_ship);
        }
        lines.join("\n")
    }

    /// A victim ship's display name, resolved from its GameParams index.
    ///
    /// The insights crate carries no translations, so it reports the raw index
    /// alongside the raw internal name (`PJSB018_Yamato_1944`). Resolved by
    /// index, the way the armor viewer's ship lookup does it; falls back to the
    /// internal name when the index does not resolve against this build.
    fn localize_ship_name(&self, ship: &PerShipFireChance) -> String {
        let metadata_provider = self.metadata_provider();
        <GameMetadataProvider as GameParamProvider>::game_param_by_index(&metadata_provider, &ship.victim_ship_index)
            .and_then(|param| metadata_provider.localized_name_from_param(&param))
            .unwrap_or_else(|| ship.victim_ship_name.clone())
    }

    /// Best-effort localized name for a formula step's source identifier.
    /// Tries an equipped upgrade or signal's GameParams entry first, then a
    /// crew skill's translation keys (a skill carries no `Param` of its own,
    /// so it cannot be found the first way); falls back to the raw identifier
    /// when neither resolves.
    fn localize_modifier_source(&self, source: &str) -> String {
        let metadata_provider = self.metadata_provider();

        let upgrade_or_signal_name =
            <GameMetadataProvider as GameParamProvider>::game_param_by_name(&metadata_provider, source).and_then(
                |param| {
                    let ctx = wowsunpack::game_params::describe::DescribeContext {
                        resource_loader: metadata_provider.as_ref(),
                        version: &self.version,
                        species: None,
                        param_name: None,
                    };
                    param.display_name(&ctx)
                },
            );
        if let Some(name) = upgrade_or_signal_name {
            return name;
        }

        let skill_name = CrewSkillName::from(source);
        let (primary, fallback) = skill_translation_keys_for(&skill_name, "IDS_SKILL", &self.version);
        let skill_display_name = metadata_provider
            .localized_name_from_id(&TranslationKey::new(primary))
            .or_else(|| metadata_provider.localized_name_from_id(&TranslationKey::new(fallback)));
        if let Some(name) = skill_display_name {
            return name;
        }

        source.to_owned()
    }

    /// The full plain-text breakdown for click-to-copy: the headline, then
    /// everything either expander can show, whether or not it is open.
    fn fire_chance_copy_text(&self, fire_chance: &EffectiveFireChance) -> String {
        let mut lines = vec![t!("ui.replay.sections.fire_chance").into_owned()];
        lines.extend(fire_chance_headline_lines(fire_chance));
        lines.push(String::new());
        lines.push(self.fire_chance_breakdown_text(fire_chance));
        lines.join("\n")
    }

    fn cell_content_ui(&mut self, row_nr: u64, col_nr: usize, ui: &mut egui::Ui) {
        let is_expanded = self.is_row_expanded.get(&row_nr).copied().unwrap_or_default();
        let expandedness = ui.ctx().animate_bool(Id::new(("replay_row", self.report_salt, row_nr)), is_expanded);

        let Some(report) = self.player_reports.get(row_nr as usize) else {
            return;
        };

        let column = *self.columns.get(col_nr).expect("somehow ended up with zero columns?");
        let mut change_expand = false;

        let inner_response = ui.vertical(|ui| {
            ui.horizontal(|ui| {
                // The first column always has the expand/collapse button
                if col_nr == 1 {
                    let (_, response) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::click());
                    egui::collapsing_header::paint_default_icon(ui, expandedness, &response);
                    if response.clicked() {
                        change_expand = true;
                    }
                }

                match column {
                    ReplayColumn::Name => {
                        // Add ship icon
                        if let Some(icon) = report.icon.as_ref() {
                            let image = Image::new(ImageSource::Bytes {
                                uri: icon.path.clone().into(),
                                // the icon size is <1k, this clone is fairly cheap
                                bytes: icon.data.clone().into(),
                            })
                            .tint(report.tint.color(ui.visuals()))
                            .fit_to_exact_size((20.0, 20.0).into())
                            .rotate(90.0_f32.to_radians(), Vec2::splat(0.5));

                            let response = ui.add(image);
                            if !report.ship_species_text.is_empty() {
                                response.on_hover_text(&report.ship_species_text);
                            }
                        } else {
                            ui.label(&report.ship_species_text);
                        }

                        // Add division ID
                        if let Some(div) = report.division_label.as_ref() {
                            ui.label(div);
                        }

                        // Add player clan
                        if let Some(clan_text) = report.clan_text(ui.visuals()) {
                            ui.label(clan_text);
                        }

                        // Add player name
                        ui.label(report.name_text(ui.visuals()));

                        // Add icons for player properties
                        {
                            let player = report.player();
                            // Hidden profile icon
                            if player.initial_state().is_hidden() {
                                ui.label(icons::EYE_SLASH).on_hover_text(t!("ui.replay.player.hidden_profile"));
                            }

                            // Stream sniper icon
                            if let Some(candidates) = self.twitch_state.read().player_is_potential_stream_sniper(
                                player.initial_state().username(),
                                self.match_timestamp,
                            ) && let Some(login) =
                                crate::ui::widgets::twitch_chip(ui, &candidates, self.match_timestamp)
                            {
                                ui.ctx().copy_text(login);
                            }

                            let disconnect_hover_text = if player.connection_change_info().is_empty() {
                                // An empty connection history means either a genuine no-show
                                // (in the roster but never loaded) or that this client just
                                // doesn't broadcast usable connection state (older replays,
                                // e.g. 0.9.10, report everyone as not-connected at the arena
                                // snapshot and never update it). A player who spawned a ship
                                // was obviously connected, so only flag the ones who never did.
                                if player.vehicle_entity().is_some() {
                                    None
                                } else {
                                    Some(t!("ui.replay.player.never_connected").into())
                                }
                            } else if player.connection_change_info().iter().any(|connection_info| {
                                ConnectionChangeKind::Disconnected == connection_info.event_kind()
                                    && !connection_info.had_death_event()
                            }) {
                                let mut event_descriptions = Vec::new();
                                // Skip the first connect event
                                for connection_change in player.connection_change_info().iter().skip(1) {
                                    let secs = connection_change.at_game_duration().as_secs();
                                    let timestamp = format!("{}:{:02}", secs / 60, secs % 60);
                                    match connection_change.event_kind() {
                                        ConnectionChangeKind::Connected => {
                                            event_descriptions.push(format!("connected @ {timestamp}"))
                                        }
                                        ConnectionChangeKind::Disconnected => {
                                            event_descriptions.push(format!("disconnected @ {timestamp}"))
                                        }
                                    }
                                }
                                Some(format!("Player {}", event_descriptions.join(", ")))
                            } else {
                                None
                            };
                            if let Some(disconnect_text) = disconnect_hover_text {
                                ui.label(icons::PLUGS).on_hover_text(disconnect_text);
                            }
                        }
                    }
                    ReplayColumn::BaseXp => {
                        if let Some(base_xp_text) = report.base_xp_text.as_ref() {
                            ui.label(RichText::new(base_xp_text).color(report.tint.color(ui.visuals())));
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::RawXp => {
                        if let Some(raw_xp_text) = report.raw_xp_text.clone() {
                            ui.label(raw_xp_text);
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::ShipName => {
                        ui.label(&report.ship_name);
                    }
                    ReplayColumn::Kills => {
                        if let Some(kills) = report.kills {
                            ui.label(kills.to_string());
                        } else {
                            ui.label(report.observed_kills.to_string());
                        }
                    }
                    ReplayColumn::ObservedDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label(t!("ui.replay.nda"));
                        } else {
                            ui.label(&report.observed_damage_text);
                        }
                    }
                    ReplayColumn::ActualDamage => {
                        if let Some(damage_text) = report.actual_damage_text.as_ref() {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label(t!("ui.replay.nda"));
                            } else {
                                let response =
                                    ui.label(RichText::new(damage_text).color(report.tint.color(ui.visuals())));
                                if report.actual_damage_hover_text().is_some() || report.damage_interactions.is_some() {
                                    let tooltip = Tooltip::for_enabled(&response);
                                    tooltip.show(|ui| {
                                        self.dealt_damage_details(report, ui);
                                    });
                                }
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::ReceivedDamage => {
                        if let Some(received_damage_text) = report.received_damage_text.as_ref() {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label(t!("ui.replay.nda"));
                            } else {
                                let response = ui
                                    .label(RichText::new(received_damage_text).color(report.tint.color(ui.visuals())));
                                if report.received_damage_hover_text().is_some() || report.damage_interactions.is_some()
                                {
                                    let tooltip = Tooltip::for_enabled(&response);
                                    tooltip.show(|ui| {
                                        self.received_damage_details(report, ui);
                                    });
                                }
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::PotentialDamage => {
                        if let Some(damage_text) = report.potential_damage_text.clone() {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label(t!("ui.replay.nda"));
                            } else {
                                let response = ui.label(damage_text);
                                if let Some(hover_text) = report.potential_damage_hover_text.as_ref() {
                                    response.on_hover_text(hover_text.clone());
                                }
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::SpottingDamage => {
                        if let Some(spotting_damage_text) = report.spotting_damage_text.clone() {
                            let response = ui.label(spotting_damage_text);
                            if let Some(hover_text) = report.spotting_damage_hover_text.as_ref() {
                                response.on_hover_text(hover_text.clone());
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::TimeLived => {
                        if let Some(time_lived_text) = report.time_lived_text.clone() {
                            ui.label(time_lived_text);
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::DistanceTraveled => {
                        if let Some(distance) = report.distance_traveled {
                            ui.label(format!("{distance:.2}km"));
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Skills => {
                        if report.relation().is_enemy() && !self.debug_mode {
                            ui.label("-");
                        } else if !report.has_vehicle_entity {
                            ui.label(
                                RichText::new(wt_translations::icon_t(icons::EXCLAMATION_MARK, "-"))
                                    .color(ui.sem().loss),
                            )
                            .on_hover_text(t!("ui.replay.build.not_spotted"));
                        } else {
                            let label = RichText::new(&report.skill_info.label_text)
                                .color(report.skill_info.tier.color(ui.visuals()));
                            let response = ui.label(label);
                            if let Some(hover_text) = &report.skill_info.hover_text {
                                response.on_hover_text(hover_text);
                            }
                        }
                    }
                    ReplayColumn::PersonalRating => {
                        if let Some(pr) = report.personal_rating.as_ref() {
                            crate::ui::widgets::pr_chip(ui, pr.category, &format!("{:.0}", pr.pr), false)
                                .on_hover_text(pr.category.name());
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Actions => {
                        ui.menu_button(icons::DOTS_THREE, |ui| {
                            if (!report.relation().is_enemy() || self.debug_mode) && report.has_vehicle_entity {
                                if ui
                                    .small_button(wt_translations::icon_t(
                                        icons::SHARE,
                                        &t!("ui.replay.build.open_in_browser"),
                                    ))
                                    .clicked()
                                {
                                    let metadata_provider = self.metadata_provider();

                                    if let Some(url) = build_ship_config_url(report.player(), &metadata_provider) {
                                        ui.ctx().open_url(OpenUrl::new_tab(url));
                                    }
                                    ui.close_kind(UiKind::Menu);
                                }

                                if ui
                                    .small_button(wt_translations::icon_t(
                                        icons::COPY,
                                        &t!("ui.replay.build.copy_link"),
                                    ))
                                    .clicked()
                                {
                                    let metadata_provider = self.metadata_provider();

                                    if let Some(url) = build_ship_config_url(report.player(), &metadata_provider) {
                                        ui.ctx().copy_text(url);

                                        let _ = self.background_task_sender.as_ref().map(|sender| {
                                            sender.send(BackgroundTask {
                                                receiver: None,
                                                kind: BackgroundTaskKind::UpdateTimedMessage(ToastMessage::success(
                                                    t!("ui.replay.build.link_copied"),
                                                )),
                                            })
                                        });
                                    }

                                    ui.close_kind(UiKind::Menu);
                                }

                                if ui
                                    .small_button(wt_translations::icon_t(
                                        icons::COPY,
                                        &t!("ui.replay.build.copy_short_link"),
                                    ))
                                    .clicked()
                                {
                                    let metadata_provider = self.metadata_provider();

                                    if let Some(url) = build_short_ship_config_url(report.player(), &metadata_provider)
                                    {
                                        ui.ctx().copy_text(url);
                                        let _ = self.background_task_sender.as_ref().map(|sender| {
                                            sender.send(BackgroundTask {
                                                receiver: None,
                                                kind: BackgroundTaskKind::UpdateTimedMessage(ToastMessage::success(
                                                    t!("ui.replay.build.link_copied"),
                                                )),
                                            })
                                        });
                                    }

                                    ui.close_kind(UiKind::Menu);
                                }

                                ui.separator();
                            }

                            if ui
                                .small_button(wt_translations::icon_t(
                                    icons::SHARE,
                                    &t!("ui.replay.build.open_wows_numbers"),
                                ))
                                .clicked()
                            {
                                if let Some(url) = build_wows_numbers_url(report.player()) {
                                    ui.ctx().open_url(OpenUrl::new_tab(url));
                                }

                                ui.close_kind(UiKind::Menu);
                            }

                            if self.debug_mode {
                                ui.separator();

                                if let Some(player) = Some(report.player())
                                    && ui
                                        .small_button(wt_translations::icon_t(
                                            icons::BUG,
                                            &t!("ui.replay.debug.view_raw_metadata"),
                                        ))
                                        .clicked()
                                {
                                    let pretty_meta =
                                        serde_json::to_string_pretty(player).expect("failed to serialize player");
                                    let viewer = plaintext_viewer::PlaintextFileViewer {
                                        title: Arc::new("metadata.json".to_owned()),
                                        file_info: Arc::new(Mutex::new(FileType::PlainTextFile {
                                            ext: ".json".to_owned(),
                                            contents: pretty_meta,
                                        })),
                                        open: Arc::new(AtomicBool::new(true)),
                                    };

                                    if let Some(sender) = self.background_task_sender.as_ref() {
                                        sender
                                            .send(BackgroundTask {
                                                receiver: None,
                                                kind: BackgroundTaskKind::OpenFileViewer(viewer),
                                            })
                                            .expect("failed to send file viewer task")
                                    }

                                    ui.close_kind(UiKind::Menu);
                                }
                            }
                        });
                    }
                    ReplayColumn::Hits => {
                        if let Some(hits_text) = report.hits_text.as_ref() {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label(t!("ui.replay.nda"));
                            } else {
                                let response =
                                    ui.label(RichText::new(hits_text).color(report.tint.color(ui.visuals())));
                                if let Some(hover_text) = report.hits_hover_text.clone() {
                                    response.on_hover_text(hover_text);
                                }
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Heals => {
                        if let Some(heal_count) = report.heal_count {
                            ui.label(format!("{heal_count}")).on_hover_text(t!("ui.replay.column.heals_tooltip"));
                        } else {
                            ui.label("-").on_hover_text(t!("ui.replay.column.heals_no_repair_tooltip"));
                        }
                    }
                }
            });

            // // Entity ID (debugging)
            // if self.tab_state.settings.replay_settings.show_entity_id {
            //     ui.col(|ui| {
            //         ui.label(format!("{}", player_report.vehicle.id()));
            //     });
            // }

            // Expanded content goes here
            if 0.0 < expandedness {
                match column {
                    ReplayColumn::Name => {
                        ui.vertical(|ui| {
                            if !report.achievements.is_empty() {
                                ui.strong(t!("ui.replay.sections.achievements"));

                                // Resolve icons: read lock for cache hits, write lock only on misses
                                let icons: Vec<Option<Arc<GameAsset>>> = {
                                    let wows_data = self.wows_data.read();
                                    report
                                        .achievements
                                        .iter()
                                        .map(|a| wows_data.cached_achievement_icon(&a.icon_key))
                                        .collect()
                                };
                                let icons: Vec<Option<Arc<GameAsset>>> = if icons.iter().any(|i| i.is_none()) {
                                    let mut wows_data = self.wows_data.write();
                                    report
                                        .achievements
                                        .iter()
                                        .zip(icons)
                                        .map(|(a, cached)| {
                                            cached.or_else(|| wows_data.load_achievement_icon(&a.icon_key))
                                        })
                                        .collect()
                                } else {
                                    icons
                                };

                                for (achievement, icon) in report.achievements.iter().zip(icons) {
                                    ui.horizontal(|ui| {
                                        if let Some(icon) = icon {
                                            let image = Image::new(ImageSource::Bytes {
                                                uri: icon.path.clone().into(),
                                                bytes: icon.data.clone().into(),
                                            })
                                            .fit_to_exact_size((32.0, 32.0).into());
                                            ui.add(image).on_hover_text(&achievement.description);
                                        }

                                        let response = if achievement.count > 1 {
                                            ui.label(format!("{} ({}x)", &achievement.display_name, achievement.count))
                                        } else {
                                            ui.label(&achievement.display_name)
                                        };
                                        response.on_hover_text(&achievement.description);
                                    });
                                }
                            }

                            // Display ribbons
                            if !report.ribbons.is_empty() {
                                if !report.achievements.is_empty() {
                                    ui.separator();
                                }
                                ui.strong(t!("ui.replay.sections.ribbons"));

                                // Sort ribbons by count descending for display
                                let mut ribbons: Vec<_> = report.ribbons.values().collect();
                                ribbons.sort_by(|a, b| a.name.cmp(&b.name));

                                // One-off fix: insert RIBBON_BULGE (torp protection) immediately after RIBBON_MAIN_CALIBER
                                if let Some(main_caliber_idx) =
                                    ribbons.iter().position(|r| r.name == "RIBBON_MAIN_CALIBER")
                                    && let Some(bulge_idx) = ribbons.iter().position(|r| r.name == "RIBBON_BULGE")
                                {
                                    let bulge = ribbons.remove(bulge_idx);
                                    // Adjust index if bulge was before main_caliber
                                    let insert_idx = if bulge_idx < main_caliber_idx {
                                        main_caliber_idx
                                    } else {
                                        main_caliber_idx + 1
                                    };
                                    ribbons.insert(insert_idx, bulge);
                                }

                                let wows_data = self.wows_data.read();
                                for ribbon in ribbons {
                                    ui.horizontal(|ui| {
                                        // Prefer the replay build's own icon; fall back to the
                                        // newest loaded build when this build ships none (Flash-era
                                        // ~0.9.5-0.10.4 and older embed ribbons in achievements.swf),
                                        // and finally to text only if even the latest build lacks it.
                                        let icon = if ribbon.is_subribbon {
                                            let key = format!("sub{}", ribbon.icon_key);
                                            wows_data
                                                .subribbon_icons
                                                .get(&key)
                                                .or_else(|| self.fallback_subribbon_icons.get(&key))
                                        } else {
                                            wows_data
                                                .ribbon_icons
                                                .get(&ribbon.icon_key)
                                                .or_else(|| self.fallback_ribbon_icons.get(&ribbon.icon_key))
                                        };

                                        let size = if ribbon.is_subribbon { (32.0, 32.0) } else { (64.0, 64.0) };

                                        if let Some(icon) = icon {
                                            let image = Image::new(ImageSource::Bytes {
                                                uri: icon.path.clone().into(),
                                                bytes: icon.data.clone().into(),
                                            })
                                            .fit_to_exact_size(size.into());
                                            ui.add(image).on_hover_text(&ribbon.description);
                                        }

                                        ui.label(format!("{} ({}x)", &ribbon.display_name, ribbon.count))
                                            .on_hover_text(&ribbon.description);
                                    });
                                }
                            }

                            let has_damage_events = report.fires.is_some()
                                || report.floods.is_some()
                                || report.citadels.is_some()
                                || report.crits.is_some();
                            if has_damage_events {
                                if !report.achievements.is_empty() || !report.ribbons.is_empty() {
                                    ui.separator();
                                }
                                ui.strong(t!("ui.replay.sections.damage_events"));
                                if report.should_hide_stats() && !self.debug_mode {
                                    ui.label(t!("ui.replay.nda"));
                                } else {
                                    if let Some(fires) = report.fires {
                                        ui.label(format!("{}: {fires}", t!("ui.replay.column.fires")));
                                    }
                                    if let Some(floods) = report.floods {
                                        ui.label(format!("{}: {floods}", t!("ui.replay.column.floods")));
                                    }
                                    if let Some(citadels) = report.citadels {
                                        ui.label(format!("{}: {citadels}", t!("ui.replay.column.citadels")));
                                    }
                                    if let Some(crits) = report.crits {
                                        ui.label(format!("{}: {crits}", t!("ui.replay.column.crits")));
                                    }
                                }
                            }

                            if let Some(fire_chance) = report.fire_chance.as_ref() {
                                if !report.achievements.is_empty() || !report.ribbons.is_empty() || has_damage_events {
                                    ui.separator();
                                }
                                self.render_fire_chance(
                                    ui,
                                    fire_chance,
                                    report.should_hide_stats() && !self.debug_mode,
                                );
                            }
                        });
                    }
                    ReplayColumn::ActualDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label(t!("ui.replay.nda"));
                        } else if report.actual_damage_hover_text().is_some() || report.damage_interactions.is_some() {
                            self.dealt_damage_details(report, ui);
                        }
                    }
                    ReplayColumn::PotentialDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label(t!("ui.replay.nda"));
                        } else if let Some(damage_extended_info) = report.potential_damage_hover_text.clone() {
                            ui.label(damage_extended_info);
                        }
                    }
                    ReplayColumn::SpottingDamage => {
                        if let Some(hover_text) = report.spotting_damage_hover_text.clone() {
                            ui.label(hover_text);
                        }
                    }
                    ReplayColumn::ReceivedDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label(t!("ui.replay.nda"));
                        } else if report.received_damage_hover_text.is_some() || report.damage_interactions.is_some() {
                            self.received_damage_details(report, ui);
                        }
                    }
                    ReplayColumn::Skills => {
                        if !report.relation().is_enemy() || self.debug_mode || self.merge_active {
                            ui.vertical(|ui| {
                                if let Some(hover_text) = &report.skill_info.hover_text {
                                    ui.label(hover_text);
                                }
                                if let Some(build_info) = &report.translated_build {
                                    ui.separator();

                                    // A ship with slots but no upgrades mounted renders a row of
                                    // empty placeholders (shows how many slots exist); only a
                                    // ship whose slot count is unknown (0) shows the none label.
                                    if build_info.modernization_slots.is_empty() {
                                        ui.label(t!("ui.replay.sections.modules_none"));
                                    } else {
                                        ui.label(t!("ui.replay.sections.modules"));
                                        self.render_modernization_slots(ui, &build_info.modernization_slots);
                                    }

                                    if !build_info.signals.is_empty() {
                                        ui.separator();
                                        ui.label(t!("ui.replay.sections.signals"));
                                        self.render_signals(ui, &build_info.signals);
                                    }

                                    ui.separator();

                                    if build_info.loadout.is_empty() {
                                        ui.label(t!("ui.replay.sections.loadout_none"));
                                    } else {
                                        ui.label(t!("ui.replay.sections.loadout"));
                                        for module in &build_info.loadout {
                                            if let Some(name) = &module.name {
                                                let label = ui.label(name);
                                                if let Some(hover_text) = module.description.as_ref() {
                                                    label.on_hover_text(hover_text);
                                                }
                                            }
                                        }
                                    }

                                    ui.separator();

                                    if let Some(captain_skills) = build_info.captain_skills.as_ref() {
                                        ui.label(t!("ui.replay.sections.captain_skills"));
                                        if captain_skills.is_empty() {
                                            ui.label(t!("ui.replay.sections.captain_skills_none"));
                                        } else {
                                            self.render_skill_grid(ui, captain_skills);
                                        }
                                    } else {
                                        ui.label(t!("ui.replay.sections.captain_skills_none"));
                                    }
                                }

                                if !report.consumables.is_empty() {
                                    ui.separator();
                                    ui.label(t!("ui.replay.sections.consumables"));
                                    self.render_consumable_inventory(ui, &report.consumables);
                                } else if let Some(build_info) = &report.translated_build
                                    && !build_info.abilities.is_empty()
                                {
                                    // No activations were replayed; list the configured
                                    // consumables by name instead of the used/total table.
                                    ui.separator();
                                    ui.label(t!("ui.replay.sections.consumables"));
                                    for ability in &build_info.abilities {
                                        if let Some(name) = &ability.name {
                                            ui.label(name);
                                        }
                                    }
                                }
                            });
                        }
                    }
                    ReplayColumn::Hits => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label(t!("ui.replay.nda"));
                        } else if let Some(hits_extended_info) = report.hits_hover_text.clone() {
                            ui.label(hits_extended_info);
                        }
                    }
                    _ => {
                        // Do nothing
                    }
                }
            }
        });

        match ui.input(|i| {
            let double_clicked = i.pointer.button_double_clicked(egui::PointerButton::Primary)
                && ui.max_rect().contains(i.pointer.interact_pos().unwrap_or_default());
            let single_clicked = i.pointer.button_clicked(egui::PointerButton::Primary)
                && i.modifiers.ctrl
                && ui.max_rect().contains(i.pointer.interact_pos().unwrap_or_default());

            (double_clicked, single_clicked)
        }) {
            (true, _) => {
                // A double-click shouldn't enable row selection
                if let Some((_row, false)) = self.selected_row {
                    self.selected_row = None;
                }

                change_expand = true;
            }
            (false, true) => {
                if self.selected_row.take().filter(|prev| prev.0 == row_nr).is_none() {
                    self.selected_row = Some((row_nr, true));
                    ui.ctx().request_repaint();
                }
            }
            _ => {
                // both false
            }
        }

        if change_expand {
            // Toggle.
            // Note: we use a map instead of a set so that we can animate opening and closing of each column.
            self.is_row_expanded.insert(row_nr, !is_expanded);
            self.row_heights.remove(&row_nr);
        }

        let cell_height = inner_response.response.rect.height();
        let previous_height = self.row_heights.entry(row_nr).or_insert(cell_height);

        if *previous_height < cell_height {
            *previous_height = cell_height;
        }
    }

    fn metadata_provider(&self) -> Arc<GameMetadataProvider> {
        self.wows_data.read().game_metadata.as_ref().expect("no metadata provider?").clone()
    }

    pub fn match_timestamp(&self) -> Timestamp {
        self.match_timestamp
    }

    pub fn player_reports(&self) -> &[PlayerReport] {
        &self.player_reports
    }

    pub fn battle_result(&self) -> Option<BattleResult> {
        self.battle_result
    }

    /// Re-derive all translation-dependent display strings (ship names, species,
    /// bot names, achievements, ribbons) from the current locale. Call this after
    /// the user changes language so the report reflects the new translations
    /// without rebuilding from scratch.
    pub fn refresh_translations(&mut self) {
        let wows_data = self.wows_data.read();
        let Some(metadata_provider) = wows_data.game_metadata.as_ref() else {
            return;
        };

        for report in &mut self.player_reports {
            let vehicle_param = report.player.vehicle();
            let player_state = report.player.initial_state();

            // Ship name
            report.ship_name = metadata_provider
                .localized_name_from_param(vehicle_param)
                .unwrap_or_else(|| format!("{}", vehicle_param.id()));

            // Ship species text
            if let Some(species) = vehicle_param.species().and_then(|r| r.known().cloned()) {
                report.ship_species_text = metadata_provider
                    .localized_name_from_id(&TranslationKey::new(species.translation_id()))
                    .unwrap_or_else(|| species.name().to_string());
            }

            // Bot display name
            if player_state.is_bot() && player_state.username().starts_with("IDS_") {
                report.name_text = metadata_provider
                    .localized_name_from_id(&TranslationKey::new(player_state.username()))
                    .unwrap_or_else(|| player_state.username().to_string());
            }

            // Translated build
            report.translated_build = models::TranslatedBuild::new(&report.player, metadata_provider, &self.version);

            // Achievements (icon_key holds the ui_name used for translation lookup)
            for achievement in &mut report.achievements {
                if let Some(name) = wowsunpack::game_params::translations::translate_achievement_name(
                    &achievement.icon_key,
                    metadata_provider.as_ref(),
                ) {
                    achievement.display_name = name;
                }
                if let Some(desc) = wowsunpack::game_params::translations::translate_achievement_description(
                    &achievement.icon_key,
                    metadata_provider.as_ref(),
                ) {
                    achievement.description = desc;
                }
            }

            // Ribbons (name holds the RIBBON_* key used for translation lookup)
            for ribbon in report.ribbons.values_mut() {
                if let Some(translation) =
                    wowsunpack::game_params::translations::translate_ribbon(&ribbon.name, metadata_provider.as_ref())
                {
                    ribbon.display_name = translation.display_name;
                    ribbon.description = translation.description;
                }
            }
        }
    }

    /// Populate Personal Rating for all players using the provided PR data
    pub fn populate_personal_ratings(&mut self, pr_data: &crate::util::personal_rating::PersonalRatingData) {
        for report in &mut self.player_reports {
            if report.personal_rating.is_some() {
                continue;
            }

            let Some(player) = Some(report.player()) else {
                continue;
            };

            let ship_id = player.vehicle().id();
            let battle_result = self.battle_result;

            // We need actual damage, kills, and win/loss for a single battle
            let Some(actual_damage) = report.actual_damage else {
                continue;
            };

            let is_win = matches!(battle_result, Some(BattleResult::Win(_)));
            let frags = report.kills.unwrap_or(0);

            let stats = crate::util::personal_rating::ShipBattleStats {
                ship_id,
                battles: 1,
                damage: actual_damage,
                wins: if is_win { 1 } else { 0 },
                frags,
            };

            report.personal_rating = pr_data.calculate_pr(&[stats]);
        }
    }
}

impl egui_table::TableDelegate for UiReport {
    fn header_cell_ui(&mut self, ui: &mut egui::Ui, cell_inf: &egui_table::HeaderCellInfo) {
        let egui_table::HeaderCellInfo { group_index, .. } = cell_inf;

        let margin = 4;

        egui::Frame::new().inner_margin(Margin::symmetric(margin, 0)).show(ui, |ui| {
            let column = *self.columns.get(*group_index).expect("somehow ended up with zero columns?");
            match column {
                ReplayColumn::Actions => {
                    ui.label(t!("ui.replay.column.actions"));
                }
                ReplayColumn::Name => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.player_name"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Name,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Name);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::BaseXp => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.base_xp"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::BaseXp,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::BaseXp);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::RawXp => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.raw_xp"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::RawXp,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::RawXp);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::ShipName => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.ship_name"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::ShipName,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::ShipName);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Hits => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.hits"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Hits,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Hits);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Heals => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.heals"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Heals,
                        ))
                        .on_hover_text(t!("ui.replay.column.heals_tooltip"))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Heals);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Kills => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.kills"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Kills,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Kills);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::ObservedDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.observed_damage"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::ObservedDamage,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::ObservedDamage);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::ActualDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.actual_damage"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::ActualDamage,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::ActualDamage);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::SpottingDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.spotting_damage"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::SpottingDamage,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::SpottingDamage);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::PotentialDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.potential_damage"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::PotentialDamage,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::PotentialDamage);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::TimeLived => {
                    ui.strong(t!("ui.replay.column.time_lived"));
                }
                ReplayColumn::ReceivedDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.received_damage"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::ReceivedDamage,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::ReceivedDamage);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::DistanceTraveled => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.distance_traveled"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::DistanceTraveled,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::DistanceTraveled);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Skills => {
                    ui.strong(t!("ui.replay.column.skills"));
                }
                ReplayColumn::PersonalRating => {
                    if ui
                        .strong(column_name_with_sort_order(
                            &t!("ui.replay.column.personal_rating"),
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::PersonalRating,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::PersonalRating);

                        self.sort_players(new_sort);
                    };
                }
            }
        });
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, cell_info: &egui_table::CellInfo) {
        let egui_table::CellInfo { row_nr, col_nr, .. } = *cell_info;

        if self.selected_row.filter(|row| row.0 == row_nr && row.1).is_some() {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().selection.bg_fill);
        } else if row_nr % 2 == 1 {
            ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().faint_bg_color);
        }

        egui::Frame::new().inner_margin(Margin::symmetric(4, 4)).show(ui, |ui| {
            self.cell_content_ui(row_nr, col_nr, ui);
        });
    }

    fn row_top_offset(&self, ctx: &Context, _table_id: Id, row_nr: u64) -> f32 {
        self.is_row_expanded
            .range(0..row_nr)
            .map(|(expanded_row_nr, expanded)| {
                let how_expanded =
                    ctx.animate_bool(Id::new(("replay_row", self.report_salt, *expanded_row_nr)), *expanded);
                how_expanded * self.row_heights.get(expanded_row_nr).copied().unwrap()
            })
            .sum::<f32>()
            + row_nr as f32 * ROW_HEIGHT
    }
}

const ROW_HEIGHT: f32 = 28.0;

/// A validated alt-perspective pick, waiting for a frame in which no replay
/// write guard is held so it can be pushed onto its replay and re-parsed.
#[derive(Clone)]
struct PendingAltRequest {
    /// The workspace whose replay tab raised this. Carried for diagnostics: the
    /// re-parse itself works through the replay handle below, which any
    /// workspace showing that replay shares.
    workspace: WorkspaceId,
    replay: Weak<RwLock<Replay>>,
    alt: std::sync::Arc<ReplayFile>,
}

/// Transient handoff between the in-tab "Load Other Team Perspective"
/// button and `handle_pending_alt_reparse`, which triggers the re-parse.
/// Wrapped because egui's `Memory::remove_temp` requires the stored type to
/// be `Default`, and [`PendingAltRequest`] is not.
#[derive(Default, Clone)]
struct PendingAltReparse(Option<PendingAltRequest>);

/// Parks a validated alt-perspective pick for the next frame's consumer.
fn stash_alt_reparse(ctx: &egui::Context, request: PendingAltRequest) {
    ctx.data_mut(|data| data.insert_temp(alt_perspective_slot_id(), PendingAltReparse(Some(request))));
}

/// Takes whatever alt-perspective pick is parked, if any. Every caller consumes
/// unconditionally: the request names its own replay, so no caller has to match
/// a workspace against it to know it is theirs to run.
fn take_alt_reparse(ctx: &egui::Context) -> Option<PendingAltRequest> {
    ctx.data_mut(|data| data.remove_temp::<PendingAltReparse>(alt_perspective_slot_id())).and_then(|pending| pending.0)
}

pub struct Replay {
    pub replay_file: ReplayFile,

    /// Optional additional replays from other players in the same match. When
    /// non-empty, the parser feeds all of them into a single merged
    /// [`wows_battle_world::merged::MergedReplays`]
    /// so that enemy positions / HP / consumables / etc. that the primary's
    /// client never saw become visible in the resulting BattleReport.
    pub alt_replays: Vec<ReplayFile>,

    pub resource_loader: Arc<GameMetadataProvider>,

    pub battle_report: Option<BattleReport>,
    pub ui_report: Option<UiReport>,

    pub game_constants: Option<Arc<wows_replays::game_constants::GameConstants>>,

    /// Original file path this replay was loaded from, if available.
    pub source_path: Option<PathBuf>,

    /// Lazy background-extraction state for the Match Timeline window.
    pub(crate) timeline: TimelineState,
}

/// The colour a player's clan tag should render with. `None` if the player
/// has no clan. Resolved to a `Color32` at draw time via `ClanColor::color`.
fn clan_color_for_player(player: &Player) -> Option<ClanColor> {
    let state = player.initial_state();
    if state.clan().is_empty() {
        return None;
    }
    // Older replays omit clanColor; fall back to the player's team color so the
    // clan tag still renders instead of panicking.
    let clan_color = match state.raw_with_names().get("clanColor").and_then(|c| c.as_i64()) {
        Some(clan_color) => ClanColor::Fixed(Color32::from_rgb(
            ((clan_color & 0xFF0000) >> 16) as u8,
            ((clan_color & 0xFF00) >> 8) as u8,
            (clan_color & 0xFF) as u8,
        )),
        None => {
            tracing::warn!("player '{}' has no clanColor; using team color", state.username());
            ClanColor::Relation(PlayerTint::from_relation(player.relation()))
        }
    };
    Some(clan_color)
}

impl Replay {
    pub fn new(replay_file: ReplayFile, resource_loader: Arc<GameMetadataProvider>) -> Self {
        Replay {
            replay_file,
            alt_replays: Vec::new(),
            resource_loader,
            battle_report: None,
            ui_report: None,
            game_constants: None,
            source_path: None,
            timeline: TimelineState::NotRequested,
        }
    }

    pub fn player_vehicle(&self) -> Option<&VehicleInfoMeta> {
        let meta = &self.replay_file.meta;
        meta.vehicles.iter().find(|vehicle| vehicle.relation == 0)
    }

    pub fn vehicle_name(&self, metadata_provider: &GameMetadataProvider) -> String {
        self.player_vehicle()
            .and_then(|vehicle| metadata_provider.param_localization_id(vehicle.shipId.raw().into()))
            .and_then(|id| metadata_provider.localized_name_from_id(&TranslationKey::new(id)))
            .unwrap_or_else(|| t!("ui.replay.spectator").into())
    }

    #[allow(dead_code)]
    pub fn player_name(&self) -> Option<&str> {
        self.player_vehicle().map(|vehicle| vehicle.name.as_str())
    }

    pub fn map_name(&self, metadata_provider: &GameMetadataProvider) -> String {
        wowsunpack::game_params::translations::translate_map_name(&self.replay_file.meta.mapName, metadata_provider)
    }

    pub fn game_mode(&self, metadata_provider: &GameMetadataProvider) -> String {
        wowsunpack::game_params::translations::translate_game_mode(
            self.replay_file.meta.gameType.as_deref().unwrap_or(""),
            metadata_provider,
        )
    }

    pub fn scenario(&self, metadata_provider: &GameMetadataProvider) -> String {
        wowsunpack::game_params::translations::translate_scenario(&self.replay_file.meta.scenario, metadata_provider)
    }

    pub fn game_time(&self) -> &str {
        &self.replay_file.meta.dateTime
    }

    /// Get the battle result, preferring battle_report if available, otherwise cached result.
    pub fn battle_report(&self) -> Option<&BattleReport> {
        self.battle_report.as_ref()
    }

    pub fn better_file_name(&self, metadata_provider: &GameMetadataProvider) -> String {
        [
            self.vehicle_name(metadata_provider).as_str(),
            self.map_name(metadata_provider).as_str(),
            self.scenario(metadata_provider).as_str(),
            self.game_mode(metadata_provider).as_str(),
            self.game_time(),
        ]
        .iter()
        .join("_")
        .replace(['.', ':', ' '], "-")
    }

    pub fn parse(&self, expected_build: &str) -> Result<BattleReport, Report> {
        let version_parts: Vec<_> = self.replay_file.meta.clientVersionFromExe.split(',').collect();
        assert!(version_parts.len() == 4);
        if version_parts[3] != expected_build {
            return Err(ToolkitError::ReplayVersionMismatch {
                game_version: expected_build.to_string(),
                replay_version: version_parts[3].to_string(),
            }
            .into());
        }

        let replay_version = wowsunpack::data::Version::from_client_exe(&self.replay_file.meta.clientVersionFromExe);

        tracing::info!(
            primary = %self.replay_file.meta.playerName,
            alt_count = self.alt_replays.len(),
            "Replay::parse"
        );
        if self.alt_replays.is_empty() {
            // Single-replay fast path — no merger involved.
            let mut world =
                BattleWorld::new(&self.replay_file.meta, self.resource_loader.as_ref(), self.game_constants.as_deref());
            // Fire-chance analysis divides by the whole-match hit history, which is
            // otherwise not recorded to save memory for renderers that only need
            // the current frame's hits. Its shells-fired count reads the salvo
            // log, which is off by the same default.
            world.set_record_hit_history(true);
            world.set_record_salvo_history(true);
            let mut p =
                wows_replays::packet2::Parser::with_version(self.resource_loader.entity_specs(), replay_version);
            let mut remaining = self.replay_file.packet_data.as_slice();
            while !remaining.is_empty() {
                match p.parse_packet(&mut remaining) {
                    Ok(packet) => world.process(&packet),
                    Err(e) => {
                        debug!("Packet parse error: {:?}", e);
                        break;
                    }
                }
            }
            world.finish();
            return Ok(world.into_report());
        }

        // Merge path — fold alt perspectives into a single world via
        // MergedReplays so the report sees through fog of war.
        let game_constants = self
            .game_constants
            .as_deref()
            .ok_or_else(|| rootcause::report!("game constants required to merge alt-perspective replays"))?;
        let mut session = MergedReplays::new(
            self.resource_loader.entity_specs(),
            self.resource_loader.as_ref(),
            game_constants,
            replay_version,
            &self.replay_file,
            &self.alt_replays,
        )
        .map_err(|e| rootcause::report!("{e}"))?;
        // See the single-replay path above: fire-chance analysis needs the
        // whole-match hit history and the salvo log, both off by default.
        session.world_mut().set_record_hit_history(true);
        session.world_mut().set_record_salvo_history(true);
        while session.step().map_err(|e| rootcause::report!("{e}"))?.is_some() {}
        session.finish();
        Ok(session.into_world().into_report())
    }

    pub fn build_ui_report(&mut self, deps: &crate::data::wows_data::ReplayDependencies) {
        if let Some(battle_report) = &self.battle_report {
            let replay_version =
                wowsunpack::data::Version::from_client_exe(&self.replay_file.meta.clientVersionFromExe);

            // Resolve version-matched data so the UI report uses the correct constants
            let Some(wows_data) = deps.resolve_versioned_deps(&replay_version) else {
                tracing::warn!(
                    "Could not resolve versioned data for build {}",
                    replay_version.build_number().map_or_else(|| "unknown".to_string(), |b| b.to_string())
                );
                return;
            };

            let merge_active = !self.alt_replays.is_empty();
            let mut ui_report = UiReport::new(&self.replay_file, battle_report, &wows_data, deps, merge_active);

            {
                let pr_data = deps.personal_rating_data.read();
                if pr_data.is_loaded() {
                    ui_report.populate_personal_ratings(&pr_data);
                }
            }

            self.ui_report = Some(ui_report);
        }
    }

    /// Returns a boolean indicating if the replay has incomplete battle results.
    pub fn battle_results_are_pending(&self) -> bool {
        // If we don't yet have a battle result, that implies that we never got the end
        // of battle packet.
        //
        // If we don't have a UI report, that implies that the battle result packet from the
        // server was never received
        self.battle_result().is_none()
    }

    pub fn battle_result(&self) -> Option<BattleResult> {
        self.battle_report()
            .and_then(|report| report.battle_result().cloned())
            .or_else(|| self.ui_report.as_ref().and_then(|report| report.battle_result()))
    }

    /// Convert this replay's player stats to ShipBattleStats for PR calculation
    pub fn to_battle_stats(&self) -> Option<crate::util::personal_rating::ShipBattleStats> {
        let vehicle = self.player_vehicle()?;
        let battle_result = self.battle_result()?;
        let ui_report = self.ui_report.as_ref()?;
        let self_report = ui_report.player_reports().iter().find(|report| report.relation().is_self())?;

        let is_win = matches!(battle_result, BattleResult::Win(_));

        Some(crate::util::personal_rating::ShipBattleStats {
            ship_id: vehicle.shipId,
            battles: 1,
            damage: self_report.actual_damage().unwrap_or_default(),
            wins: if is_win { 1 } else { 0 },
            frags: self_report.kills().unwrap_or_default(),
        })
    }
}

fn column_name_with_sort_order(text: &str, has_info: bool, sort_order: SortOrder, column: SortColumn) -> String {
    if sort_order.column() == column {
        if has_info {
            format!("{} {} {}", text, icons::INFO, sort_order.icon())
        } else {
            format!("{} {}", text, sort_order.icon())
        }
    } else if has_info {
        format!("{} {}", text, icons::INFO)
    } else {
        text.to_string()
    }
}

fn copy_files_to_clipboard(paths: &[std::path::PathBuf]) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.set().file_list(paths);
    }
}

/// Label for the alt-perspective action, with a merge count when replays are
/// already merged in.
fn alt_perspective_label(replay_file: &Replay) -> String {
    let mut label = wt_translations::icon_t(icons::FOLDER_OPEN, &t!("ui.replay.load_alt_perspective"));
    if !replay_file.alt_replays.is_empty() {
        label.push_str(&format!(" ({})", replay_file.alt_replays.len()));
    }
    label
}

/// Which source a workspace's summary load should read: its own resolved
/// source once it has one, or the live source while it does not. A workspace
/// stuck on `None` forever (the live workspace) keeps reading the live
/// source for its whole life; an imported workspace moves to `Explicit` once
/// its source is ensured.
fn summary_source_selector(source: Option<crate::db::index::rows::SourceId>) -> crate::task::SourceSelector {
    match source {
        Some(source) => crate::task::SourceSelector::Explicit(source),
        None => crate::task::SourceSelector::Live,
    }
}

impl ToolkitTabViewer<'_> {
    fn metadata_provider(&self) -> Option<Arc<GameMetadataProvider>> {
        self.tab_state.world_of_warships_data.as_ref().and_then(|wows_data| wows_data.read().game_metadata.clone())
    }

    fn build_replay_player_list(&self, ui_report: &mut UiReport, ui: &mut egui::Ui) {
        // Populate PR data if available (must happen before sorting so PR sort works)
        {
            let pr_data = self.tab_state.personal_rating_data.read();
            if pr_data.is_loaded() {
                ui_report.populate_personal_ratings(&pr_data);
            }
        }

        if !ui_report.sorted {
            let replay_sort = self.tab_state.replay_sort.lock();
            ui_report.sort_players(*replay_sort);
        }

        ui_report.update_visible_columns(&self.tab_state.persisted.read().settings.replay);

        let mut columns =
            vec![egui_table::Column::new(100.0).range(10.0..=500.0).resizable(true); ui_report.columns.len()];
        let action_label_layout = ui.painter().layout_no_wrap(
            t!("ui.replay.column.actions").into(),
            egui::FontId::default(),
            ui.style().visuals.text_color(),
        );
        let action_label_width = action_label_layout.rect.width() + 4.0;
        columns[ReplayColumn::Actions as usize] = egui_table::Column::new(action_label_width).resizable(false);

        // The name column carries every expanded section: achievements, ribbons,
        // damage events and the fire-chance breakdown. Those are wider than a
        // name, and a cell clips rather than wraps, so the shared 500 px ceiling
        // truncates them once expanded.
        columns[ReplayColumn::Name as usize] = egui_table::Column::new(260.0).range(10.0..=1400.0).resizable(true);

        let table = egui_table::Table::new()
            .id_salt("replay_player_list")
            .num_rows(ui_report.player_reports.len() as u64)
            .columns(columns)
            .num_sticky_cols(3)
            .headers([egui_table::HeaderRow { height: 14.0f32, groups: Default::default() }])
            .auto_size_mode(egui_table::AutoSizeMode::Never);
        table.show(ui, ui_report);
    }

    fn build_replay_view(
        &self,
        replay_file: &mut Replay,
        replay_weak: &Weak<RwLock<Replay>>,
        ui: &mut egui::Ui,
        metadata_provider: &GameMetadataProvider,
        ws_id: WorkspaceId,
    ) {
        // little hack because of borrowing issues
        let mut hide_my_stats = false;
        let mut hide_my_stats_changed = false;
        if let Some(report) = replay_file.battle_report.as_ref() {
            let self_player = report.self_player();
            let self_state = self_player.initial_state();
            // --- Row 1: Key outcome info + action buttons ---
            let mut self_report = None;
            ui.horizontal(|ui| {
                if replay_file.battle_results_are_pending() {
                    let text = RichText::new(wt_translations::icon_t(icons::INFO, &t!("ui.replay.incomplete_results")))
                        .color(ui.sem().warn);
                    ui.strong(text).on_hover_text(t!("ui.replay.incomplete_results_tooltip"));
                }

                if let Some(battle_result) = replay_file.battle_result() {
                    let sem = ui.sem();
                    let text = match battle_result {
                        BattleResult::Win(_) => {
                            RichText::new(wt_translations::icon_t(icons::TROPHY, &t!("ui.replay.results.victory")))
                                .color(sem.win)
                        }
                        BattleResult::Loss(_) => {
                            RichText::new(wt_translations::icon_t(icons::SMILEY_SAD, &t!("ui.replay.results.defeat")))
                                .color(sem.loss)
                        }
                        BattleResult::Draw => {
                            RichText::new(wt_translations::icon_t(icons::NOTCHES, &t!("ui.replay.results.draw")))
                                .color(sem.draw)
                        }
                    };
                    ui.label(text);
                }

                if let Some(battle_stats) = replay_file.to_battle_stats() {
                    let pr_data = self.tab_state.personal_rating_data.read();
                    if let Some(pr_result) = pr_data.calculate_pr(&[battle_stats]) {
                        crate::ui::widgets::pr_chip(
                            ui,
                            pr_result.category,
                            &format!("PR: {:.0} ({})", pr_result.pr, pr_result.category.name()),
                            false,
                        );
                    }
                }

                if let Some(ui_report) = replay_file.ui_report.as_ref() {
                    for vehicle_report in &ui_report.player_reports {
                        if vehicle_report.relation().is_self() {
                            self_report = Some(vehicle_report);
                            hide_my_stats = vehicle_report.manual_stat_hide_toggle;
                            break;
                        }
                    }
                }

                ui.menu_button(t!("ui.replay.actions"), |ui| {
                    ui.label(RichText::new(t!("ui.replay.section_match").as_ref()).strong());

                    if ui.button(wt_translations::icon_t(icons::LIST_BULLETS, &t!("ui.replay.timeline"))).clicked() {
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("show_timeline"), true);
                        });
                        ui.close_kind(UiKind::Menu);
                    }

                    if !self.tab_state.persisted.read().settings.game.wows_dir.is_empty()
                        && replay_file.source_path.is_some()
                    {
                        if ui
                            .button(wt_translations::icon_t(
                                icons::GAME_CONTROLLER,
                                &t!("ui.replay.context.open_in_game"),
                            ))
                            .clicked()
                        {
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(
                                    egui::Id::new("pending_confirmation_request"),
                                    Some(crate::tab_state::ConfirmableAction::OpenInGame {
                                        replay_path: replay_file.source_path.clone().unwrap(),
                                    }),
                                );
                            });
                            ui.close_kind(UiKind::Menu);
                        }
                        if ui
                            .button(wt_translations::icon_t(
                                icons::KEYBOARD,
                                &t!("ui.replay.context.show_replay_controls"),
                            ))
                            .clicked()
                        {
                            ui.ctx().data_mut(|data| {
                                // App-wide: opens the single reference window regardless of workspace.
                                data.insert_temp(egui::Id::new("open_replay_controls_window"), true);
                            });
                            ui.close_kind(UiKind::Menu);
                        }
                    }

                    if ui
                        .button(alt_perspective_label(replay_file))
                        .on_hover_text(t!("ui.replay.load_alt_perspective_tooltip"))
                        .clicked()
                    {
                        self.load_alt_perspective_action(ui, replay_file, replay_weak, ws_id);
                        ui.close_kind(UiKind::Menu);
                    }

                    if let Some(self_report) = self_report
                        && self_report.is_test_ship()
                        && ui.checkbox(&mut hide_my_stats, t!("ui.replay.hide_my_stats")).changed()
                    {
                        hide_my_stats_changed = true;
                    }

                    ui.separator();
                    ui.label(RichText::new(t!("ui.replay.section_export").as_ref()).strong());

                    if ui
                        .button(wt_translations::icon_t(icons::FLOPPY_DISK, &t!("ui.replay.export_chat_save")))
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!(
                                "{} {} {} - Game Chat.txt",
                                report.game_type(),
                                report.game_mode(),
                                report.map_name()
                            ))
                            .save_file()
                            && let Ok(mut file) = std::fs::File::create(path)
                        {
                            for message in report.game_chat() {
                                let GameMessage {
                                    sender_relation: _,
                                    sender_name,
                                    channel,
                                    message,
                                    entity_id: _,
                                    player,
                                    clock: _,
                                } = message;
                                match player {
                                    Some(player) if !player.initial_state().clan().is_empty() => {
                                        let _ = writeln!(
                                            file,
                                            "[{}] {} ({:?}): {}",
                                            player.initial_state().clan(),
                                            sender_name,
                                            channel,
                                            message
                                        );
                                    }
                                    _ => {
                                        let _ = writeln!(file, "{sender_name} ({channel:?}): {message}");
                                    }
                                }
                            }
                        }
                        ui.close_kind(UiKind::Menu);
                    }
                    if ui.button(wt_translations::icon_t(icons::COPY, &t!("ui.replay.export_chat_copy"))).clicked() {
                        let mut buf = BufWriter::new(Vec::new());
                        for message in report.game_chat() {
                            let GameMessage {
                                sender_relation: _,
                                sender_name,
                                channel,
                                message,
                                entity_id: _,
                                player,
                                clock: _,
                            } = message;
                            match player {
                                Some(player) if !player.initial_state().clan().is_empty() => {
                                    let _ = writeln!(
                                        buf,
                                        "[{}] {} ({:?}): {}",
                                        player.initial_state().clan(),
                                        sender_name,
                                        channel,
                                        message
                                    );
                                }
                                _ => {
                                    let _ = writeln!(buf, "{sender_name} ({channel:?}): {message}");
                                }
                            }
                        }
                        let game_chat = String::from_utf8(buf.into_inner().expect("failed to get buf inner"))
                            .expect("failed to convert game chat buffer to string");
                        ui.ctx().copy_text(game_chat);
                        ui.close_kind(UiKind::Menu);
                    }

                    let format = if ui.button(t!("ui.replay.export_results_json")).clicked() {
                        Some(ReplayExportFormat::Json)
                    } else if ui.button(t!("ui.replay.export_results_cbor")).clicked() {
                        Some(ReplayExportFormat::Cbor)
                    } else if ui.button(t!("ui.replay.export_results_csv")).clicked() {
                        Some(ReplayExportFormat::Csv)
                    } else {
                        None
                    };
                    if let Some(format) = format
                        && let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!(
                                "{}.{}",
                                replay_file.better_file_name(metadata_provider),
                                format.extension()
                            ))
                            .save_file()
                        && let Ok(mut file) = std::fs::File::create(path)
                    {
                        let transformed_results =
                            Match::new(replay_file, self.tab_state.persisted.read().settings.app.debug_mode);
                        let result = match format {
                            ReplayExportFormat::Json => serde_json::to_writer(&mut file, &transformed_results)
                                .map_err(|e| Box::new(e) as Box<dyn std::error::Error>),
                            ReplayExportFormat::Cbor => ciborium::into_writer(&transformed_results, &mut file)
                                .map_err(|e| Box::new(e) as Box<dyn std::error::Error>),
                            ReplayExportFormat::Csv => {
                                let mut writer = csv::WriterBuilder::new().has_headers(true).from_writer(file);
                                let mut result = Ok(());
                                for vehicle in transformed_results.vehicles {
                                    result = writer.serialize(FlattenedVehicle::from(vehicle));
                                    if result.is_err() {
                                        break;
                                    }
                                }
                                let _ = writer.flush();
                                result.map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
                            }
                        };
                        if let Err(e) = result {
                            error!("Failed to write results to file: {}", e);
                        }
                    }

                    if self.tab_state.persisted.read().settings.app.debug_mode {
                        ui.separator();
                        ui.label(RichText::new(t!("ui.replay.section_debug").as_ref()).strong());

                        if ui.button(t!("ui.replay.debug.raw_metadata")).clicked() {
                            let parsed_meta: serde_json::Value =
                                serde_json::from_str(&replay_file.replay_file.raw_meta)
                                    .expect("failed to parse replay metadata");
                            let pretty_meta = serde_json::to_string_pretty(&parsed_meta)
                                .expect("failed to serialize replay metadata");
                            let viewer = plaintext_viewer::PlaintextFileViewer {
                                title: Arc::new("metadata.json".to_owned()),
                                file_info: Arc::new(Mutex::new(FileType::PlainTextFile {
                                    ext: ".json".to_owned(),
                                    contents: pretty_meta,
                                })),
                                open: Arc::new(AtomicBool::new(true)),
                            };
                            self.tab_state.file_viewer.lock().push(viewer);
                            ui.close_kind(UiKind::Menu);
                        }

                        let has_results = report.battle_results().is_some();
                        ui.add_enabled_ui(has_results, |ui| {
                            if ui
                                .button(t!("ui.replay.debug.results_raw_json"))
                                .on_hover_text(t!("ui.replay.debug.raw_json_tooltip"))
                                .clicked()
                            {
                                if let Some(results_json) = report.battle_results() {
                                    let parsed_results: serde_json::Value =
                                        serde_json::from_str(results_json).expect("failed to parse battle results");
                                    let pretty = serde_json::to_string_pretty(&parsed_results)
                                        .expect("failed to serialize battle results");
                                    let viewer = plaintext_viewer::PlaintextFileViewer {
                                        title: Arc::new("results_raw.json".to_owned()),
                                        file_info: Arc::new(Mutex::new(FileType::PlainTextFile {
                                            ext: ".json".to_owned(),
                                            contents: pretty,
                                        })),
                                        open: Arc::new(AtomicBool::new(true)),
                                    };
                                    self.tab_state.file_viewer.lock().push(viewer);
                                }
                                ui.close_kind(UiKind::Menu);
                            }
                            if ui
                                .button(t!("ui.replay.debug.results_mapped_json"))
                                .on_hover_text(t!("ui.replay.debug.mapped_json_tooltip"))
                                .clicked()
                            {
                                if let Some(resolved) =
                                    replay_file.ui_report.as_ref().and_then(|r| r.resolved_results.as_ref())
                                {
                                    let pretty = serde_json::to_string_pretty(resolved)
                                        .expect("failed to serialize resolved results");
                                    let viewer = plaintext_viewer::PlaintextFileViewer {
                                        title: Arc::new("results_mapped.json".to_owned()),
                                        file_info: Arc::new(Mutex::new(FileType::PlainTextFile {
                                            ext: ".json".to_owned(),
                                            contents: pretty,
                                        })),
                                        open: Arc::new(AtomicBool::new(true)),
                                    };
                                    self.tab_state.file_viewer.lock().push(viewer);
                                }
                                ui.close_kind(UiKind::Menu);
                            }
                        });
                    }
                });

                {
                    let has_chat = !report.game_chat().is_empty();
                    let show_chat: bool =
                        ui.ctx().data(|d| d.get_temp(egui::Id::new("show_game_chat"))).unwrap_or(false);
                    let response = ui.add_enabled(
                        has_chat,
                        egui::Button::new(wt_translations::icon_t(icons::CHAT_TEXT, &t!("ui.replay.chat")))
                            .selected(show_chat),
                    );
                    if !has_chat {
                        response.on_disabled_hover_text(t!("ui.replay.no_chat"));
                    } else if response.clicked() {
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(egui::Id::new("show_game_chat"), !show_chat);
                        });
                    }
                }

                if self.tab_state.wows_data_map.is_some()
                    && ui.button(wt_translations::icon_t(icons::PLAY, &t!("ui.replay.render"))).clicked()
                {
                    let raw_meta = replay_file.replay_file.raw_meta.clone().into_bytes();
                    let pkt_data = replay_file.replay_file.packet_data.clone();
                    let map_name = replay_file.replay_file.meta.mapName.clone();
                    let translated_map = replay_file.map_name(metadata_provider);
                    let base = format!("{} - {}", replay_file.replay_file.meta.playerName, translated_map);
                    let replay_name = if let Some(stem) = replay_file
                        .source_path
                        .as_ref()
                        .and_then(|p: &PathBuf| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    {
                        format!("{} - {}", base, stem)
                    } else {
                        base
                    };
                    let game_duration = replay_file.replay_file.meta.duration as f32;
                    let replay_version =
                        wowsunpack::data::Version::from_client_exe(&replay_file.replay_file.meta.clientVersionFromExe);
                    let Some(wows_data) =
                        self.tab_state.wows_data_map.as_ref().and_then(|map| map.resolve(&replay_version))
                    else {
                        tracing::warn!(
                            "No data for build {}",
                            replay_version.build_number().map_or_else(|| "unknown".to_string(), |b| b.to_string())
                        );
                        return;
                    };
                    let asset_cache = self.tab_state.renderer_asset_cache.clone();
                    let alt_replays: Vec<crate::replay::renderer::AltReplayBytes> = replay_file
                        .alt_replays
                        .iter()
                        .map(|r| crate::replay::renderer::AltReplayBytes {
                            raw_meta: r.raw_meta.clone().into_bytes(),
                            packet_data: r.packet_data.clone(),
                        })
                        .collect();
                    let is_debug_mode = self.tab_state.persisted.read().settings.app.debug_mode;
                    let viewer = crate::replay::renderer::launch_replay_renderer(
                        raw_meta,
                        pkt_data,
                        alt_replays,
                        map_name,
                        replay_name,
                        game_duration,
                        wows_data,
                        asset_cache,
                        &self.tab_state.persisted.read().settings.renderer,
                        Arc::clone(&self.tab_state.suppress_gpu_encoder_warning),
                        self.tab_state.window_settings.clone(),
                        self.tab_state.save_notify.clone(),
                        is_debug_mode,
                    );
                    self.tab_state.replay_renderers.lock().push(viewer);
                }
            });

            // --- Row 2: Match context (subdued) ---
            ui.horizontal(|ui| {
                let weak = ui.visuals().weak_text_color();
                if !self_state.clan().is_empty() {
                    ui.label(RichText::new(format!("[{}]", self_state.clan())).color(weak));
                }
                ui.label(RichText::new(self_state.username()).color(weak));
                ui.label(RichText::new("\u{00B7}").color(weak));
                ui.label(
                    RichText::new(wowsunpack::game_params::translations::translate_game_mode(
                        &report.game_type().to_string(),
                        metadata_provider,
                    ))
                    .color(weak),
                );
                ui.label(RichText::new("\u{00B7}").color(weak));
                ui.label(RichText::new(report.version().to_path()).color(weak));
                ui.label(RichText::new("\u{00B7}").color(weak));
                ui.label(RichText::new(report.game_mode()).color(weak));
                ui.label(RichText::new("\u{00B7}").color(weak));
                ui.label(RichText::new(report.map_name()).color(weak));

                if let Some(ui_report) = replay_file.ui_report.as_ref() {
                    let mut team_damage = 0u64;
                    let mut red_team_damage = 0u64;
                    for vehicle_report in &ui_report.player_reports {
                        if vehicle_report.relation().is_enemy() {
                            red_team_damage += vehicle_report.actual_damage.unwrap_or(0);
                        } else {
                            team_damage += vehicle_report.actual_damage.unwrap_or(0);
                        }
                    }

                    ui.label(RichText::new("\u{00B7}").color(weak));
                    let locale_val = self.tab_state.persisted.read().settings.app.locale.clone();
                    let locale = locale_val.as_ref().map(|s| s.as_ref());
                    let mut job = LayoutJob::default();
                    let weak_fmt = TextFormat { color: weak, ..Default::default() };
                    job.append(&t!("ui.replay.team_damage"), 0.0, weak_fmt.clone());
                    job.append(
                        &separate_number(team_damage, locale),
                        0.0,
                        TextFormat { color: ui.sem().win, ..Default::default() },
                    );
                    job.append(" : ", 0.0, weak_fmt.clone());
                    job.append(
                        &separate_number(red_team_damage, locale),
                        0.0,
                        TextFormat { color: ui.sem().loss, ..Default::default() },
                    );
                    job.append(
                        &format!(" ({})", separate_number(team_damage + red_team_damage, locale)),
                        0.0,
                        weak_fmt,
                    );
                    ui.label(job);
                }
            });

            // Synchronize the hide_my_stats value
            if hide_my_stats_changed
                && let Some(ui_report) = replay_file.ui_report.as_mut()
                && let Some(self_report) =
                    ui_report.player_reports.iter_mut().find(|report| report.relation().is_self())
            {
                self_report.manual_stat_hide_toggle = hide_my_stats;
            }

            egui::CentralPanel::default().show(ui, |ui| {
                if let Some(ui_report) = replay_file.ui_report.as_mut() {
                    ui_report.debug_mode = self.tab_state.persisted.read().settings.app.debug_mode;
                    self.build_replay_player_list(ui_report, ui);
                }
            });
        }
    }

    fn build_file_listing(&mut self, ui: &mut egui::Ui, ws_id: WorkspaceId) {
        let grouping = self.tab_state.persisted.read().settings.replay.grouping;

        match grouping {
            ReplayGrouping::None => self.build_file_listing_ungrouped(ui, ws_id),
            ReplayGrouping::Date | ReplayGrouping::Ship => self.build_file_listing_grouped(ui, ws_id, grouping),
        }
    }

    fn build_file_listing_ungrouped(&mut self, ui: &mut egui::Ui, ws_id: WorkspaceId) {
        let mut replay_to_open: Option<Arc<RwLock<Replay>>> = None;
        let mut replay_to_open_new: Option<Arc<RwLock<Replay>>> = None;

        // Existence of `ws_id` is guaranteed by the caller (`build_replay_parser_tab`
        // checks it before drawing any tab content), so this and the other
        // `workspace(ws_id)` reads below are justified in assuming `Some`.
        if let Some(mut files) = self
            .tab_state
            .workspace(ws_id)
            .expect("ws_id checked present by build_replay_parser_tab")
            .replay_files
            .as_ref()
            .map(|files| files.iter().map(|(x, y)| (x.clone(), y.clone())).collect::<Vec<_>>())
        {
            files.sort_by(|a, b| b.0.cmp(&a.0));

            let metadata_provider = self.metadata_provider().unwrap();
            let focused = self.tab_state.workspace(ws_id).expect("checked above").focused_replay();
            let locale = self.tab_state.persisted.read().settings.app.locale.clone();
            let wows_dir = self.tab_state.persisted.read().settings.game.wows_dir.clone();
            let row_summaries = &self.tab_state.workspace(ws_id).expect("checked above").replay_row_summaries;
            let font_id = egui::TextStyle::Body.resolve(ui.style());

            // `show_rows` takes the height without spacing and adds `item_spacing.y` itself,
            // so this is just the two-line galley.
            let row_height = ui.text_style_height(&egui::TextStyle::Body) * 2.0;

            egui::ScrollArea::both().id_salt(workspace_salt(ws_id, "replay_listing_scroll_area")).show_rows(
                ui,
                row_height,
                files.len(),
                |ui, range| {
                    for (path, replay) in &files[range] {
                        let replay_guard = replay.read();
                        let identity = listing_row::replay_row_identity(&replay_guard, &metadata_provider);
                        let parsed = listing_row::replay_parsed_stats(&replay_guard);
                        drop(replay_guard);

                        let summary = row_summaries.get(path);
                        let stats = listing_row::resolve_row_stats(parsed, summary);
                        let identity_text = listing_row::identity_line(&identity, ReplayGrouping::None);
                        let stats_text =
                            listing_row::stats_line(&identity, &stats, ReplayGrouping::None, locale.as_deref());

                        let is_selected = focused.as_ref().map(|c| Arc::ptr_eq(c, replay)).unwrap_or(false);
                        let label_text = listing_row::row_layout_job(
                            &identity_text,
                            &stats_text,
                            &stats,
                            is_selected,
                            ui.visuals(),
                            font_id.clone(),
                        );
                        let hover = listing_row::hover_text(&identity, &stats, locale.as_deref());

                        let replay_weak = Arc::downgrade(replay);
                        let path_clone = path.clone();
                        let wows_dir_clone = wows_dir.clone();
                        let label_response = ui
                            .add(
                                Label::new(label_text)
                                    .selectable(false)
                                    .sense(Sense::click())
                                    // The scroll area hands rows its full visible width, so a
                                    // panel dragged narrower than the auto-sized width would
                                    // otherwise wrap to a third line that `show_rows` has not
                                    // reserved height for, overlapping the row below.
                                    .wrap_mode(egui::TextWrapMode::Truncate),
                            )
                            .on_hover_text(hover);
                        label_response.context_menu(|ui| {
                            show_leaf_context_menu(ui, &replay_weak, &path_clone, &wows_dir_clone, ws_id);
                        });

                        if label_response.double_clicked() {
                            replay_to_open = Some(Arc::clone(replay));
                        }
                    }
                },
            );
        }

        self.handle_context_menu_render(ui, ws_id);
        self.handle_batch_render_request(ui, ws_id);
        self.handle_replay_open_actions(ui, ws_id, &mut replay_to_open, &mut replay_to_open_new);
    }

    fn build_file_listing_grouped(&mut self, ui: &mut egui::Ui, ws_id: WorkspaceId, grouping: ReplayGrouping) {
        // Existence of `ws_id` is guaranteed by the caller; see
        // `build_file_listing_ungrouped` for the same justification.
        let Some(mut files) = self
            .tab_state
            .workspace(ws_id)
            .expect("ws_id checked present by build_replay_parser_tab")
            .replay_files
            .as_ref()
            .map(|files| files.iter().map(|(x, y)| (x.clone(), y.clone())).collect::<Vec<_>>())
        else {
            return;
        };

        files.sort_by(|a, b| b.0.cmp(&a.0));
        let files_len = files.len();
        let metadata_provider = self.metadata_provider().unwrap();

        egui::ScrollArea::both().id_salt(workspace_salt(ws_id, "replay_listing_scroll_area")).show(ui, |ui| {
            // Build groups based on grouping mode
            let (groups, group_id_salt, tree_id_salt) = match grouping {
                ReplayGrouping::Date => {
                    let mut groups: Vec<ReplayGroup> = Vec::new();
                    for (path, replay) in files {
                        let game_time = replay.read().game_time().to_string();
                        let date = game_time.split(' ').next().unwrap_or(&game_time).to_string();
                        if let Some((last_date, last_group)) = groups.last_mut()
                            && *last_date == date
                        {
                            last_group.push((path, replay));
                            continue;
                        }
                        groups.push((date, vec![(path, replay)]));
                    }
                    (groups, "date_group", "replay_date_tree")
                }
                ReplayGrouping::Ship => {
                    let mut ship_groups: HashMap<String, Vec<ReplayEntry>> = HashMap::new();
                    let mut ship_most_recent: HashMap<String, std::path::PathBuf> = HashMap::new();
                    for (path, replay) in files {
                        let ship_name = replay.read().vehicle_name(&metadata_provider);
                        ship_groups.entry(ship_name.clone()).or_default().push((path.clone(), replay));
                        ship_most_recent.entry(ship_name).or_insert(path);
                    }
                    let mut groups: Vec<ReplayGroup> = ship_groups.into_iter().collect();
                    groups.sort_by(|a, b| {
                        let a_recent = ship_most_recent.get(&a.0).unwrap();
                        let b_recent = ship_most_recent.get(&b.0).unwrap();
                        b_recent.cmp(a_recent)
                    });
                    (groups, "ship_group", "replay_ship_tree")
                }
                ReplayGrouping::None => unreachable!(),
            };

            // Build lookup maps for tree node IDs
            let mut id_to_replay: HashMap<egui::Id, Arc<RwLock<Replay>>> = HashMap::new();
            let mut tree_maps = GroupedTreeMaps {
                leaf_replays: HashMap::new(),
                leaf_paths: HashMap::new(),
                group_replays: HashMap::new(),
                group_child_ids: HashMap::new(),
                group_paths: HashMap::new(),
            };

            for (group_name, replays) in &groups {
                let group_id = workspace_group_salt(ws_id, group_id_salt, group_name);
                let mut grp_replays = Vec::new();
                let mut child_ids = Vec::new();
                let mut grp_paths = Vec::new();
                for (path, replay) in replays {
                    let id = workspace_leaf_salt(ws_id, path);
                    id_to_replay.insert(id, replay.clone());
                    tree_maps.leaf_replays.insert(id, Arc::downgrade(replay));
                    tree_maps.leaf_paths.insert(id, path.clone());
                    grp_replays.push(Arc::downgrade(replay));
                    child_ids.push(id);
                    grp_paths.push(path.clone());
                }
                tree_maps.group_replays.insert(group_id, grp_replays);
                tree_maps.group_child_ids.insert(group_id, child_ids);
                tree_maps.group_paths.insert(group_id, grp_paths);
            }

            let fallback_maps = tree_maps.clone();
            let visuals = ui.visuals().clone();
            let font_id = egui::TextStyle::Body.resolve(ui.style());
            let locale = self.tab_state.persisted.read().settings.app.locale.clone();

            // Unlike `ScrollArea::show_rows`, `egui_ltreeview`'s `ui.set_height` call
            // (node.rs) clamps the node's layout rect directly to the height we pass,
            // with no spacing added on top. The two-line label needs that spacing
            // budgeted in here, or it collides with the row below it.
            let leaf_node_height = ui.text_style_height(&egui::TextStyle::Body) * 2.0 + ui.spacing().item_spacing.y;

            if !self
                .tab_state
                .workspace(ws_id)
                .expect("ws_id checked present by build_replay_parser_tab")
                .replay_listing_collapse_defaulted
                && files_len > LARGE_LISTING_THRESHOLD
            {
                let tree_id = ui.make_persistent_id(workspace_salt(ws_id, tree_id_salt));
                ui.ctx().data_mut(|data| {
                    let state = data.get_temp_mut_or_default::<egui_ltreeview::TreeViewState<egui::Id>>(tree_id);
                    for (group_name, _) in &groups {
                        let node_id = workspace_group_salt(ws_id, group_id_salt, group_name);
                        // Only groups the user has never opened or closed.
                        if state.is_open(&node_id).is_none() {
                            state.set_openness(node_id, false);
                        }
                    }
                });
                self.tab_state.workspace_mut(ws_id).expect("checked above").replay_listing_collapse_defaulted = true;
            }

            // Bound after the collapse-default write: workspace(ws_id) borrows the whole workspace.
            let row_summaries = &self.tab_state.workspace(ws_id).expect("checked above").replay_row_summaries;

            let tree = egui_ltreeview::TreeView::new(ui.make_persistent_id(workspace_salt(ws_id, tree_id_salt)))
                .allow_multi_selection(true)
                .fallback_context_menu(move |ui, selected_ids| {
                    fallback_maps.show_multi_selection_context_menu(ui, selected_ids, ws_id);
                });

            let (response, actions) = tree.show(ui, |builder| {
                for (group_name, replays) in &groups {
                    let win_rate = win_rate_label(replays);
                    let group_id = workspace_group_salt(ws_id, group_id_salt, group_name);
                    let group_replays = tree_maps.group_replays.get(&group_id).cloned().unwrap_or_default();
                    let group_paths = tree_maps.group_paths.get(&group_id).cloned().unwrap_or_default();
                    let dir_node = egui_ltreeview::NodeBuilder::dir(group_id)
                        .label(format!("{} ({}){}", group_name, replays.len(), win_rate))
                        .context_menu(move |ui| {
                            show_group_context_menu(ui, &group_paths, &group_replays, ws_id);
                        });
                    let is_open = builder.node(dir_node);
                    if is_open {
                        for (path, _replay) in replays {
                            let id = workspace_leaf_salt(ws_id, path);
                            let path_clone = path.clone();
                            let wows_dir = self.tab_state.persisted.read().settings.game.wows_dir.clone();
                            let replay_weak = tree_maps.leaf_replays.get(&id).cloned().unwrap();

                            let replay_guard = id_to_replay.get(&id).unwrap().read();
                            let identity = listing_row::replay_row_identity(&replay_guard, &metadata_provider);
                            let parsed = listing_row::replay_parsed_stats(&replay_guard);
                            drop(replay_guard);

                            let summary = row_summaries.get(path);
                            let stats = listing_row::resolve_row_stats(parsed, summary);
                            let identity_text = listing_row::identity_line(&identity, grouping);
                            let stats_text = listing_row::stats_line(&identity, &stats, grouping, locale.as_deref());
                            let label_text = listing_row::row_layout_job(
                                &identity_text,
                                &stats_text,
                                &stats,
                                false,
                                &visuals,
                                font_id.clone(),
                            );
                            let hover = listing_row::hover_text(&identity, &stats, locale.as_deref());
                            let node = egui_ltreeview::NodeBuilder::leaf(id)
                                .height(leaf_node_height)
                                .label_ui(move |ui| {
                                    ui.add(Label::new(label_text.clone()).selectable(false))
                                        .on_hover_text(hover.clone());
                                })
                                .context_menu(move |ui| {
                                    show_leaf_context_menu(ui, &replay_weak, &path_clone, &wows_dir, ws_id);
                                });
                            builder.node(node);
                        }
                    }
                    builder.close_dir();
                }
            });

            self.handle_context_menu_render(ui, ws_id);
            self.handle_batch_render_request(ui, ws_id);

            // Handle tree actions
            let mut replay_to_open: Option<Arc<RwLock<Replay>>> = None;
            let mut replay_to_open_new: Option<Arc<RwLock<Replay>>> = None;
            for action in actions {
                match action {
                    egui_ltreeview::Action::SetSelected(selected_ids) => {
                        let mut expanded_selection: Vec<egui::Id> = Vec::new();
                        let mut needs_expansion = false;
                        for id in &selected_ids {
                            expanded_selection.push(*id);
                            if let Some(child_ids) = tree_maps.group_child_ids.get(id) {
                                for child_id in child_ids {
                                    if !selected_ids.contains(child_id) {
                                        needs_expansion = true;
                                        expanded_selection.push(*child_id);
                                    }
                                }
                            }
                        }
                        if needs_expansion {
                            let tree_id = ui.make_persistent_id(workspace_salt(ws_id, tree_id_salt));
                            ui.ctx().data_mut(|data| {
                                let state =
                                    data.get_temp_mut_or_default::<egui_ltreeview::TreeViewState<egui::Id>>(tree_id);
                                state.set_selected(expanded_selection);
                            });
                        }
                    }
                    egui_ltreeview::Action::Activate(activate) => {
                        for id in activate.selected {
                            if let Some(replay) = id_to_replay.get(&id) {
                                replay_to_open = Some(replay.clone());
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Workaround: egui_ltreeview 0.7 doesn't fire Action::Activate on double-click.
            // Fall back to checking the response directly and opening the selected replay.
            if replay_to_open.is_none() && response.double_clicked() {
                let tree_id = ui.make_persistent_id(workspace_salt(ws_id, tree_id_salt));
                let selected = ui.ctx().data(|data| {
                    data.get_temp::<egui_ltreeview::TreeViewState<egui::Id>>(tree_id)
                        .map(|state| state.selected().clone())
                });
                if let Some(selected_ids) = selected {
                    for id in &selected_ids {
                        if let Some(replay) = id_to_replay.get(id) {
                            replay_to_open = Some(replay.clone());
                            break;
                        }
                    }
                }
            }

            self.handle_replay_open_actions(ui, ws_id, &mut replay_to_open, &mut replay_to_open_new);
        });
    }

    /// Opens a file picker for another `.wowsreplay` recording of the same
    /// match and validates it. If valid, the freshly-loaded `ReplayFile`
    /// plus a Weak pointer to the current Replay are stashed in egui's
    /// transient store for [`Self::handle_pending_alt_reparse`] to pick up -
    /// that handler takes the write lock, appends to `alt_replays`, and
    /// dispatches the re-parse.
    fn load_alt_perspective_action(
        &self,
        ui: &mut egui::Ui,
        replay_file: &Replay,
        replay_weak: &Weak<RwLock<Replay>>,
        ws_id: WorkspaceId,
    ) {
        let Some(file) = rfd::FileDialog::new().add_filter("WoWs Replays", &["wowsreplay"]).pick_file() else {
            return;
        };
        let alt = match ReplayFile::from_file(&file) {
            Ok(r) => r,
            Err(e) => {
                self.tab_state
                    .toasts
                    .lock()
                    .error(t!("ui.replay.load_alt_perspective_failed", error = format!("{e:?}")));
                return;
            }
        };

        // Validate version + arena id up front so a bad pick never lands in
        // alt_replays. Otherwise it would stick around and every subsequent
        // attempt would re-fail on the same stale entry.
        let validation = if alt.meta.clientVersionFromExe != replay_file.replay_file.meta.clientVersionFromExe {
            Err(format!(
                "version mismatch (primary={}, merge={})",
                replay_file.replay_file.meta.clientVersionFromExe, alt.meta.clientVersionFromExe
            ))
        } else {
            let primary_arena = replay_file.battle_report.as_ref().map(|r| r.arena_id());
            let alt_arena = wows_replays::analyzer::battle_controller::merged::scan_arena_id(
                replay_file.resource_loader.entity_specs(),
                wowsunpack::data::Version::from_client_exe(&replay_file.replay_file.meta.clientVersionFromExe),
                &alt,
            );
            match (primary_arena, alt_arena) {
                (Some(p), Some(a)) if p != a => Err(format!("arena id mismatch (primary={p}, merge={a})")),
                (_, None) => Err("could not extract arena id from selected replay".to_string()),
                _ => Ok(()),
            }
        };

        if let Err(msg) = validation {
            self.tab_state.toasts.lock().error(t!("ui.replay.load_alt_perspective_failed", error = msg));
            return;
        }

        // Hand the alt off to the outer loop: we only hold &Replay here, so
        // we can't push to alt_replays or call `deps.load_replay` without
        // first releasing the calling write guard. The next frame's handler
        // upgrades the Weak, takes its own write lock, and re-parses.
        tracing::info!(player = %alt.meta.playerName, "alt-perspective validated; staging for re-parse");
        stash_alt_reparse(
            ui.ctx(),
            PendingAltRequest { workspace: ws_id, replay: replay_weak.clone(), alt: std::sync::Arc::new(alt) },
        );
    }

    /// Check for "Open in New Tab" from context menu, then open replays in the appropriate tab.
    fn handle_replay_open_actions(
        &mut self,
        ui: &mut egui::Ui,
        ws_id: WorkspaceId,
        replay_to_open: &mut Option<Arc<RwLock<Replay>>>,
        replay_to_open_new: &mut Option<Arc<RwLock<Replay>>>,
    ) {
        if let Some(replay) = ui
            .ctx()
            .data_mut(|data| {
                data.remove_temp::<Weak<RwLock<Replay>>>(request_slot_id(ws_id, ReplayRequestSlot::OpenReplayNewTab))
            })
            .and_then(|w| w.upgrade())
        {
            *replay_to_open_new = Some(replay);
        }

        // A double-click or context menu in this listing opens into this
        // listing's own dock, which is why the workspace comes from `ws_id`
        // rather than from whichever workspace is active.
        let Some(workspace) = self.tab_state.workspace_mut(ws_id) else {
            return;
        };
        let opened = if let Some(replay) = replay_to_open_new.take() {
            workspace.open_replay_in_new_tab(Arc::clone(&replay));
            Some(replay)
        } else if let Some(replay) = replay_to_open.take() {
            workspace.open_replay_in_focused_tab(Arc::clone(&replay));
            Some(replay)
        } else {
            None
        };
        if let Some(replay) = opened
            && let Some(deps) = self.tab_state.replay_dependencies()
        {
            update_background_task!(
                self.tab_state.background_tasks,
                deps.load_replay(replay, ReplaySource::FileListing)
            );
        }
    }

    /// Picks up an alt-perspective request raised in a previous frame and runs
    /// it: pushes the alt onto its replay and dispatches the merged re-parse.
    ///
    /// Runs from every replay tab, before any of that tab's own content, and
    /// deliberately not from the listing: the button that raises the request
    /// lives in the replay dock, which draws whether or not the listing beside
    /// it does, so a listing-side consumer would miss any request raised while
    /// the listing is collapsed or empty, leaving a whole `ReplayFile` parked
    /// in egui's store for the session.
    fn handle_pending_alt_reparse(&mut self, ctx: &egui::Context) {
        // The write guard `build_replay_view` holds over the button is long
        // gone by the time we get here, so taking a fresh one is safe.
        let Some(request) = take_alt_reparse(ctx) else {
            return;
        };
        let Some(arc) = request.replay.upgrade() else {
            tracing::warn!(workspace = request.workspace.0, "alt-perspective re-parse: replay was dropped");
            return;
        };
        // Try to unwrap the Arc; if some other handler still holds a
        // reference, clone the inner instead. Either way, the alt
        // lands in `alt_replays` as an owned ReplayFile.
        let alt = std::sync::Arc::try_unwrap(request.alt).unwrap_or_else(|a| (*a).clone());
        let player = alt.meta.playerName.clone();
        let mut guard = arc.write();
        guard.alt_replays.push(alt);
        let count = guard.alt_replays.len();
        // The cached timeline is primary-only; force a re-extract/merge on next open.
        guard.timeline = TimelineState::NotRequested;
        drop(guard);
        tracing::info!(
            player = %player,
            alt_count = count,
            workspace = request.workspace.0,
            "pushed alt-perspective; triggering re-parse"
        );
        if let Some(deps) = self.tab_state.replay_dependencies() {
            update_background_task!(self.tab_state.background_tasks, deps.load_replay(arc, ReplaySource::ManualOpen));
        } else {
            tracing::warn!("alt-perspective re-parse: no replay dependencies available");
        }
    }

    fn build_replay_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(wt_translations::icon_t(icons::FOLDER_OPEN, &t!("ui.replay.open_manually"))).clicked()
                && let Some(file) = rfd::FileDialog::new().add_filter("WoWs Replays", &["wowsreplay"]).pick_file()
            {
                self.tab_state.persisted.write().settings.game.current_replay_path = file.clone();

                if let Some(deps) = self.tab_state.replay_dependencies() {
                    update_background_task!(
                        self.tab_state.background_tasks,
                        deps.parse_replay_from_path(file, ReplaySource::ManualOpen)
                    );
                }
            }

            {
                let mut val = self.tab_state.persisted.read().auto_load_latest_replay;
                if ui.checkbox(&mut val, t!("ui.replay.autoload_latest")).changed() {
                    self.tab_state.persisted.write().auto_load_latest_replay = val;
                }
            }

            {
                let mut grouping = self.tab_state.persisted.read().settings.replay.grouping;
                ComboBox::from_id_salt("replay_grouping")
                    .selected_text(t!("ui.replay.group.prefix", label = grouping.label()))
                    .show_ui(ui, |ui| {
                        let changed = ui
                            .selectable_value(&mut grouping, ReplayGrouping::Date, t!("ui.replay.group.date"))
                            .changed()
                            | ui.selectable_value(&mut grouping, ReplayGrouping::Ship, t!("ui.replay.group.ship"))
                                .changed()
                            | ui.selectable_value(&mut grouping, ReplayGrouping::None, t!("ui.replay.group.none"))
                                .changed();
                        if changed {
                            self.tab_state.persisted.write().settings.replay.grouping = grouping;
                        }
                    });
            }

            ComboBox::from_id_salt("column_filters")
                .selected_text(t!("ui.replay.column_filters"))
                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                .show_ui(ui, |ui| {
                    let mut rs = self.tab_state.persisted.read().settings.replay.clone();
                    let mut changed = false;
                    changed |= ui.checkbox(&mut rs.show_raw_xp, t!("ui.replay.filter.raw_xp")).changed();
                    changed |= ui.checkbox(&mut rs.show_entity_id, t!("ui.replay.filter.entity_id")).changed();
                    changed |=
                        ui.checkbox(&mut rs.show_observed_damage, t!("ui.replay.filter.observed_damage")).changed();
                    changed |= ui.checkbox(&mut rs.show_heals, t!("ui.replay.filter.heals")).changed();
                    if changed {
                        self.tab_state.persisted.write().settings.replay = rs;
                    }
                });

            ui.separator();

            // ── Collab session popover ──
            self.show_session_popover(ui);

            // ── Tactics Board ──
            {
                let has_data = self.tab_state.world_of_warships_data.is_some();
                let board_count = self.tab_state.tactics_boards.lock().len();
                let at_limit = board_count >= crate::collab::protocol::MAX_TACTICS_BOARDS;
                let btn = ui.add_enabled(
                    has_data && !at_limit,
                    egui::Button::new(wt_translations::icon_t(icons::MAP_TRIFOLD, &t!("ui.collab.tactics_board"))),
                );
                let btn = if !has_data {
                    btn.on_hover_text(t!("ui.collab.waiting_for_data"))
                } else if at_limit {
                    btn.on_hover_text(t!("ui.collab.max_boards", count = crate::collab::protocol::MAX_TACTICS_BOARDS))
                } else {
                    btn
                };
                if btn.clicked() {
                    let session_handle =
                        self.tab_state.host_session.as_ref().or(self.tab_state.client_session.as_ref());
                    let owner_user_id =
                        session_handle.map(|_| self.tab_state.session_state.lock().my_user_id).unwrap_or(0);
                    let mut board = crate::replay::minimap_view::tactics::TacticsBoardViewer::new(
                        rand::random(),
                        owner_user_id,
                        std::sync::Arc::clone(&self.tab_state.cap_layout_db),
                        std::sync::Arc::clone(&self.tab_state.renderer_asset_cache),
                        std::sync::Arc::clone(self.tab_state.world_of_warships_data.as_ref().unwrap()),
                        self.tab_state.db_pool.clone(),
                        self.tab_state.tokio_runtime.clone(),
                        self.tab_state.window_settings.clone(),
                        self.tab_state.save_notify.clone(),
                    );
                    if let Some(handle) = session_handle {
                        let is_authority = {
                            let s = self.tab_state.session_state.lock();
                            s.role.is_host() || s.role.is_co_host()
                        };
                        board.is_session_board = is_authority;
                        board.collab_local_tx = Some(handle.local_tx.clone());
                        board.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                        board.collab_command_tx = Some(handle.command_tx.clone());
                    }
                    self.tab_state.tactics_boards.lock().push(board);
                }
            }
        });
    }

    /// Session popover: host, join, and active session controls.
    fn show_session_popover(&mut self, ui: &mut egui::Ui) {
        // Determine active states from tab_state directly.
        let host_status = if self.tab_state.host_session.is_some() {
            Some(self.tab_state.session_state.lock().status.clone())
        } else {
            None
        };
        let host_active = matches!(host_status, Some(SessionStatus::Active) | Some(SessionStatus::Starting));
        let client_active = self.tab_state.client_session.is_some();
        let any_active = host_active || client_active;

        // Session button (turns red when active).
        let label = if any_active {
            RichText::new(wt_translations::icon_t(icons::BROADCAST, &t!("ui.collab.session")))
                .color(crate::ui::theme::contrast::label_on(ui.sem().error))
        } else {
            RichText::new(wt_translations::icon_t(icons::BROADCAST, &t!("ui.collab.session")))
        };
        let mut button = egui::Button::new(label);
        if any_active {
            button = button.fill(ui.sem().error);
        }
        let btn = ui.add(button);

        egui::Popup::from_toggle_button_response(&btn).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(
            |ui| {
                ui.set_min_width(260.0);

                if host_active && matches!(host_status, Some(SessionStatus::Starting)) {
                    // ── Host session is starting ──
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(t!("ui.collab.starting"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(t!("ui.buttons.cancel")).clicked() {
                                if let Some(ref handle) = self.tab_state.host_session {
                                    let _ = handle.command_tx.send(SessionCommand::Stop);
                                }
                                for r in self.tab_state.replay_renderers.lock().iter() {
                                    let mut s = r.shared_state().lock();
                                    s.session_frame_tx = None;
                                    s.collab_replay_id = None;
                                    s.session_announced = false;
                                    s.collab_session_state = None;
                                }
                                self.tab_state.host_session = None;
                                {
                                    let mut s = self.tab_state.session_state.lock();
                                    s.status = SessionStatus::Idle;
                                    s.connected_users.clear();
                                    s.cursors.clear();
                                    s.token = None;
                                    s.open_replays.clear();
                                }
                            }
                        });
                    });
                } else if host_active {
                    // ── Active host session controls ──
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t!("ui.collab.session_active").as_ref()).strong().color(ui.sem().error));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(t!("ui.buttons.stop")).clicked() {
                                if let Some(ref handle) = self.tab_state.host_session {
                                    let _ = handle.command_tx.send(SessionCommand::Stop);
                                }
                                for r in self.tab_state.replay_renderers.lock().iter() {
                                    let mut s = r.shared_state().lock();
                                    s.session_frame_tx = None;
                                    s.collab_replay_id = None;
                                    s.session_announced = false;
                                    s.collab_session_state = None;
                                }
                                self.tab_state.host_session = None;
                                {
                                    let mut s = self.tab_state.session_state.lock();
                                    s.status = SessionStatus::Idle;
                                    s.connected_users.clear();
                                    s.cursors.clear();
                                    s.token = None;
                                    s.open_replays.clear();
                                }
                            }
                        });
                    });
                    ui.separator();

                    // Token display
                    let token = self.tab_state.session_state.lock().token.clone().unwrap_or_default();
                    if !token.is_empty() {
                        ui.label(t!("ui.collab.session_token"));
                        let visible = self.tab_state.session_token_visible;
                        ui.horizontal(|ui| {
                            let mut display_token = token.clone();
                            let te = egui::TextEdit::singleline(&mut display_token)
                                .password(!visible)
                                .interactive(false)
                                .desired_width(160.0);
                            ui.add(te);

                            let eye_icon = if visible { icons::EYE } else { icons::EYE_SLASH };
                            if ui.button(eye_icon).on_hover_text(t!("ui.collab.toggle_visibility")).clicked() {
                                self.tab_state.session_token_visible = !visible;
                            }

                            if ui.button(icons::COPY).on_hover_text(t!("ui.collab.copy_token")).clicked() {
                                ui.ctx().copy_text(token.clone());
                                self.tab_state.toasts.lock().info(t!("ui.collab.token_copied"));
                            }
                        });

                        // Copy web link buttons
                        if ui
                            .button(wt_translations::icon_t(icons::BROWSER, &t!("ui.collab.copy_web_link")))
                            .on_hover_text(t!("ui.collab.copy_web_link_tooltip"))
                            .clicked()
                        {
                            let url = format!("{}#{}", crate::collab::WEB_CLIENT_URL, token);
                            ui.ctx().copy_text(url);
                            self.tab_state.toasts.lock().info(t!("ui.collab.web_link_copied"));
                        }
                        #[cfg(debug_assertions)]
                        if ui
                            .button(wt_translations::icon_t(icons::BROWSER, &t!("ui.collab.copy_localhost_link")))
                            .on_hover_text(t!("ui.collab.copy_localhost_tooltip"))
                            .clicked()
                        {
                            let url = format!("http://localhost:8080/#{}", token);
                            ui.ctx().copy_text(url);
                            self.tab_state.toasts.lock().info(t!("ui.collab.localhost_copied"));
                        }

                        ui.add_space(4.0);
                    }

                    // Connected users list
                    let connected_users = self.tab_state.session_state.lock().connected_users.clone();
                    // Exclude self from "connected" count — the host is always connected
                    let my_id = self.tab_state.session_state.lock().my_user_id;
                    let peer_count = connected_users.iter().filter(|u| u.id != my_id).count();
                    ui.horizontal(|ui| {
                        ui.label(icons::USERS);
                        ui.label(t!("ui.collab.connected_count", count = peer_count));
                    });
                    // Show each connected user with color dot, name, and role
                    for user in &connected_users {
                        if user.id == my_id {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            let color = crate::ui::widgets::identity_dot_color(
                                ui.visuals(),
                                user.color[0],
                                user.color[1],
                                user.color[2],
                            );
                            crate::ui::widgets::identity_dot(ui, color);
                            ui.label(&user.name);
                            if user.role == crate::collab::PeerRole::CoHost {
                                ui.label(RichText::new(icons::CROWN).small().color(ui.sem().crown_cohost));
                            }
                            if user.role != crate::collab::PeerRole::Host
                                && user.role != crate::collab::PeerRole::CoHost
                                && ui.small_button(icons::CROWN).on_hover_text(t!("ui.collab.promote_cohost")).clicked()
                                && let Some(ref handle) = self.tab_state.host_session
                            {
                                let _ = handle.command_tx.send(SessionCommand::PromoteToCoHost { user_id: user.id });
                                self.tab_state.toasts.lock().info(t!("ui.collab.promoted_cohost", name = &user.name));
                            }
                        });
                    }

                    ui.add_space(4.0);
                    ui.separator();

                    // Permission controls
                    ui.label(RichText::new(t!("ui.collab.permissions").as_ref()).small().strong());
                    let (mut lock_ann, mut lock_settings) = {
                        let s = self.tab_state.session_state.lock();
                        (s.permissions.annotations_locked, s.permissions.settings_locked)
                    };

                    let mut perms_changed = false;
                    perms_changed |= ui.checkbox(&mut lock_ann, t!("ui.collab.lock_annotations")).changed();
                    perms_changed |= ui.checkbox(&mut lock_settings, t!("ui.collab.lock_settings")).changed();

                    if perms_changed {
                        let perms = Permissions { annotations_locked: lock_ann, settings_locked: lock_settings };
                        self.tab_state.session_state.lock().permissions = perms.clone();
                        if let Some(ref handle) = self.tab_state.host_session {
                            let _ = handle.command_tx.send(SessionCommand::SetPermissions(perms));
                        }
                    }

                    ui.add_space(4.0);
                    if ui
                        .button(t!("ui.collab.reset_overrides"))
                        .on_hover_text(t!("ui.collab.reset_overrides_tooltip"))
                        .clicked()
                        && let Some(ref handle) = self.tab_state.host_session
                    {
                        let _ = handle.command_tx.send(SessionCommand::ResetClientOverrides);
                    }
                } else if client_active {
                    // ── Active client session ──
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(t!("ui.collab.connected_to_session").as_ref()).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(t!("ui.collab.leave")).clicked() {
                                if let Some(ref handle) = self.tab_state.client_session {
                                    let _ = handle.command_tx.send(SessionCommand::Stop);
                                }
                                self.tab_state.client_session = None;
                            }
                        });
                    });
                    ui.separator();

                    // Show connected users
                    let connected_users = self.tab_state.session_state.lock().connected_users.clone();
                    let my_id = self.tab_state.session_state.lock().my_user_id;
                    for user in &connected_users {
                        ui.horizontal(|ui| {
                            let color = crate::ui::widgets::identity_dot_color(
                                ui.visuals(),
                                user.color[0],
                                user.color[1],
                                user.color[2],
                            );
                            crate::ui::widgets::identity_dot(ui, color);
                            if user.id == my_id {
                                ui.label(RichText::new(&user.name).italics());
                                ui.label(RichText::new(t!("ui.collab.you").as_ref()).small().weak());
                            } else {
                                ui.label(&user.name);
                            }
                            match user.role {
                                crate::collab::PeerRole::Host => {
                                    ui.label(RichText::new(icons::CROWN).small().color(ui.sem().crown_host))
                                        .on_hover_text(t!("ui.collab.role_host"));
                                }
                                crate::collab::PeerRole::CoHost => {
                                    ui.label(RichText::new(icons::CROWN).small().color(ui.sem().crown_cohost))
                                        .on_hover_text(t!("ui.collab.role_cohost"));
                                }
                                _ => {}
                            }
                        });
                    }

                    self.show_shared_windows(ui);
                } else {
                    // ── No active session ──

                    // Display name (shared for host + join)
                    if self.tab_state.show_display_name_error {
                        ui.label(
                            RichText::new(t!("ui.collab.display_name_error").as_ref()).color(ui.sem().error).small(),
                        );
                    }
                    ui.label(t!("ui.collab.display_name"));
                    let mut display_name = self.tab_state.persisted.read().settings.collab.display_name.clone();
                    let name_response = ui.add(
                        egui::TextEdit::singleline(&mut display_name)
                            .hint_text(t!("ui.collab.display_name_hint"))
                            .desired_width(160.0)
                            .text_color(if self.tab_state.show_display_name_error {
                                ui.sem().error
                            } else {
                                ui.visuals().text_color()
                            }),
                    );
                    if name_response.changed() {
                        self.tab_state.persisted.write().settings.collab.display_name = display_name;
                    }
                    if self.tab_state.show_display_name_error {
                        ui.painter().rect_stroke(
                            name_response.rect,
                            name_response.rect.height() * 0.15,
                            egui::Stroke::new(1.5, ui.sem().error),
                            egui::StrokeKind::Outside,
                        );
                    }
                    // Clear error when user edits the field
                    if name_response.changed() {
                        self.tab_state.show_display_name_error = false;
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    ui.label(RichText::new(t!("ui.collab.host_session").as_ref()).strong());
                    if ui.button(t!("ui.collab.start_session")).clicked() {
                        if self.tab_state.persisted.read().settings.collab.display_name.trim().is_empty() {
                            self.tab_state.show_display_name_error = true;
                            self.tab_state.toasts.lock().error(t!("ui.collab.enter_display_name"));
                        } else {
                            self.tab_state.pending_host = true;
                            if !self.tab_state.persisted.read().settings.collab.suppress_p2p_ip_warning {
                                self.tab_state.show_ip_warning = true;
                            }
                        }
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // ── Join a session ──
                    ui.label(RichText::new(t!("ui.collab.join_session").as_ref()).strong());
                    ui.add_space(2.0);

                    // Paste token -> validate -> auto-join
                    if ui.button(wt_translations::icon_t(icons::CLIPBOARD, &t!("ui.collab.paste_and_join"))).clicked()
                        && let Ok(mut clipboard) = arboard::Clipboard::new()
                        && let Ok(text) = clipboard.get_text()
                    {
                        let trimmed = text.trim().to_string();
                        if trimmed.is_empty() {
                            self.tab_state.toasts.lock().error(t!("ui.collab.clipboard_empty"));
                        } else if self.tab_state.persisted.read().settings.collab.display_name.trim().is_empty() {
                            self.tab_state.show_display_name_error = true;
                            self.tab_state.toasts.lock().error(t!("ui.collab.enter_display_name"));
                        } else if let Err(e) = crate::collab::protocol::decode_token(&trimmed) {
                            self.tab_state.toasts.lock().error(t!("ui.collab.invalid_token", error = e));
                        } else {
                            self.tab_state.join_session_token = trimmed;
                            self.tab_state.pending_join = true;
                            if !self.tab_state.persisted.read().settings.collab.suppress_p2p_ip_warning {
                                self.tab_state.show_ip_warning = true;
                            }
                        }
                    }
                }
            },
        );
    }

    /// Show the "Shared Windows" section inside the session popover.
    /// Lists open replays and tactics boards with Open / Open-for-everyone buttons.
    fn show_shared_windows(&mut self, ui: &mut egui::Ui) {
        let ss = self.tab_state.session_state.lock();
        let open_replays = ss.open_replays.clone();
        let session_boards: Vec<(u64, u64, String)> = ss
            .tactics_boards
            .iter()
            .map(|(&bid, bs)| {
                let title = if !bs.window_title.is_empty() {
                    bs.window_title.clone()
                } else if !bs.tactics_map.display_name.is_empty() {
                    format!("Tactics Board \u{2014} {}", bs.tactics_map.display_name)
                } else if !bs.tactics_map.map_name.is_empty() {
                    format!("Tactics Board \u{2014} {}", bs.tactics_map.map_name)
                } else {
                    "Tactics Board".to_string()
                };
                (bid, bs.owner_user_id, title)
            })
            .collect();
        let is_host_role = ss.role.is_host();
        let connected_users = ss.connected_users.clone();
        drop(ss);

        if open_replays.is_empty() && session_boards.is_empty() {
            return;
        }

        ui.add_space(4.0);
        ui.separator();
        ui.label(RichText::new(t!("ui.collab.shared_windows").as_ref()).small().strong());

        // ── Replays ──
        let renderers = self.tab_state.replay_renderers.lock();
        // Only count visible (open) renderers as "active" — hidden ones show an Open button.
        let visible_replay_ids: Vec<u64> = renderers
            .iter()
            .filter(|r| r.open.load(std::sync::atomic::Ordering::Relaxed))
            .filter_map(|r| r.shared_state().lock().collab_replay_id)
            .collect();
        drop(renderers);

        for replay in &open_replays {
            let is_visible = visible_replay_ids.contains(&replay.replay_id);
            ui.horizontal(|ui| {
                let name = if replay.replay_name.len() > 40 {
                    format!("{}…", &replay.replay_name[..39])
                } else {
                    replay.replay_name.clone()
                };
                let label = format!("{} {}", icons::MONITOR, name);
                if is_visible {
                    ui.label(&label);
                } else {
                    ui.label(RichText::new(&label).weak());
                    if ui.small_button(t!("ui.collab.open")).clicked() {
                        let renderers = self.tab_state.replay_renderers.lock();
                        // Check for an existing hidden viewer we can reuse.
                        let existing = renderers
                            .iter()
                            .find(|r| r.shared_state().lock().collab_replay_id == Some(replay.replay_id));
                        if let Some(viewer) = existing {
                            // Reuse: show the hidden viewer and re-wire its frame channel.
                            viewer.open.store(true, std::sync::atomic::Ordering::Relaxed);
                            if self.tab_state.client_session.is_some() {
                                let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel(2);
                                let viewport_id = egui::ViewportId::from_hash_of(&*viewer.title);
                                viewer.shared_state().lock().collab_frame_rx = Some(frame_rx);
                                self.tab_state.session_state.lock().register_viewport_sink(
                                    replay.replay_id,
                                    crate::collab::ViewportSink { frame_tx: Some(frame_tx), viewport_id },
                                );
                            }
                        } else {
                            // No hidden viewer — create a fresh one.
                            drop(renderers);
                            let saved_options = self.tab_state.persisted.read().settings.renderer.clone();
                            let suppress = std::sync::Arc::clone(&self.tab_state.suppress_gpu_encoder_warning);
                            let is_debug_mode = self.tab_state.persisted.read().settings.app.debug_mode;
                            let viewer = crate::replay::renderer::launch_client_renderer(
                                replay.replay_name.clone(),
                                replay.map_image_png.clone(),
                                replay.game_version.clone(),
                                &saved_options,
                                suppress,
                                self.tab_state.world_of_warships_data.as_ref(),
                                &self.tab_state.renderer_asset_cache,
                                self.tab_state.window_settings.clone(),
                                self.tab_state.save_notify.clone(),
                                is_debug_mode,
                            );
                            if let Some(ref client_handle) = self.tab_state.client_session {
                                let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel(2);
                                let viewport_id = egui::ViewportId::from_hash_of(&*viewer.title);
                                let mut state = viewer.shared_state().lock();
                                state.collab_replay_id = Some(replay.replay_id);
                                state.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                                state.collab_local_tx = Some(client_handle.local_tx.clone());
                                state.collab_frame_rx = Some(frame_rx);
                                self.tab_state.session_state.lock().register_viewport_sink(
                                    replay.replay_id,
                                    crate::collab::ViewportSink { frame_tx: Some(frame_tx), viewport_id },
                                );
                            }
                            self.tab_state.replay_renderers.lock().push(viewer);
                        }
                    }
                }
            });
        }

        // ── Tactics Boards ──
        let local_boards = self.tab_state.tactics_boards.lock();
        let local_board_ids: Vec<u64> = local_boards.iter().map(|b| b.board_id).collect();
        drop(local_boards);

        let session_handle = self.tab_state.host_session.as_ref().or(self.tab_state.client_session.as_ref());

        for (bid, owner_uid, title) in &session_boards {
            let is_open_locally = local_board_ids.contains(bid);
            let owner_name =
                connected_users.iter().find(|u| u.id == *owner_uid).map(|u| u.name.as_str()).unwrap_or("unknown");
            ui.horizontal(|ui| {
                let display_title = if title.len() > 40 { format!("{}…", &title[..39]) } else { title.clone() };
                let label = format!("{} {} ({})", icons::MAP_TRIFOLD, display_title, owner_name);
                if is_open_locally {
                    ui.label(&label);
                } else {
                    ui.label(RichText::new(&label).weak());
                    if ui.small_button(t!("ui.collab.open")).clicked()
                        && let Some(ref wows_data) = self.tab_state.world_of_warships_data
                    {
                        let board_count = self.tab_state.tactics_boards.lock().len();
                        if board_count < crate::collab::protocol::MAX_TACTICS_BOARDS {
                            let mut board = crate::replay::minimap_view::tactics::TacticsBoardViewer::new(
                                *bid,
                                *owner_uid,
                                std::sync::Arc::clone(&self.tab_state.cap_layout_db),
                                std::sync::Arc::clone(&self.tab_state.renderer_asset_cache),
                                std::sync::Arc::clone(wows_data),
                                self.tab_state.db_pool.clone(),
                                self.tab_state.tokio_runtime.clone(),
                                self.tab_state.window_settings.clone(),
                                self.tab_state.save_notify.clone(),
                            );
                            board.is_session_board = true;
                            if let Some(handle) = session_handle {
                                board.collab_local_tx = Some(handle.local_tx.clone());
                                board.collab_session_state = Some(std::sync::Arc::clone(&self.tab_state.session_state));
                                board.collab_command_tx = Some(handle.command_tx.clone());
                            }
                            self.tab_state.tactics_boards.lock().push(board);
                        }
                    }
                }
                // Host can request all peers to open this window.
                if is_host_role
                    && let Some(handle) = session_handle
                    && ui.small_button(t!("ui.collab.open_for_everyone")).clicked()
                {
                    let _ = handle
                        .command_tx
                        .send(crate::collab::SessionCommand::OpenWindowForEveryone { window_id: *bid });
                }
            });
        }
    }

    /// Builds a `Tab::Replays(ws_id)` tab's body, drawing strictly the workspace
    /// `ws_id` names. Also makes `ws_id` the app-wide active workspace so the
    /// floating chat, timeline, and replay-controls windows (which have no tab
    /// payload of their own) follow whichever replay tab was drawn last.
    pub fn build_replay_parser_tab(&mut self, ui: &mut egui::Ui, ws_id: WorkspaceId) {
        self.tab_state.set_active_workspace(ws_id);

        // Ahead of the workspace check below so a request raised in a workspace
        // that has since closed still runs: it carries its own replay handle
        // and needs nothing from the workspace it came from.
        self.handle_pending_alt_reparse(ui.ctx());

        // A tab can outlive its workspace (e.g. a stale split after the
        // workspace's owning tab was closed). Showing a placeholder here
        // instead of falling back to another workspace is the whole point of
        // this method taking `ws_id` explicitly. Both arms fall through to the
        // same four floating-window calls at the end of this function instead
        // of returning early, so a closed workspace never skips them.
        if self.tab_state.workspace_for_tab(ws_id).is_some() {
            self.refresh_row_summaries(ws_id);
            // Nothing can close ws_id's workspace during this function's own
            // execution: there is no reentrant call back into
            // `build_replay_parser_tab` (or anything it calls) between the
            // existence check above and this read. `unwrap_or(false)` is still
            // the right shape here rather than `.expect(...)`: if that ever
            // stopped holding, "nothing to scan" is the safe reading, not a panic.
            let needs_reindex_scan = self
                .tab_state
                .workspace_mut(ws_id)
                .map(|workspace| std::mem::take(&mut workspace.replay_rows_need_reindex_scan))
                .unwrap_or(false);
            if needs_reindex_scan {
                self.queue_stale_rows_for_reindex(ws_id);
            }
            ui.vertical(|ui| {
                self.build_replay_header(ui);

                {
                    let panel_id = workspace_salt(ws_id, "replay_listing_panel");
                    let collapsed_before = self.tab_state.persisted.read().settings.replay.listing_collapsed;
                    // `show_collapsible` tracks the expanded state, and flips it itself when the
                    // user drags or double-clicks the resize edge.
                    let mut expanded = !collapsed_before;

                    // Auto-size the panel to the widest label when files are first populated.
                    // Uses a flag on TabState (not egui temp data) to survive GC. Deferred while
                    // collapsed, so re-expanding does not clobber a width the user chose, and
                    // deferred until a summary load has completed, since measuring before the
                    // stats line exists would latch a width fitted to "not indexed".
                    // `ws_id`'s workspace existence was checked before this function reached
                    // `ui.vertical`. No reentrant call back into `build_replay_parser_tab` (or
                    // anything it calls) happens between that check and here, so every
                    // `workspace(ws_id)` / `workspace_mut(ws_id)` in this block is justified
                    // in assuming `Some`.
                    let has_files = self
                        .tab_state
                        .workspace(ws_id)
                        .expect("ws_id checked present at function entry")
                        .replay_files
                        .as_ref()
                        .is_some_and(|f| !f.is_empty());

                    let mut default_width = 250.0f32;

                    if has_files
                        && expanded
                        && !self
                            .tab_state
                            .workspace(ws_id)
                            .expect("ws_id checked present at function entry")
                            .replay_listing_auto_sized
                        && self
                            .tab_state
                            .workspace(ws_id)
                            .expect("ws_id checked present at function entry")
                            .replay_row_summaries_loaded
                        && let Some(metadata_provider) = self.metadata_provider()
                    {
                        let grouping = self.tab_state.persisted.read().settings.replay.grouping;
                        let locale = self.tab_state.persisted.read().settings.app.locale.clone();
                        let font_id = egui::TextStyle::Body.resolve(ui.style());
                        let max_width = self
                            .tab_state
                            .workspace(ws_id)
                            .expect("ws_id checked present at function entry")
                            .replay_files
                            .as_ref()
                            .unwrap()
                            .iter()
                            .map(|(path, replay)| {
                                let guard = replay.read();
                                let identity = listing_row::replay_row_identity(&guard, &metadata_provider);
                                let parsed = listing_row::replay_parsed_stats(&guard);
                                drop(guard);
                                let summary = self
                                    .tab_state
                                    .workspace(ws_id)
                                    .expect("ws_id checked present at function entry")
                                    .replay_row_summaries
                                    .get(path);
                                let stats = listing_row::resolve_row_stats(parsed, summary);
                                let identity_text = listing_row::identity_line(&identity, grouping);
                                let stats_text =
                                    listing_row::stats_line(&identity, &stats, grouping, locale.as_deref());
                                [identity_text, stats_text]
                                    .iter()
                                    .map(|line| {
                                        ui.painter()
                                            .layout_no_wrap(line.to_string(), font_id.clone(), ui.sem().text_strong)
                                            .size()
                                            .x
                                    })
                                    .fold(0.0f32, f32::max)
                            })
                            .fold(0.0f32, f32::max);

                        // Allowance for tree indentation, the right margin, and the scrollbar.
                        // Line 1 carries at most one trailing glyph (the division icon), and
                        // the stats line's icons are already part of the measured text above,
                        // so this no longer needs to budget for a second glyph.
                        default_width = (max_width + 44.0).max(200.0);

                        self.tab_state
                            .workspace_mut(ws_id)
                            .expect("ws_id checked present at function entry")
                            .replay_listing_auto_sized = true;

                        // Clear stored panel state so default_width takes effect
                        ui.ctx().data_mut(|d| {
                            d.remove::<egui::containers::panel::PanelState>(panel_id);
                        });
                    }

                    egui::Panel::left(panel_id)
                        .default_size(default_width)
                        .size_range(REPLAY_LISTING_MIN_WIDTH..=f32::INFINITY)
                        // Left margin is zero so labels sit flush; the right margin keeps them
                        // clear of the resize divider.
                        .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin {
                            left: 0,
                            right: 8,
                            top: 2,
                            bottom: 2,
                        }))
                        .show_collapsible(ui, &mut expanded, |ui| {
                            // egui_ltreeview 0.8.0 draw_indent_hint clamps against an
                            // un-normalized clip rect. `show_collapsible` slides the panel
                            // off-screen rather than shrinking it, so the clip rect - not
                            // the panel width - is what collapses and inverts mid-animation;
                            // an inverted rect makes the clamp panic. Skip drawing the tree
                            // until we're past it.
                            let clip_width = ui.clip_rect().width();
                            if clip_width < REPLAY_LISTING_MIN_WIDTH {
                                return;
                            }
                            self.build_file_listing(ui, ws_id);
                        });

                    egui::Panel::left(workspace_salt(ws_id, "replay_listing_rail"))
                        .exact_size(20.0)
                        .resizable(false)
                        .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::same(0)))
                        .show(ui, |ui| {
                            let icon = if expanded { icons::CARET_LEFT } else { icons::CARET_RIGHT };
                            let tooltip = if expanded {
                                t!("ui.replay.collapse_listing")
                            } else {
                                t!("ui.replay.expand_listing")
                            };

                            // The listing panel's resize divider is registered on its own
                            // right edge, i.e. exactly where this rail begins (the listing
                            // is declared first). Cede that strip to the divider so a
                            // full-rail click can't swallow the resize / drag-to-collapse
                            // gesture.
                            let mut rect = ui.max_rect();
                            rect.min.x += ui.style().interaction.resize_grab_radius_side;

                            let response = ui.interact(rect, ui.id().with("listing_rail_toggle"), egui::Sense::click());
                            if response.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    ui.visuals().widgets.hovered.corner_radius,
                                    ui.visuals().widgets.hovered.weak_bg_fill,
                                );
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }

                            let fg_color = if response.hovered() {
                                ui.visuals().widgets.hovered.fg_stroke.color
                            } else {
                                ui.visuals().widgets.inactive.fg_stroke.color
                            };
                            ui.painter().text(
                                egui::pos2(rect.center().x, rect.top() + 14.0),
                                egui::Align2::CENTER_CENTER,
                                icon,
                                egui::TextStyle::Button.resolve(ui.style()),
                                fg_color,
                            );

                            if !expanded {
                                paint_vertical_caption(ui, rect, 28.0, &t!("ui.replay.listing_caption"));
                            }

                            if response.on_hover_text(tooltip).clicked() {
                                expanded = !expanded;
                            }
                        });

                    // `show_collapsible` may have flipped `expanded` itself via a resize drag or a
                    // double-click on the edge, so persist on any change, not just a caret click.
                    let collapsed_now = !expanded;
                    if collapsed_now != collapsed_before {
                        self.tab_state.persisted.write().settings.replay.listing_collapsed = collapsed_now;
                    }
                }

                egui::CentralPanel::default().show(ui, |ui| {
                    let has_tabs = self
                        .tab_state
                        .workspace(ws_id)
                        .expect("ws_id checked present at function entry")
                        .replay_dock_state
                        .iter_all_tabs()
                        .next()
                        .is_some();
                    if has_tabs {
                        let mut dock_state = std::mem::replace(
                            &mut self
                                .tab_state
                                .workspace_mut(ws_id)
                                .expect("ws_id checked present at function entry")
                                .replay_dock_state,
                            egui_dock::DockState::new(vec![]),
                        );
                        let mut viewer = ReplayTabViewer { tab_state: self.tab_state, workspace: ws_id };
                        egui_dock::DockArea::new(&mut dock_state)
                            .id(workspace_salt(ws_id, "replay_parser_dock"))
                            .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                            .show_close_buttons(true)
                            .show_leaf_collapse_buttons(false)
                            .show_leaf_close_all_buttons(false)
                            .allowed_splits(egui_dock::AllowedSplits::All)
                            .show_inside(ui, &mut viewer);
                        self.tab_state
                            .workspace_mut(ws_id)
                            .expect("ws_id checked present at function entry")
                            .replay_dock_state = dock_state;
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.heading(t!("ui.replay.no_selection"));
                        });
                    }
                });
            });
        } else {
            ui.centered_and_justified(|ui| {
                ui.heading(t!("ui.replay.workspace_closed"));
            });
        }

        // These four resolve through the active workspace (set at the top of
        // this function) or are workspace-independent outright, never through
        // ws_id's own workspace, so both branches above reach them. They must
        // stay after the normal path's dock write-back
        // (`replay_dock_state = dock_state` above): `focused_replay()` reads
        // `replay_dock_state.focused_leaf()`, so running them any earlier
        // renders the previously focused sub-tab's chat and timeline for a
        // frame after every sub-tab switch.
        self.show_game_chat_window(ui.ctx());
        self.show_timeline_window(ui.ctx());
        self.pick_up_replay_controls_request(ui.ctx());
        self.show_replay_controls_window(ui.ctx());
    }

    /// Reload the listing's index-sourced row data when the index has been
    /// written since the last load attempt. `index_generation` is bumped after
    /// every index write, so this is a cheap per-frame comparison rather than a
    /// query.
    fn refresh_row_summaries(&mut self, ws_id: WorkspaceId) {
        // `ws_id` existence is checked by the caller (`build_replay_parser_tab`)
        // before this runs, so every `workspace(ws_id)` / `workspace_mut(ws_id)`
        // below is justified in assuming `Some`.
        let generation = crate::data::replay_index::index_generation();
        let workspace = self.tab_state.workspace(ws_id).expect("ws_id checked present by build_replay_parser_tab");
        if !listing_row::should_reload_summaries(
            workspace.replay_row_summaries_loading,
            workspace.replay_row_summaries_generation,
            generation,
        ) {
            return;
        }
        // A configured replays dir is what makes a live index source possible at all,
        // so it gates the load even though the query itself does not need the path.
        let deps = match (
            self.tab_state.db_pool.as_ref(),
            self.tab_state.tokio_runtime.as_ref(),
            self.tab_state.workspace(ws_id).expect("checked above").root.as_ref(),
        ) {
            (Some(pool), Some(rt), Some(_)) => Some((pool.clone(), Arc::clone(rt))),
            _ => None,
        };
        let Some((pool, rt)) = deps else {
            return;
        };
        let selector = summary_source_selector(self.tab_state.workspace(ws_id).expect("checked above").source);
        // Stamped here rather than on completion so a query that keeps failing does not
        // re-dispatch on every frame. The next attempt waits for the index to move again.
        let workspace = self.tab_state.workspace_mut(ws_id).expect("checked above");
        workspace.replay_row_summaries_loading = true;
        workspace.replay_row_summaries_generation = Some(generation);
        crate::update_background_task!(
            self.tab_state.background_tasks,
            Some(crate::task::start_load_row_summaries(pool, rt, selector, ws_id, generation))
        );
    }

    /// Hand every listed replay whose index row is out of date to the background
    /// parser. `ModifiedReplay` re-parses and re-indexes without re-uploading,
    /// which is exactly the semantics wanted here.
    ///
    /// Only `Stale` is queued. Detecting mtime drift is what this scan uniquely
    /// knows how to do; a replay with no index row at all belongs to the parser
    /// thread's startup walk, which already carries `record_paths_in_source` as
    /// its ledger. Queueing `Missing` here would re-parse everything that walk
    /// just handled, re-run auto-export over the whole library, and re-parse on
    /// every launch forever for old-version replays that can never index because
    /// their build data is not downloaded.
    fn queue_stale_rows_for_reindex(&mut self, ws_id: WorkspaceId) {
        // `ws_id` existence is checked by the caller (`build_replay_parser_tab`)
        // before this runs, so every `workspace(ws_id)` / `workspace_mut(ws_id)`
        // below is justified in assuming `Some`.
        let Some(sender) = self.tab_state.background_parser_tx.as_ref() else {
            return;
        };
        let Some(files) = self
            .tab_state
            .workspace(ws_id)
            .expect("ws_id checked present by build_replay_parser_tab")
            .replay_files
            .as_ref()
        else {
            return;
        };
        let mut queued: Vec<std::path::PathBuf> = Vec::new();
        for path in files.keys() {
            if self.tab_state.workspace(ws_id).expect("checked above").replay_rows_reindex_requested.contains(path) {
                continue;
            }
            let summary = self.tab_state.workspace(ws_id).expect("checked above").replay_row_summaries.get(path);
            let freshness = listing_row::row_freshness(summary, listing_row::file_mtime_secs(path));
            if !matches!(freshness, listing_row::RowFreshness::Stale) {
                continue;
            }
            if sender.send(crate::task::ReplayBackgroundParserThreadMessage::ModifiedReplay(path.clone())).is_ok() {
                queued.push(path.clone());
            }
        }
        for path in queued {
            self.tab_state.workspace_mut(ws_id).expect("checked above").replay_rows_reindex_requested.insert(path);
        }
    }

    fn show_game_chat_window(&self, ctx: &egui::Context) {
        let mut open: bool = ctx.data(|d| d.get_temp(egui::Id::new("show_game_chat"))).unwrap_or(false);
        if !open {
            return;
        }

        let Some(replay_arc) = self.tab_state.focused_replay() else {
            return;
        };
        let replay_file = replay_arc.read();
        let Some(report) = replay_file.battle_report.as_ref() else {
            return;
        };

        let chat_messages = report.game_chat();
        if chat_messages.is_empty() {
            return;
        }

        let toasts = self.tab_state.toasts.clone();
        let metadata_provider = self.metadata_provider();

        egui::Window::new(wt_translations::icon_t(icons::CHAT_TEXT, &t!("ui.replay.game_chat")))
            .open(&mut open)
            .default_width(CHAT_VIEW_WIDTH)
            .default_height(400.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(wt_translations::icon_t(icons::COPY, &t!("ui.replay.copy_all"))).clicked() {
                        let mut buf = std::io::BufWriter::new(Vec::new());
                        for message in chat_messages {
                            let GameMessage {
                                sender_relation: _,
                                sender_name,
                                channel,
                                message,
                                entity_id: _,
                                player,
                                clock: _,
                            } = message;
                            match player {
                                Some(player) if !player.initial_state().clan().is_empty() => {
                                    let _ = writeln!(
                                        buf,
                                        "[{}] {} ({:?}): {}",
                                        player.initial_state().clan(),
                                        sender_name,
                                        channel,
                                        message
                                    );
                                }
                                _ => {
                                    let _ = writeln!(buf, "{sender_name} ({channel:?}): {message}");
                                }
                            }
                        }
                        let game_chat = String::from_utf8(buf.into_inner().expect("failed to get buf inner"))
                            .expect("failed to convert game chat buffer to string");
                        ui.ctx().copy_text(game_chat);
                        toasts.lock().success(t!("ui.replay.chat_copied"));
                    }
                    if ui.button(wt_translations::icon_t(icons::FLOPPY_DISK, &t!("ui.replay.save_to_file"))).clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!(
                                "{} {} {} - Game Chat.txt",
                                report.game_type(),
                                report.game_mode(),
                                report.map_name()
                            ))
                            .save_file()
                        && let Ok(mut file) = std::fs::File::create(path)
                    {
                        for message in chat_messages {
                            let GameMessage {
                                sender_relation: _,
                                sender_name,
                                channel,
                                message,
                                entity_id: _,
                                player,
                                clock: _,
                            } = message;
                            match player {
                                Some(player) if !player.initial_state().clan().is_empty() => {
                                    let _ = writeln!(
                                        file,
                                        "[{}] {} ({:?}): {}",
                                        player.initial_state().clan(),
                                        sender_name,
                                        channel,
                                        message
                                    );
                                }
                                _ => {
                                    let _ = writeln!(file, "{sender_name} ({channel:?}): {message}");
                                }
                            }
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt("game_chat_window_scroll").show(ui, |ui| {
                    build_replay_chat_content(metadata_provider.as_deref(), chat_messages, ui);
                });
            });

        // Write back the open state (user may have closed the window via X)
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::new("show_game_chat"), open);
        });
    }

    /// Starts a background timeline scan if none is running or cached yet.
    /// Everything the scan needs is cloned out of the write guard before the
    /// guard is dropped - the scan takes seconds and must never hold the lock
    /// the UI reads every frame.
    fn start_timeline_extraction_if_needed(&self, replay_arc: &Arc<RwLock<Replay>>) {
        let mut guard = replay_arc.write();
        if !guard.timeline.should_start() {
            return;
        }
        let raw_meta = guard.replay_file.raw_meta.clone().into_bytes();
        let packet_data = guard.replay_file.packet_data.clone();
        let alt_bytes: Vec<(Vec<u8>, Vec<u8>)> =
            guard.alt_replays.iter().map(|r| (r.raw_meta.clone().into_bytes(), r.packet_data.clone())).collect();
        let resource_loader = guard.resource_loader.clone();
        let game_constants = guard.game_constants.clone();
        guard.timeline = TimelineState::Extracting;
        drop(guard);

        let weak = Arc::downgrade(replay_arc);
        crate::util::thread::spawn_logged("timeline-extraction", move || {
            let primary = match ReplayFile::from_decrypted_parts(raw_meta, packet_data) {
                Ok(replay) => replay,
                Err(source) => {
                    if let Some(arc) = weak.upgrade() {
                        arc.write().timeline =
                            TimelineState::Failed(TimelineExtractionError::Reconstruction { source });
                    }
                    return;
                }
            };

            let primary_result = extract_timeline_events(&primary, &resource_loader, game_constants.as_deref());

            // An alt that fails to reconstruct is skipped: the primary's events are
            // still worth showing, and only a primary failure is fatal to the window.
            let alt_results: Vec<_> = alt_bytes
                .into_iter()
                .filter_map(|(alt_raw_meta, alt_packet_data)| {
                    match ReplayFile::from_decrypted_parts(alt_raw_meta, alt_packet_data) {
                        Ok(alt) => Some(extract_timeline_events(&alt, &resource_loader, game_constants.as_deref())),
                        Err(e) => {
                            tracing::warn!("timeline extraction: skipping alt replay that failed to reconstruct: {e}");
                            None
                        }
                    }
                })
                .collect();

            let merged = merge_timelines(primary_result, alt_results);

            if let Some(arc) = weak.upgrade() {
                arc.write().timeline = TimelineState::Ready(Arc::new(merged));
            }
        });
    }

    fn show_timeline_window(&self, ctx: &egui::Context) {
        let mut open: bool = ctx.data(|d| d.get_temp(egui::Id::new("show_timeline"))).unwrap_or(false);
        if !open {
            return;
        }

        let Some(replay_arc) = self.tab_state.focused_replay() else {
            return;
        };

        self.start_timeline_extraction_if_needed(&replay_arc);

        let filter_id = egui::Id::new("timeline_window_filter");
        let mut filter = ctx.data_mut(|d| d.get_temp_mut_or_default::<TimelineFilter>(filter_id).clone());
        let mut retry_requested = false;

        {
            let replay = replay_arc.read();
            egui::Window::new(wt_translations::icon_t(icons::LIST_BULLETS, &t!("ui.replay.timeline")))
                .open(&mut open)
                .default_width(CHAT_VIEW_WIDTH)
                .default_height(400.0)
                .resizable(true)
                .show(ctx, |ui| match &replay.timeline {
                    TimelineState::NotRequested | TimelineState::Extracting => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(t!("ui.replay.timeline_parsing"));
                        });
                    }
                    TimelineState::Failed(err) => {
                        ui.label(
                            RichText::new(t!("ui.replay.timeline_failed", error = err.to_string()).as_ref())
                                .color(Color32::LIGHT_RED),
                        );
                        if ui.button(t!("ui.replay.timeline_retry")).clicked() {
                            retry_requested = true;
                        }
                    }
                    TimelineState::Ready(result) => {
                        let visible_count = result.events.iter().filter(|e| filter.matches(e)).count();

                        ui.horizontal(|ui| {
                            timeline_filter_bar(ui, &mut filter);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add_enabled(visible_count > 0, egui::Button::new(t!("ui.replay.save_to_file")))
                                    .on_hover_text(t!("ui.replay.timeline_export_tooltip"))
                                    .clicked()
                                    && let Some(path) = rfd::FileDialog::new()
                                        .set_file_name(format!(
                                            "{} - Match Timeline.txt",
                                            replay.map_name(&replay.resource_loader)
                                        ))
                                        .save_file()
                                    && let Ok(mut file) = std::fs::File::create(path)
                                {
                                    for event in result.events.iter().filter(|e| filter.matches(e)) {
                                        let _ = writeln!(file, "{}", format_timeline_event(event));
                                    }
                                }
                                if ui
                                    .add_enabled(visible_count > 0, egui::Button::new(t!("ui.buttons.copy")))
                                    .on_hover_text(t!("ui.replay.timeline_export_tooltip"))
                                    .clicked()
                                {
                                    let text = result
                                        .events
                                        .iter()
                                        .filter(|e| filter.matches(e))
                                        .map(format_timeline_event)
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    ui.ctx().copy_text(text);
                                    self.tab_state.toasts.lock().success(t!("ui.replay.timeline_copied"));
                                }
                            });
                        });
                        ui.separator();

                        if !result.events.is_empty() && visible_count == 0 {
                            ui.label(t!("ui.replay.timeline_no_matches"));
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt("timeline_window_scroll")
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    let egui_ctx = ui.ctx().clone();
                                    timeline_list(ui, &result.events, &filter, result.viewer_team, |event| {
                                        egui_ctx.copy_text(format_timeline_event(event));
                                        self.tab_state.toasts.lock().success(t!("ui.replay.timeline_event_copied"));
                                    });
                                });
                        }
                    }
                });
        }

        if retry_requested {
            replay_arc.write().timeline = TimelineState::NotRequested;
        }

        ctx.data_mut(|d| {
            d.insert_temp(filter_id, filter);
            d.insert_temp(egui::Id::new("show_timeline"), open);
        });
    }

    fn handle_context_menu_render(&mut self, ui: &mut egui::Ui, ws_id: WorkspaceId) {
        let replay_weak: Option<Weak<RwLock<Replay>>> = ui
            .ctx()
            .data_mut(|data| data.remove_temp(request_slot_id(ws_id, ReplayRequestSlot::ContextMenuRenderReplay)));
        if let Some(weak) = replay_weak
            && let Some(arc) = weak.upgrade()
            && self.tab_state.wows_data_map.is_some()
        {
            let guard = arc.read();
            let raw_meta = guard.replay_file.raw_meta.clone().into_bytes();
            let pkt_data = guard.replay_file.packet_data.clone();
            let alt_replays: Vec<crate::replay::renderer::AltReplayBytes> = guard
                .alt_replays
                .iter()
                .map(|r| crate::replay::renderer::AltReplayBytes {
                    raw_meta: r.raw_meta.clone().into_bytes(),
                    packet_data: r.packet_data.clone(),
                })
                .collect();
            let map_name = guard.replay_file.meta.mapName.clone();
            let translated_map = guard.map_name(&guard.resource_loader);
            let base = format!("{} - {}", guard.replay_file.meta.playerName, translated_map);
            let replay_name = if let Some(stem) = guard
                .source_path
                .as_ref()
                .and_then(|p: &PathBuf| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            {
                format!("{} - {}", base, stem)
            } else {
                base
            };
            let game_duration = guard.replay_file.meta.duration as f32;
            let replay_version =
                wowsunpack::data::Version::from_client_exe(&guard.replay_file.meta.clientVersionFromExe);
            drop(guard);

            let Some(wows_data) = self.tab_state.wows_data_map.as_ref().and_then(|map| map.resolve(&replay_version))
            else {
                tracing::warn!(
                    "No data for build {}",
                    replay_version.build_number().map_or_else(|| "unknown".to_string(), |b| b.to_string())
                );
                return;
            };
            let asset_cache = self.tab_state.renderer_asset_cache.clone();
            let is_debug_mode = self.tab_state.persisted.read().settings.app.debug_mode;
            let viewer = crate::replay::renderer::launch_replay_renderer(
                raw_meta,
                pkt_data,
                alt_replays,
                map_name,
                replay_name,
                game_duration,
                wows_data,
                asset_cache,
                &self.tab_state.persisted.read().settings.renderer,
                Arc::clone(&self.tab_state.suppress_gpu_encoder_warning),
                self.tab_state.window_settings.clone(),
                self.tab_state.save_notify.clone(),
                is_debug_mode,
            );
            self.tab_state.replay_renderers.lock().push(viewer);
        }
    }

    fn collect_batch_replay_infos(
        &self,
        replay_weaks: &[Weak<RwLock<Replay>>],
    ) -> Vec<crate::replay::renderer::BatchReplayInfo> {
        let mut batch_infos = Vec::new();
        for weak in replay_weaks {
            let Some(arc) = weak.upgrade() else { continue };
            let guard = arc.read();
            let map_name = guard.replay_file.meta.mapName.clone();
            let translated_map = guard.map_name(&guard.resource_loader);
            let base = format!("{} - {}", guard.replay_file.meta.playerName, translated_map);
            let replay_name = if let Some(stem) =
                guard.source_path.as_ref().and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            {
                format!("{} - {}", base, stem)
            } else {
                base
            };
            let game_duration = guard.replay_file.meta.duration as f32;
            let replay_version =
                wowsunpack::data::Version::from_client_exe(&guard.replay_file.meta.clientVersionFromExe);
            let raw_meta = guard.replay_file.raw_meta.clone().into_bytes();
            let pkt_data = guard.replay_file.packet_data.clone();
            drop(guard);

            let Some(wows_data) = self.tab_state.wows_data_map.as_ref().and_then(|map| map.resolve(&replay_version))
            else {
                tracing::warn!(
                    "No data for build {} - skipping replay '{}'",
                    replay_version.build_number().map_or_else(|| "unknown".to_string(), |b| b.to_string()),
                    replay_name
                );
                continue;
            };

            batch_infos.push(crate::replay::renderer::BatchReplayInfo {
                raw_meta,
                packet_data: pkt_data,
                map_name,
                replay_name,
                game_duration,
                wows_data,
            });
        }
        batch_infos
    }

    fn handle_batch_render_request(&mut self, ui: &mut egui::Ui, ws_id: WorkspaceId) {
        // Batch render to folder
        if let Some(replay_weaks) = ui.ctx().data_mut(|data| {
            data.remove_temp::<Vec<Weak<RwLock<Replay>>>>(request_slot_id(ws_id, ReplayRequestSlot::BatchRenderReplays))
        }) {
            let Some(output_dir) =
                rfd::FileDialog::new().set_title("Select output folder for rendered videos").pick_folder()
            else {
                return;
            };

            let batch_infos = self.collect_batch_replay_infos(&replay_weaks);
            if batch_infos.is_empty() {
                self.tab_state.toasts.lock().warning("No renderable replays in selection");
                return;
            }

            let options =
                crate::replay::renderer::render_options_from_saved(&self.tab_state.persisted.read().settings.renderer);
            let renderer_settings = self.tab_state.persisted.read().settings.renderer.clone();
            let status = wows_minimap_renderer::check_encoder();
            let prefer_cpu = crate::replay::renderer::resolve_prefer_cpu(
                renderer_settings.prefer_cpu_encoder,
                renderer_settings.video_codec,
                &status,
            );

            let task = crate::replay::renderer::batch_render_to_folder(
                output_dir,
                batch_infos,
                options,
                self.tab_state.renderer_asset_cache.clone(),
                self.tab_state.toasts.clone(),
                crate::replay::renderer::BatchEncodeOptions {
                    prefer_cpu,
                    codec: renderer_settings.video_codec,
                    include_pre_battle: renderer_settings.include_pre_battle,
                },
            );
            self.tab_state.background_tasks.push(task);
            return;
        }

        // Batch render to clipboard
        if let Some(replay_weaks) = ui.ctx().data_mut(|data| {
            data.remove_temp::<Vec<Weak<RwLock<Replay>>>>(request_slot_id(
                ws_id,
                ReplayRequestSlot::BatchRenderClipboard,
            ))
        }) {
            let batch_infos = self.collect_batch_replay_infos(&replay_weaks);
            if batch_infos.is_empty() {
                self.tab_state.toasts.lock().warning("No renderable replays in selection");
                return;
            }

            let options =
                crate::replay::renderer::render_options_from_saved(&self.tab_state.persisted.read().settings.renderer);
            let renderer_settings = self.tab_state.persisted.read().settings.renderer.clone();
            let status = wows_minimap_renderer::check_encoder();
            let prefer_cpu = crate::replay::renderer::resolve_prefer_cpu(
                renderer_settings.prefer_cpu_encoder,
                renderer_settings.video_codec,
                &status,
            );

            let task = crate::replay::renderer::batch_render_to_clipboard(
                batch_infos,
                options,
                self.tab_state.renderer_asset_cache.clone(),
                self.tab_state.toasts.clone(),
                crate::replay::renderer::BatchEncodeOptions {
                    prefer_cpu,
                    codec: renderer_settings.video_codec,
                    include_pre_battle: renderer_settings.include_pre_battle,
                },
            );
            self.tab_state.background_tasks.push(task);
        }
    }

    /// Called from the main button (non-closure) path.
    fn open_replay_controls_window(&mut self) {
        // Parse from VFS on first use, then cache
        if self.tab_state.replay_controls_cache.is_none()
            && let Some(map) = &self.tab_state.wows_data_map
        {
            let result = map.with_builds(|builds| {
                for data in builds.values() {
                    let data = data.read();
                    let path = "system/data/commands.scheme.xml";
                    let mut buf = Vec::new();
                    if let Ok(mut file) = data.vfs.join(path).and_then(|p| p.open_file()) {
                        use std::io::Read;
                        if file.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
                            let groups = crate::util::controls::parse_commands_scheme(&buf);
                            if !groups.is_empty() {
                                return Some(groups);
                            }
                        }
                    }
                }
                None
            });
            self.tab_state.replay_controls_cache = result;
        }
        self.tab_state.show_replay_controls = true;
    }

    /// Pick up the temp data flag set from context menu closures.
    fn pick_up_replay_controls_request(&mut self, ctx: &egui::Context) {
        // App-wide: opens the single reference window regardless of workspace.
        let request: Option<bool> = ctx.data_mut(|data| data.remove_temp(egui::Id::new("open_replay_controls_window")));
        if request == Some(true) {
            self.open_replay_controls_window();
        }
    }

    /// Draw the standalone replay controls reference window.
    fn show_replay_controls_window(&mut self, ctx: &egui::Context) {
        if !self.tab_state.show_replay_controls {
            return;
        }

        egui::Window::new(t!("ui.replay.controls.window_title"))
            .open(&mut self.tab_state.show_replay_controls)
            .collapsible(true)
            .resizable(true)
            .default_width(360.0)
            .show(ctx, |ui| {
                if let Some(groups) = &self.tab_state.replay_controls_cache {
                    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.7).show(ui, |ui| {
                        for group in groups {
                            ui.add_space(2.0);
                            ui.label(egui::RichText::new(group.title).strong());
                            egui::Grid::new(group.title).num_columns(2).spacing([16.0, 2.0]).striped(true).show(
                                ui,
                                |ui| {
                                    for cmd in &group.commands {
                                        ui.label(&cmd.label);
                                        let binding = if let Some(ref k2) = cmd.key2 {
                                            format!("{}  /  {}", cmd.key1, k2)
                                        } else {
                                            cmd.key1.clone()
                                        };
                                        ui.label(egui::RichText::new(binding).monospace().color(ui.sem().text_dim));
                                        ui.end_row();
                                    }
                                },
                            );
                            ui.separator();
                        }
                    });
                } else {
                    ui.label(t!("ui.replay.controls.unavailable"));
                }
            });
    }
}

struct ReplayTabViewer<'a> {
    tab_state: &'a mut crate::tab_state::TabState,
    workspace: WorkspaceId,
}

impl egui_dock::TabViewer for ReplayTabViewer<'_> {
    type Tab = ReplayTab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        workspace_salt(self.workspace, "replay_tab").with(tab.id)
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        let replay = tab.replay.read();
        let viewer = ToolkitTabViewer { tab_state: self.tab_state };
        if let Some(mp) = viewer.metadata_provider() {
            let ship = replay.vehicle_name(&mp);
            let map = replay.map_name(&mp);
            format!("{ship} - {map}").into()
        } else {
            t!("ui.replay.loading").into_owned().into()
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        let viewer = ToolkitTabViewer { tab_state: self.tab_state };
        let metadata_provider = viewer.metadata_provider().expect("no metadata provider?");
        let replay_weak = Arc::downgrade(&tab.replay);
        let mut replay = tab.replay.write();
        viewer.build_replay_view(&mut replay, &replay_weak, ui, metadata_provider.as_ref(), self.workspace);
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        true
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }

    fn scroll_bars(&self, _tab: &Self::Tab) -> [bool; 2] {
        // The replay view manages its own panels and the player-list table
        // brings its own scroll area. The outer wrapper would duplicate bars.
        [false, false]
    }
}

/// Rebuilds a monospace breakdown block ("Label   : 1,234") from a per-type
/// value lookup, in `descriptions` order, skipping zero entries. `get` reads the
/// value for a key; the column width matches the widest present label + 1,
/// reproducing the original inline hover formatting exactly.
fn breakdown_hover_string<F: Fn(&str) -> u64>(descriptions: &[(&str, &str)], locale: &str, get: F) -> String {
    let longest_width =
        descriptions.iter().filter(|(key, _)| get(key) > 0).map(|(_, desc)| desc.len()).max().unwrap_or_default() + 1;
    descriptions
        .iter()
        .filter_map(|(key, description)| {
            let num = get(key);
            if num > 0 {
                let num_str = separate_number(num, Some(locale));
                Some(format!("{description:<longest_width$}: {num_str}"))
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Sample-count headline for the effective-fire-chance block: fires and
/// eligible hits summed over every target ship, and how many ships those
/// totals cover.
///
/// Counts, not a rate. Fire resistance is a property of the victim, so a rate
/// only means something inside one target ship's row, where the victim's
/// `burnProb` coefficient and node probabilities are fixed. Reducing several
/// ships to one percentage would need a weighting between a ship hit twice and
/// one hit eighty times, and no weighting is the right one. Zero eligible hits
/// says so in words rather than showing a total nothing stands behind.
fn fire_chance_headline_text(fire_chance: &EffectiveFireChance) -> String {
    if fire_chance.eligible_hits == 0 {
        return t!("ui.replay.sections.fire_chance_no_eligible_hits").into_owned();
    }
    format!(
        "{}   {}",
        fire_chance_counts_text(fire_chance.fires, fire_chance.eligible_hits),
        fire_chance_ships_text(fire_chance),
    )
}

/// "across N target ships", with its own singular form. The translation layer
/// carries no plural machinery, so a count of one takes a separate key the way
/// the session-stats labels do.
fn fire_chance_ships_text(fire_chance: &EffectiveFireChance) -> Cow<'static, str> {
    let ships = fire_chance.ships_with_trials();
    if ships == 1 {
        return t!("ui.replay.sections.fire_chance_ships_one");
    }
    t!("ui.replay.sections.fire_chance_ships", ships = ships)
}

/// The headline plus the optional "expected" line beneath it, shared verbatim
/// for the copy-to-clipboard and hover text. The on-screen block lays the same
/// figures out with egui widgets instead, so this is the plain-text form.
///
/// The expected line renders `expected_fires`, a count of fires, which is what
/// the observed fire count above it is comparable against.
fn fire_chance_headline_lines(fire_chance: &EffectiveFireChance) -> Vec<String> {
    let mut lines = vec![fire_chance_headline_text(fire_chance)];
    if fire_chance.eligible_hits > 0
        && let Some(expected) = fire_chance.expected_fires
    {
        let text = t!("ui.replay.sections.fire_chance_expected_fires", fires = format!("{expected:.1}"));
        lines.push(format!("  {text}"));
    }
    lines
}

/// `per_ship`, sorted by eligible hits descending, for both the expander and
/// the copy-to-clipboard breakdown.
fn sorted_per_ship(fire_chance: &EffectiveFireChance) -> Vec<&PerShipFireChance> {
    let mut ships: Vec<&PerShipFireChance> = fire_chance.per_ship.iter().collect();
    ships.sort_by(|a, b| b.eligible_hits.cmp(&a.eligible_hits));
    ships
}

/// "N fires / M eligible hits", the counts a rate stands on. Both figures carry
/// their unit, because a bare pair of numbers says nothing about which is which;
/// the on-screen form puts a flame in front of the first as well.
///
/// The denominator says "eligible hits" rather than "hits" so it reads as the
/// same figure the breakdown's `eligible` row states, which is what it is. A
/// bare "hits" there invites the reader to compare it against the hits on the
/// ship, which is a different and larger number.
fn fire_chance_counts_text(fires: u32, hits: u32) -> String {
    format!("{} / {}", fire_chance_fires_text(fires), fire_chance_eligible_hits_text(hits))
}

/// "N fires", with its own singular form. The translation layer carries no
/// plural machinery, so a count of one takes a separate key throughout, the way
/// the target-ship count does.
fn fire_chance_fires_text(fires: u32) -> Cow<'static, str> {
    if fires == 1 {
        return t!("ui.replay.sections.fire_chance_fires_one");
    }
    t!("ui.replay.sections.fire_chance_fires", fires = fires)
}

/// "N eligible hits", with its own singular form.
fn fire_chance_eligible_hits_text(hits: u32) -> Cow<'static, str> {
    if hits == 1 {
        return t!("ui.replay.sections.fire_chance_eligible_hits_one");
    }
    t!("ui.replay.sections.fire_chance_eligible_hits", hits = hits)
}

/// The unit a count in the breakdown's left column carries, singular at one.
/// The count itself is printed apart from the label so a column of them lines
/// up, which is why these keys are bare nouns.
fn fire_chance_count_label(count: u32, plural: &'static str, singular: &'static str) -> Cow<'static, str> {
    t!(if count == 1 { singular } else { plural })
}

/// One target-ship row in plain text, the header of that ship's breakdown. The
/// on-screen expander lays the same figures out as a grid. This is the only
/// place a percentage is stated, because the victim's fire resistance is fixed
/// within the row. `PerShipFireChance::rate` is `None` over zero eligible hits,
/// which is unknown rather than a zero rate, and the expected column is the
/// matching per-hit chance so the two are comparable.
fn fire_chance_per_ship_line(ship: &PerShipFireChance, localize_ship: &dyn Fn(&PerShipFireChance) -> String) -> String {
    let rate_text = match ship.rate() {
        Some(rate) => format!("{:.1}%  {}", rate * 100.0, fire_chance_counts_text(ship.fires, ship.eligible_hits)),
        None => t!("ui.replay.sections.fire_chance_no_eligible_hits").into_owned(),
    };
    match ship.expected_rate() {
        Some(expected) => format!(
            "{}   {rate_text}   {} {:.1}%",
            localize_ship(ship),
            t!("ui.replay.sections.fire_chance_expected"),
            expected * 100.0
        ),
        None => format!("{}   {rate_text}", localize_ship(ship)),
    }
}

/// The attacker-side formula breakdown, in order: a base line for the shell's
/// raw `burnProb`, then each modifier step that moved the value, then the
/// resulting total. `localize_source` resolves a step's raw source identifier
/// to a display name (an equipped upgrade, signal or crew skill); passed in
/// rather than called directly so this function stays free of the metadata
/// provider and is testable with a stub. Empty when no shell resolved to
/// compute a formula from at all.
fn fire_chance_formula_lines(
    fire_chance: &EffectiveFireChance,
    localize_source: &dyn Fn(&str) -> String,
) -> Vec<String> {
    let (Some(base), Some((raw, clamped))) = (fire_chance.formula_base, fire_chance.formula_total()) else {
        return Vec::new();
    };
    let formula = &fire_chance.formula;

    let names: Vec<String> = formula
        .iter()
        .map(|step| match &step.source {
            Some(source) => format!("{} ({})", step.modifier, localize_source(source)),
            None => step.modifier.clone(),
        })
        .collect();
    let base_label = t!("ui.replay.sections.fire_chance_formula_base").into_owned();
    // 2-char slot for the step lines' "x "/"+ " prefix, so the base line's
    // label starts in the same column as a step's name even though it carries
    // no operator symbol of its own.
    let prefix_width = 2;
    let name_width = names
        .iter()
        .map(|name| name.chars().count())
        .chain(std::iter::once(base_label.chars().count()))
        .max()
        .unwrap_or(0);

    let mut lines = vec![t!("ui.replay.sections.fire_chance_formula").into_owned()];
    lines.push(format!("  {:prefix_width$}{base_label:<name_width$} {:.1}%", "", base * 100.0));
    for (step, name) in formula.iter().zip(&names) {
        let (symbol, value_text) = match step.op {
            FormulaOp::Multiply => ("x", format!("{:.2}", step.value)),
            FormulaOp::Add => ("+", format!("+{:.1}pp", step.value * 100.0)),
        };
        lines.push(format!("  {symbol} {name:<name_width$} {value_text}"));
    }

    // `formula_total` reports the raw product and the value the eligibility
    // model rolls with. Showing only the raw one would silently disagree with
    // the rate above wherever the clamp bites.
    if (raw - clamped).abs() > f32::EPSILON {
        lines.push(format!(
            "  = {:.1}%   ({} {:.1}%)",
            raw * 100.0,
            t!("ui.replay.sections.fire_chance_formula_clamped"),
            clamped * 100.0
        ));
    } else {
        lines.push(format!("  = {:.1}%", raw * 100.0));
    }
    lines
}

/// One population's counts as the breakdown reads them, taken from either the
/// whole battle or one target ship's row. Both carry the same figures, so the
/// two levels render through one function.
///
/// HE-only and ship-only throughout. Our AP and SAP hits, our secondaries and
/// the shells that struck terrain are none of them members of the population
/// this chain describes, so they appear nowhere in it; the analysis still counts
/// them, and its corpus checks still reconcile against them, but a listing whose
/// first line is a count of HE shells cannot state how many shells could not
/// burn without contradicting itself.
struct FireChanceTally<'a> {
    he_hits_on_a_ship: u32,
    eligible_hits: u32,
    exclusions: &'a BTreeMap<ExclusionReason, u32>,
    not_applicable: u32,
}

impl<'a> From<&'a EffectiveFireChance> for FireChanceTally<'a> {
    fn from(fire_chance: &'a EffectiveFireChance) -> FireChanceTally<'a> {
        FireChanceTally {
            he_hits_on_a_ship: fire_chance.he_hits_on_a_ship,
            eligible_hits: fire_chance.eligible_hits,
            exclusions: &fire_chance.exclusions,
            not_applicable: fire_chance.not_applicable,
        }
    }
}

impl<'a> From<&'a PerShipFireChance> for FireChanceTally<'a> {
    fn from(ship: &'a PerShipFireChance) -> FireChanceTally<'a> {
        FireChanceTally {
            he_hits_on_a_ship: ship.he_hits_on_a_ship,
            eligible_hits: ship.eligible_hits,
            exclusions: &ship.exclusions,
            not_applicable: ship.not_applicable,
        }
    }
}

/// One row of a count-and-label listing, at the depth it sits under the row
/// above it. The on-screen breakdown lays these out as a grid and the
/// clipboard form renders them as text, so both read the same rows and cannot
/// drift apart.
#[derive(Clone, Debug, PartialEq)]
struct TallyRow {
    depth: usize,
    count: u32,
    label: Cow<'static, str>,
}

impl TallyRow {
    fn new(depth: usize, count: u32, label: Cow<'static, str>) -> TallyRow {
        TallyRow { depth, count, label }
    }
}

/// Rows as plain text, counts right-aligned within their own depth so a nested
/// listing lines up independently of the wider figures above it.
///
/// `indent` is the leading whitespace the whole block sits under, so the
/// aggregate and the per-ship blocks share this function and differ only in
/// depth.
fn fire_chance_rows_to_lines(rows: &[TallyRow], indent: &str) -> Vec<String> {
    let mut widths: BTreeMap<usize, usize> = BTreeMap::new();
    for row in rows {
        let digits = row.count.to_string().len();
        let width = widths.entry(row.depth).or_insert(digits);
        *width = (*width).max(digits);
    }
    rows.iter()
        .map(|row| {
            let width = widths.get(&row.depth).copied().unwrap_or(1);
            let nesting = "  ".repeat(row.depth);
            format!("{indent}{nesting}{:>width$} {}", row.count, row.label)
        })
        .collect()
}

/// One population of our shells: how many HE hits landed on a ship, then the
/// eligibility model's own answer over them.
///
/// Eligible is pinned first, then the refusals by count descending with
/// zero-count reasons omitted, then the hits that were never the model's
/// question because the ship was already dead, apart from the refusals because
/// they are not one. The rows under the HE line sum to it exactly.
///
/// `heads` are the rows that sit above it at the same depth, which is where the
/// whole-battle block states its fired count. A per-ship block passes none: a
/// salvo is fired at the water rather than at a victim.
fn fire_chance_tally_rows(tally: &FireChanceTally<'_>, heads: &[TallyRow]) -> Vec<TallyRow> {
    let mut rows: Vec<TallyRow> = heads.to_vec();
    rows.push(TallyRow::new(
        0,
        tally.he_hits_on_a_ship,
        fire_chance_count_label(
            tally.he_hits_on_a_ship,
            "ui.replay.sections.fire_chance_he_hits",
            "ui.replay.sections.fire_chance_he_hits_one",
        ),
    ));
    rows.push(TallyRow::new(1, tally.eligible_hits, t!("ui.replay.sections.fire_chance_eligible")));

    let mut excluded: Vec<(&ExclusionReason, &u32)> =
        tally.exclusions.iter().filter(|(_, count)| **count > 0).collect();
    excluded.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    rows.extend(
        excluded.into_iter().map(|(reason, count)| TallyRow::new(1, *count, t!(keys::exclusion_reason_key(*reason)))),
    );
    if tally.not_applicable > 0 {
        rows.push(TallyRow::new(1, tally.not_applicable, t!("ui.replay.sections.fire_chance_not_applicable")));
    }
    rows
}

/// A count-and-label listing on screen: counts right-aligned in their own
/// column, labels indented by depth. A grid rather than padded text, so the
/// columns line up in the proportional font the rest of the row uses.
fn fire_chance_rows_ui(ui: &mut egui::Ui, id: &str, rows: &[TallyRow]) {
    egui::Grid::new(ui.id().with(id)).num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
        for row in rows {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(row.count.to_string());
            });
            ui.horizontal(|ui| {
                ui.add_space(row.depth as f32 * 12.0);
                if row.depth == 0 {
                    ui.label(row.label.as_ref());
                } else {
                    ui.weak(row.label.as_ref());
                }
            });
            ui.end_row();
        }
    });
}

/// The whole battle's tally: HE shells fired, then the HE hits on a ship they
/// produced, then how those split.
fn fire_chance_battle_tally_rows(fire_chance: &EffectiveFireChance) -> Vec<TallyRow> {
    let fired = TallyRow::new(
        0,
        fire_chance.he_shells_fired,
        fire_chance_count_label(
            fire_chance.he_shells_fired,
            "ui.replay.sections.fire_chance_shells_fired",
            "ui.replay.sections.fire_chance_shells_fired_one",
        ),
    );
    fire_chance_tally_rows(&fire_chance.into(), std::slice::from_ref(&fired))
}

/// The `SetFire` ribbon accounting: every fire the game credited us with, split
/// into the ones a shell of ours could be named for and the ones that could not,
/// with a reason under each of the latter.
///
/// This is the block that answers "the game gave me six fires and this says
/// four". The two figures under the ribbon count sum to it by construction, so
/// the arithmetic is visible rather than asserted.
///
/// Whole-battle only. A `SetFire` ribbon names no victim, so an uncredited one
/// belongs to no target ship without inventing the assignment, and the
/// per-target-ship rows would have to state a total they cannot account for.
fn fire_chance_ribbon_rows(fire_chance: &EffectiveFireChance) -> Vec<TallyRow> {
    if fire_chance.set_fire_ribbons == 0 {
        return Vec::new();
    }
    let mut rows = vec![
        TallyRow::new(
            0,
            fire_chance.set_fire_ribbons,
            fire_chance_count_label(
                fire_chance.set_fire_ribbons,
                "ui.replay.sections.fire_chance_ribbons",
                "ui.replay.sections.fire_chance_ribbons_one",
            ),
        ),
        TallyRow::new(1, fire_chance.fires, t!("ui.replay.sections.fire_chance_ribbons_credited")),
    ];
    if fire_chance.unattributed_fires == 0 {
        return rows;
    }
    rows.push(TallyRow::new(
        1,
        fire_chance.unattributed_fires,
        t!("ui.replay.sections.fire_chance_ribbons_uncredited"),
    ));
    let mut reasons: Vec<(&UnattributedFireReason, &u32)> =
        fire_chance.unattributed_reasons.iter().filter(|(_, count)| **count > 0).collect();
    reasons.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    rows.extend(
        reasons
            .into_iter()
            .map(|(reason, count)| TallyRow::new(2, *count, t!(keys::unattributed_fire_reason_key(*reason)))),
    );
    rows
}

/// Shells fired, the hits they produced, how those split, and then what became
/// of the fire ribbons themselves. Top down, so the reader sees the whole
/// population before its parts.
fn fire_chance_breakdown_lines(fire_chance: &EffectiveFireChance) -> Vec<String> {
    let mut lines = fire_chance_rows_to_lines(&fire_chance_battle_tally_rows(fire_chance), "");
    let ribbons = fire_chance_ribbon_rows(fire_chance);
    if !ribbons.is_empty() {
        lines.push(String::new());
        lines.extend(fire_chance_rows_to_lines(&ribbons, ""));
    }
    lines
}

/// The same accounting per target ship, each row's rate over its own breakdown,
/// closed by the hits no target ship's row could carry.
///
/// There is no per-ship shells-fired line: a salvo is fired at the water rather
/// than at a victim, so the analysis states that count only once. Nor is there
/// a per-ship ribbon block: a `SetFire` ribbon names no victim.
fn fire_chance_per_ship_lines(
    fire_chance: &EffectiveFireChance,
    localize_ship: &dyn Fn(&PerShipFireChance) -> String,
) -> Vec<String> {
    if fire_chance.per_ship.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![t!("ui.replay.sections.fire_chance_per_ship").into_owned()];
    for ship in sorted_per_ship(fire_chance) {
        lines.push(format!("  {}", fire_chance_per_ship_line(ship, localize_ship)));
        lines.extend(fire_chance_rows_to_lines(&fire_chance_tally_rows(&ship.into(), &[]), "    "));
    }
    // Without this the rows silently fail to add up to the aggregate: a hit keyed
    // to the recording player's own ship, or to a player whose hull never
    // resolved, has no row to sit in.
    if let Some(line) = fire_chance_no_target_ship_line(fire_chance) {
        lines.push(format!("  {line}"));
    }
    lines
}

/// "N HE hits not attributable to a target ship", or `None` when every HE hit
/// on a ship landed on one the breakdown has a row for.
///
/// Derived here rather than read off the analysis, because the analysis reports
/// the remainder over every hit of ours and this listing is HE-only: a hit keyed
/// to our own ship or to a player whose hull never resolved is counted in the
/// aggregate HE line and carried by no row, and this is exactly that difference.
fn fire_chance_no_target_ship_line(fire_chance: &EffectiveFireChance) -> Option<String> {
    let in_rows: u32 = fire_chance.per_ship.iter().map(|ship| ship.he_hits_on_a_ship).sum();
    let hits = fire_chance.he_hits_on_a_ship.saturating_sub(in_rows);
    if hits == 0 {
        return None;
    }
    let label = fire_chance_count_label(
        hits,
        "ui.replay.sections.fire_chance_no_target_ship",
        "ui.replay.sections.fire_chance_no_target_ship_one",
    );
    Some(format!("{hits} {label}"))
}

/// Presentation view of a normalized per-victim interaction: the numeric fields
/// are copied verbatim and the text/hover fields are formatted from them, gated
/// on `> 0` exactly as the original inline extraction did.
fn damage_interaction_from_normalized(
    interaction: &wows_replay_insights::battle_report::DamageInteraction,
    locale: &str,
) -> DamageInteraction {
    let mut out = DamageInteraction { damage_dealt: interaction.damage_dealt, ..Default::default() };
    if interaction.damage_dealt > 0 {
        out.damage_dealt_text = separate_number(interaction.damage_dealt, Some(locale));
        out.damage_dealt_hover_text = breakdown_hover_string(&DAMAGE_DESCRIPTIONS, locale, |key| {
            interaction.damage_dealt_by_type_full.get(key).copied().unwrap_or(0)
        });
    }
    out.damage_dealt_percentage = interaction.damage_dealt_percentage;
    if interaction.damage_dealt_percentage > 0.0 {
        out.damage_dealt_percentage_text = format!("{:.0}%", interaction.damage_dealt_percentage);
    }
    out.damage_dealt_inverse_percentage = interaction.damage_dealt_inverse_percentage;
    if interaction.damage_dealt_inverse_percentage > 0.0 {
        out.damage_dealt_inverse_percentage_text = format!("{:.0}%", interaction.damage_dealt_inverse_percentage);
    }
    out.damage_received = interaction.damage_received;
    if interaction.damage_received > 0 {
        out.damage_received_text = separate_number(interaction.damage_received, Some(locale));
        out.damage_received_hover_text = breakdown_hover_string(&DAMAGE_DESCRIPTIONS, locale, |key| {
            interaction.damage_received_by_type_full.get(key).copied().unwrap_or(0)
        });
    }
    out.damage_received_percentage = interaction.damage_received_percentage;
    if interaction.damage_received_percentage > 0.0 {
        out.damage_received_percentage_text = format!("{:.0}%", interaction.damage_received_percentage);
    }
    out.damage_received_inverse_percentage = interaction.damage_received_inverse_percentage;
    if interaction.damage_received_inverse_percentage > 0.0 {
        out.damage_received_inverse_percentage_text = format!("{:.0}%", interaction.damage_received_inverse_percentage);
    }
    out
}

/// Groups `DamageStatEntry` items by weapon for a given category, returning hover text.
fn build_damage_stat_hover_text(
    stats: &[wows_replays::analyzer::decoder::DamageStatEntry],
    category: DamageStatCategory,
    locale: &str,
) -> Option<RichText> {
    let mut groups: Vec<(&str, f64)> = Vec::new();
    for entry in stats {
        if entry.category == Recognized::Known(category) && entry.total > 0.0 {
            let label = weapon_group_label(&entry.weapon);
            if let Some(existing) = groups.iter_mut().find(|(l, _)| *l == label) {
                existing.1 += entry.total;
            } else {
                groups.push((label, entry.total));
            }
        }
    }
    if groups.is_empty() {
        return None;
    }
    groups.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let longest = groups.iter().map(|(l, _)| l.len()).max().unwrap_or(0) + 1;
    let lines: Vec<String> = groups
        .iter()
        .map(|(label, dmg)| {
            let num_str = separate_number(*dmg as u64, Some(locale));
            format!("{label:<longest$}: {num_str}")
        })
        .collect();
    Some(RichText::new(lines.join("\n")).font(FontId::monospace(12.0)))
}

/// Renders chat messages into a `Ui`. Used by both the inline chat view and the chat window.
///
/// Click-to-copy is signaled via a temp data slot `"chat_message_copied"` containing the
/// plaintext string. The caller is responsible for reading this and performing the copy/toast.
fn build_replay_chat_content(
    metadata_provider: Option<&GameMetadataProvider>,
    messages: &[GameMessage],
    ui: &mut egui::Ui,
) {
    for message in messages {
        let GameMessage { sender_relation, sender_name, channel, message, entity_id: _, player, clock: _ } = message;

        let (translated_name, translated_text) =
            if sender_relation.is_none() || player.as_ref().map(|player| player.is_bot()).unwrap_or_default() {
                let translated_user = metadata_provider.and_then(|provider| {
                    provider.localized_name_from_id(&TranslationKey::new(sender_name.as_str())).map(Cow::Owned)
                });
                let translated_text = metadata_provider.and_then(|provider| {
                    provider.localized_name_from_id(&TranslationKey::new(message.as_str())).map(Cow::Owned)
                });
                (translated_user, translated_text)
            } else {
                (None, None)
            };

        let message =
            if let Ok(decoded) = decode_html(message.as_str()) { Cow::Owned(decoded) } else { Cow::Borrowed(message) };

        let sender_name: Cow<'_, str> = translated_name.unwrap_or(Cow::Borrowed(sender_name.as_str()));
        let message: Cow<'_, str> = match translated_text {
            Some(t) => t,
            None => match message {
                Cow::Owned(s) => Cow::Owned(s),
                Cow::Borrowed(s) => Cow::Borrowed(s.as_str()),
            },
        };

        let text = match player {
            Some(player) if !player.initial_state().clan().is_empty() => {
                format!("[{}] {sender_name} ({channel:?}): {message}", player.initial_state().clan())
            }
            _ => {
                format!("{sender_name} ({channel:?}): {message}")
            }
        };

        let name_color = if let Some(relation) = sender_relation {
            player_color_for_team_relation(*relation, ui.visuals())
        } else {
            ui.sem().text_dim
        };

        let mut job = LayoutJob::default();
        if let Some(player) = player
            && !player.initial_state().clan().is_empty()
        {
            job.append(
                &format!("[{}] ", player.initial_state().clan()),
                0.0,
                TextFormat { color: clan_color_for_player(player).unwrap().color(ui.visuals()), ..Default::default() },
            );
        }
        job.append(&format!("{sender_name}:\n"), 0.0, TextFormat { color: name_color, ..Default::default() });

        let text_color = match channel {
            ChatChannel::Division => ui.sem().chat.division,
            ChatChannel::Global => ui.sem().chat.global,
            ChatChannel::Team => ui.sem().chat.team,
            _ => ui.sem().chat.other,
        };

        job.append(&message, 0.0, TextFormat { color: text_color, ..Default::default() });

        let label_response = ui.add(Label::new(job));
        // Full-width hover row so the copy button appears when hovering anywhere on the row
        let row_rect = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), label_response.rect.y_range());
        let row_hovered = ui.rect_contains_pointer(row_rect);
        if row_hovered {
            // Place button using a child ui so it doesn't affect parent layout
            let padded_row = row_rect.shrink2(egui::vec2(8.0, 0.0));
            let btn_rect = egui::Align2::RIGHT_CENTER
                .align_size_within_rect(egui::vec2(20.0, label_response.rect.height()), padded_row);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(btn_rect));
            if child.small_button(crate::icons::COPY).on_hover_text(t!("ui.replay.copy_message")).clicked() {
                ui.ctx().copy_text(text);
            }
        }
        ui.add(Separator::default());
        ui.end_row();
    }
}

#[cfg(test)]
mod fire_chance_render_tests {
    use super::*;
    use wows_replay_insights::fire_chance::analysis::FormulaStep;
    // Only the tests name this: the analysis still counts our AP hits and our
    // splashes, and these check that none of it reaches the breakdown.
    use wows_replay_insights::fire_chance::analysis::NarrowingReason;

    fn fixture(eligible_hits: u32, fires: u32, expected_fires: Option<f32>) -> EffectiveFireChance {
        EffectiveFireChance {
            he_shells_fired: 0,
            hits: eligible_hits,
            narrowed: BTreeMap::new(),
            he_hits_on_a_ship: eligible_hits,
            hits_without_a_target_ship: 0,
            not_applicable: 0,
            eligible_hits,
            fires,
            expected_fires,
            per_ship: Vec::new(),
            exclusions: BTreeMap::new(),
            section_predictions: Vec::new(),
            set_fire_ribbons: fires,
            unattributed_fires: 0,
            unattributed_reasons: BTreeMap::new(),
            formula_base: None,
            formula: Vec::new(),
        }
    }

    /// A result carrying only a formula, for the breakdown lines. One eligible
    /// hit so the struct is a shape `analyze` could actually produce.
    fn formula_fixture(base: Option<f32>, formula: Vec<FormulaStep>) -> EffectiveFireChance {
        EffectiveFireChance { formula_base: base, formula, ..fixture(1, 0, None) }
    }

    fn formula_step(modifier: &str, source: Option<&str>, op: FormulaOp, value: f32, result: f32) -> FormulaStep {
        FormulaStep { modifier: modifier.to_owned(), source: source.map(str::to_owned), op, value, result }
    }

    /// Identity localizer: returns the source string unchanged, for tests that
    /// don't care about localization.
    fn no_localization(source: &str) -> String {
        source.to_owned()
    }

    fn ship(name: &str, eligible_hits: u32, fires: u32, expected_fires: Option<f32>) -> PerShipFireChance {
        PerShipFireChance {
            victim_ship_index: format!("{name}_INDEX"),
            victim_ship_name: name.to_owned(),
            hits: eligible_hits,
            he_hits_on_a_ship: eligible_hits,
            narrowed: BTreeMap::new(),
            eligible_hits,
            exclusions: BTreeMap::new(),
            not_applicable: 0,
            fires,
            expected_fires,
        }
    }

    /// The headline is counts and the ship count they cover. A percentage
    /// there would need a weighting across victims of different fire
    /// resistance, which is why the rate lives on the per-ship rows instead.
    #[test]
    fn headline_shows_counts_and_the_ships_they_cover() {
        let mut fc = fixture(63, 9, Some(6.3));
        fc.per_ship = vec![ship("Zao", 40, 6, None), ship("Iowa", 23, 3, None)];
        let headline = fire_chance_headline_text(&fc);
        assert!(!headline.contains('%'), "expected no percentage in {headline:?}");
        assert_eq!(headline, "9 fires / 63 hits that could have started one   across 2 target ships");
    }

    /// `expected_fires` is a count of fires, and the observed figure beside it
    /// is now a count too, so the line renders the count as it stands.
    #[test]
    fn headline_expected_line_renders_the_fire_count() {
        let mut fc = fixture(63, 9, Some(6.3));
        fc.per_ship = vec![ship("Zao", 63, 9, None)];
        assert_eq!(
            fire_chance_headline_lines(&fc),
            vec![
                "9 fires / 63 hits that could have started one   across 1 target ship".to_owned(),
                "  expected 6.3 fires".to_owned()
            ]
        );
    }

    /// With no eligible hits `expected_fires` is legitimately `Some(0.0)`: a
    /// sum over nothing. Nothing stands behind it, so no expected line is
    /// shown, exactly as no observed total is.
    #[test]
    fn headline_over_zero_eligible_hits_shows_no_expected_line() {
        let fc = fixture(0, 0, Some(0.0));
        assert_eq!(fire_chance_headline_lines(&fc), vec!["no hits that could have started a fire".to_owned()]);
    }

    /// Zero eligible hits is unknown, not zero: this must never render as a
    /// total or a rate.
    #[test]
    fn headline_over_zero_eligible_hits_shows_no_totals() {
        let fc = fixture(0, 0, None);
        let headline = fire_chance_headline_text(&fc);
        assert!(!headline.contains('%'), "expected no percentage in {headline:?}");
        assert_eq!(headline, "no hits that could have started a fire");
    }

    /// The whole shape in one place: HE shells fired, the HE hits on a ship
    /// they produced, and then the eligibility split. The hits that could have
    /// started a fire are pinned first, the refusals follow by count descending
    /// with zero-count reasons dropped, and the hits that were never in the
    /// population come last.
    #[test]
    fn the_breakdown_reads_fired_then_he_hits_then_the_split() {
        let mut fc = fixture(43, 4, None);
        fc.he_shells_fired = 171;
        fc.hits = 130;
        fc.he_hits_on_a_ship = 69;
        fc.narrowed.insert(NarrowingReason::ShellCannotBurn, 39);
        fc.narrowed.insert(NarrowingReason::ImpactNotOnAShip, 22);
        fc.not_applicable = 1;
        fc.exclusions.insert(ExclusionReason::SectionAlreadyBurning, 15);
        fc.exclusions.insert(ExclusionReason::DamageControlActive, 8);
        fc.exclusions.insert(ExclusionReason::ImpactUnplaceableOnVictim, 2);

        assert_eq!(
            fire_chance_breakdown_lines(&fc)[..7],
            [
                "171 HE shells fired".to_owned(),
                " 69 HE hits on a ship".to_owned(),
                "  43 could have started a fire".to_owned(),
                "  15 section already burning".to_owned(),
                "   8 Damage Control Party active".to_owned(),
                "   2 could not place the impact on the ship we matched it to".to_owned(),
                "   1 victim already dead, not applicable".to_owned(),
            ]
        );
    }

    /// The chain is HE-only and ship-only from top to bottom. An AP hit was
    /// never an HE shell and a splash in the water hit no ship, so neither has
    /// any business in a listing whose first line counts HE shells fired and
    /// whose last lines read as "these could have burned and did not".
    #[test]
    fn the_shell_and_terrain_filters_never_appear_in_the_breakdown() {
        let mut fc = fixture(43, 4, None);
        fc.hits = 130;
        fc.he_hits_on_a_ship = 69;
        fc.narrowed.insert(NarrowingReason::ShellCannotBurn, 39);
        fc.narrowed.insert(NarrowingReason::ImpactNotOnAShip, 22);
        fc.exclusions.insert(ExclusionReason::SectionAlreadyBurning, 26);

        let lines = fire_chance_breakdown_lines(&fc);
        assert!(!lines.iter().any(|line| line.contains("could not burn")), "got {lines:?}");
        assert!(!lines.iter().any(|line| line.contains("terrain")), "got {lines:?}");
        assert!(!lines.iter().any(|line| line.contains("130")), "got {lines:?}");
    }

    /// A victim that was never hit after it died contributes no row, rather
    /// than a zero one that reads as a category with nothing in it.
    #[test]
    fn the_breakdown_omits_the_not_applicable_row_when_it_is_empty() {
        let mut fc = fixture(2, 0, None);
        fc.he_shells_fired = 6;
        fc.hits = 2;
        let lines = fire_chance_breakdown_lines(&fc);
        assert!(!lines.iter().any(|line| line.contains("not applicable")), "got {lines:?}");
    }

    /// The head of the chain is the fired count and the HE hits on a ship, in
    /// that order and with nothing between them.
    #[test]
    fn the_breakdown_head_is_fired_then_he_hits_on_a_ship() {
        let mut fc = fixture(2, 0, None);
        fc.he_shells_fired = 6;
        fc.hits = 2;
        let lines = fire_chance_breakdown_lines(&fc);
        assert_eq!(lines[..2], ["6 HE shells fired".to_owned(), "2 HE hits on a ship".to_owned()]);
    }

    /// Each ship's own accounting sits under its row in the same shape, so the
    /// hover answers "why did so few hits on this ship count" without a second
    /// lookup. The row's denominator and the `eligible` line under it are the
    /// same figure, said the same way.
    #[test]
    fn the_per_ship_block_carries_each_ships_own_breakdown() {
        let mut fc = fixture(12, 2, None);
        let mut zao = ship("Zao", 12, 2, None);
        zao.hits = 20;
        zao.he_hits_on_a_ship = 17;
        zao.narrowed.insert(NarrowingReason::ShellCannotBurn, 3);
        zao.not_applicable = 3;
        zao.exclusions.insert(ExclusionReason::ObservationGap, 2);
        fc.per_ship = vec![zao];

        assert_eq!(
            fire_chance_per_ship_lines(&fc, &|s: &PerShipFireChance| s.victim_ship_name.clone()),
            vec![
                "Per Target Ship".to_owned(),
                "  Zao   16.7%  2 fires / 12 hits that could have started one".to_owned(),
                "    17 HE hits on a ship".to_owned(),
                "      12 could have started a fire".to_owned(),
                "       2 observation gap".to_owned(),
                "       3 victim already dead, not applicable".to_owned(),
            ]
        );
    }

    /// A shell that hit the water hit no ship, so it belongs to the whole
    /// battle's narrowing step and to no target ship's rows.
    #[test]
    fn no_per_ship_row_carries_a_terrain_count() {
        let mut fc = fixture(12, 2, None);
        fc.narrowed.insert(NarrowingReason::ImpactNotOnAShip, 22);
        fc.per_ship = vec![ship("Zao", 12, 2, None)];
        let lines = fire_chance_per_ship_lines(&fc, &|s: &PerShipFireChance| s.victim_ship_name.clone());
        assert!(!lines.iter().any(|line| line.contains("terrain")), "got {lines:?}");
    }

    /// The per-ship rows have to add back up to the aggregate, so the hits no
    /// row could carry are stated rather than dropped.
    #[test]
    fn the_per_ship_block_states_the_hits_no_row_carries() {
        let mut fc = fixture(12, 2, None);
        fc.he_hits_on_a_ship = 45;
        fc.per_ship = vec![ship("Zao", 12, 2, None)];
        let lines = fire_chance_per_ship_lines(&fc, &|s: &PerShipFireChance| s.victim_ship_name.clone());
        assert_eq!(lines.last().map(String::as_str), Some("  33 HE hits not attributable to a target ship"));
    }

    /// Same count-versus-rate rule as the headline: 12 hits expecting 1.656
    /// fires is a 13.8% per-hit rate. Both counts carry their unit, because a
    /// bare pair of numbers does not say which of them is which.
    #[test]
    fn per_ship_line_includes_expected_when_present() {
        let s = ship("Zao", 12, 2, Some(1.656));
        assert_eq!(
            fire_chance_per_ship_line(&s, &|s: &PerShipFireChance| s.victim_ship_name.clone()),
            "Zao   16.7%  2 fires / 12 hits that could have started one   expected 13.8%"
        );
    }

    #[test]
    fn per_ship_line_omits_expected_when_absent() {
        let s = ship("Iowa", 11, 1, None);
        assert_eq!(
            fire_chance_per_ship_line(&s, &|s: &PerShipFireChance| s.victim_ship_name.clone()),
            "Iowa   9.1%  1 fire / 11 hits that could have started one"
        );
    }

    /// One fire is one fire, not "1 fires", and one hit is one hit.
    #[test]
    fn counts_of_one_are_singular() {
        assert_eq!(fire_chance_counts_text(1, 1), "1 fire / 1 hit that could have started one");
        assert_eq!(fire_chance_counts_text(0, 2), "0 fires / 2 hits that could have started one");
    }

    /// The same shape over a real match's counts, taken from
    /// `20260309_140531_PGSC720-Bremen_s02_Naval_Defense.wowsreplay` as the
    /// corpus harness reports them. A fixture can be made to render anything;
    /// this is what the block actually says about a replay with 1276 hits in it.
    #[test]
    fn the_breakdown_renders_a_real_replays_counts() {
        let mut fc = fixture(202, 11, None);
        fc.he_shells_fired = 2312;
        fc.hits = 1276;
        fc.he_hits_on_a_ship = 1132;
        fc.narrowed.insert(NarrowingReason::NotMainBattery, 97);
        fc.narrowed.insert(NarrowingReason::ImpactNotOnAShip, 47);
        fc.not_applicable = 100;
        fc.exclusions.insert(ExclusionReason::SectionAlreadyBurning, 320);
        fc.exclusions.insert(ExclusionReason::MergedSectionVictimBuildUnknown, 257);
        fc.exclusions.insert(ExclusionReason::AmbiguousWithAnotherHit, 171);
        fc.exclusions.insert(ExclusionReason::DamageControlActive, 59);
        fc.exclusions.insert(ExclusionReason::ImpactUnplaceableOnVictim, 23);

        assert_eq!(
            fire_chance_breakdown_lines(&fc)[..9],
            [
                "2312 HE shells fired".to_owned(),
                "1132 HE hits on a ship".to_owned(),
                "  202 could have started a fire".to_owned(),
                "  320 section already burning".to_owned(),
                "  257 victim build unknown, fire zones may be merged".to_owned(),
                "  171 ambiguous with another hit of ours".to_owned(),
                "   59 Damage Control Party active".to_owned(),
                "   23 could not place the impact on the ship we matched it to".to_owned(),
                "  100 victim already dead, not applicable".to_owned(),
            ]
        );
        assert_eq!(fc.eligible_hits + fc.exclusions.values().sum::<u32>() + fc.not_applicable, fc.he_hits_on_a_ship);
    }

    /// The block that answers "the game gave me six fires and this says four".
    /// The ribbon count is the game's own figure, the two lines under it are
    /// what became of them, and the reasons under those say why each uncredited
    /// one could not be tied to a shell.
    #[test]
    fn the_ribbon_block_accounts_for_every_fire_the_game_gave_us() {
        let mut fc = fixture(43, 4, None);
        fc.set_fire_ribbons = 6;
        fc.unattributed_fires = 2;
        fc.unattributed_reasons.insert(UnattributedFireReason::NoHitInWindow, 1);
        fc.unattributed_reasons.insert(UnattributedFireReason::EveryNearbyHitExcluded, 1);

        let lines = fire_chance_breakdown_lines(&fc);
        let ribbons = &lines[lines.len() - 5..];
        assert_eq!(
            ribbons,
            [
                "6 SetFire ribbons".to_owned(),
                "  4 credited to a shell".to_owned(),
                "  2 not credited".to_owned(),
                "    1 every nearby hit of ours was already excluded".to_owned(),
                "    1 no hit of ours landed in the window".to_owned(),
            ]
        );
    }

    /// The arithmetic the block exists to make visible: credited plus
    /// uncredited is the ribbon count, and the reasons account for the
    /// uncredited ones exactly.
    #[test]
    fn the_ribbon_block_adds_up() {
        let mut fc = fixture(43, 4, None);
        fc.set_fire_ribbons = 6;
        fc.unattributed_fires = 2;
        fc.unattributed_reasons.insert(UnattributedFireReason::BurnStateNotObserved, 2);
        let rows = fire_chance_ribbon_rows(&fc);
        assert_eq!(rows[0].count, fc.fires + fc.unattributed_fires);
        assert_eq!(rows.iter().filter(|row| row.depth == 1).map(|row| row.count).sum::<u32>(), rows[0].count);
        assert_eq!(rows.iter().filter(|row| row.depth == 2).map(|row| row.count).sum::<u32>(), fc.unattributed_fires);
    }

    /// Every ribbon credited leaves nothing to explain, so the block stops at
    /// the two lines that state as much rather than showing an empty heading.
    #[test]
    fn the_ribbon_block_omits_the_reasons_when_every_fire_was_credited() {
        let mut fc = fixture(43, 4, None);
        fc.set_fire_ribbons = 4;
        let lines = fire_chance_breakdown_lines(&fc);
        assert_eq!(&lines[lines.len() - 2..], ["4 SetFire ribbons".to_owned(), "  4 credited to a shell".to_owned()]);
    }

    /// A player who started no fires at all has no ribbon accounting to show,
    /// and a block of zeroes would read as a category with nothing in it.
    #[test]
    fn the_ribbon_block_is_absent_without_ribbons() {
        let fc = fixture(43, 0, None);
        assert!(fire_chance_ribbon_rows(&fc).is_empty());
        assert!(!fire_chance_breakdown_lines(&fc).iter().any(|line| line.contains("SetFire")));
    }

    /// The key match is exhaustive, so a stale arm is a compile error, but a key
    /// naming a string the toml does not carry is not. `rust-i18n` returns the
    /// key itself when it cannot resolve one, which is what this catches.
    #[test]
    fn every_breakdown_key_resolves_to_a_string() {
        const EXCLUSIONS: [ExclusionReason; 12] = [
            ExclusionReason::SectionAlreadyBurning,
            ExclusionReason::MergedSectionVictimBuildUnknown,
            ExclusionReason::DamageControlActive,
            ExclusionReason::DamageControlUnknown,
            ExclusionReason::ObservationGap,
            ExclusionReason::ConsumableModelUnreliable,
            ExclusionReason::VictimFateUnknown,
            ExclusionReason::HitTypeDoesNotRoll,
            ExclusionReason::NoSectionGeometry,
            ExclusionReason::ImpactUnplaceableOnVictim,
            ExclusionReason::VictimPoseUnknown,
            ExclusionReason::AmbiguousWithAnotherHit,
        ];
        const UNATTRIBUTED: [UnattributedFireReason; 7] = [
            UnattributedFireReason::BurnStateNotObserved,
            UnattributedFireReason::AlreadyCreditedToAnEarlierFire,
            UnattributedFireReason::ContestedByOurSecondary,
            UnattributedFireReason::ContestedByAnotherHitOfOurs,
            UnattributedFireReason::EveryNearbyHitExcluded,
            UnattributedFireReason::NoNearbyHitCouldStartAFire,
            UnattributedFireReason::NoHitInWindow,
        ];
        const LABELS: [&str; 13] = [
            "ui.replay.sections.fire_chance_breakdown",
            "ui.replay.sections.fire_chance_shells_fired",
            "ui.replay.sections.fire_chance_shells_fired_one",
            "ui.replay.sections.fire_chance_he_hits",
            "ui.replay.sections.fire_chance_he_hits_one",
            "ui.replay.sections.fire_chance_no_target_ship",
            "ui.replay.sections.fire_chance_no_target_ship_one",
            "ui.replay.sections.fire_chance_eligible",
            "ui.replay.sections.fire_chance_not_applicable",
            "ui.replay.sections.fire_chance_ribbons",
            "ui.replay.sections.fire_chance_ribbons_one",
            "ui.replay.sections.fire_chance_ribbons_credited",
            "ui.replay.sections.fire_chance_ribbons_uncredited",
        ];

        let mut keys: Vec<&'static str> = EXCLUSIONS.iter().map(|r| keys::exclusion_reason_key(*r)).collect();
        keys.extend(UNATTRIBUTED.iter().map(|r| keys::unattributed_fire_reason_key(*r)));
        keys.extend(LABELS);
        for key in keys {
            assert_ne!(t!(key), key, "{key} resolves to nothing");
        }
    }

    /// A row over no hits states no rate, including in the expected column: a
    /// sum over no hits implies no rate.
    #[test]
    fn per_ship_line_over_zero_eligible_hits_has_no_percentage() {
        let s = ship("Fletcher", 0, 0, Some(0.0));
        let line = fire_chance_per_ship_line(&s, &|s: &PerShipFireChance| s.victim_ship_name.clone());
        assert!(!line.contains('%'), "expected no percentage in {line:?}");
        assert_eq!(line, "Fletcher   no hits that could have started a fire");
    }

    #[test]
    fn sorted_per_ship_orders_by_eligible_hits_descending() {
        let mut fc = fixture(23, 3, None);
        fc.per_ship = vec![ship("Iowa", 11, 1, None), ship("Zao", 12, 2, None)];
        let names: Vec<&str> = sorted_per_ship(&fc).into_iter().map(|s| s.victim_ship_name.as_str()).collect();
        assert_eq!(names, vec!["Zao", "Iowa"]);
    }

    /// No shell ever resolved to compute a formula from, so there is nothing
    /// to show at all: not even a base line.
    #[test]
    fn formula_lines_are_empty_without_a_base() {
        assert!(fire_chance_formula_lines(&formula_fixture(None, Vec::new()), &no_localization).is_empty());
    }

    /// A resolved shell whose modifiers are all identities still shows the
    /// base and a total, even though there are no steps under it.
    #[test]
    fn formula_lines_show_the_base_even_with_no_steps() {
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.12), Vec::new()), &no_localization);
        assert_eq!(
            lines,
            vec![
                "Attacker fire chance formula".to_owned(),
                "    base burnProb 12.0%".to_owned(),
                "  = 12.0%".to_owned()
            ]
        );
    }

    /// Each step's source is localized, the value column reflects the op
    /// (a bare multiplier vs. a "+X.Ypp" bonus), and the total is the last
    /// step's running result.
    #[test]
    fn formula_lines_show_each_localized_step_and_the_total() {
        let formula = vec![
            formula_step("burnChanceFactorHighLevel", Some("ifhe_id"), FormulaOp::Multiply, 0.5, 0.06),
            formula_step("artilleryBurnChanceBonus", Some("de_id"), FormulaOp::Add, 0.01, 0.07),
        ];
        let localize = |source: &str| match source {
            "ifhe_id" => "IFHE".to_owned(),
            "de_id" => "DE".to_owned(),
            other => other.to_owned(),
        };
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.12), formula), &localize);
        assert_eq!(
            lines,
            vec![
                "Attacker fire chance formula".to_owned(),
                "    base burnProb                    12.0%".to_owned(),
                "  x burnChanceFactorHighLevel (IFHE) 0.50".to_owned(),
                "  + artilleryBurnChanceBonus (DE)    +1.0pp".to_owned(),
                "  = 7.0%".to_owned(),
            ]
        );
    }

    /// A step with no source renders its bare modifier name, unparenthesized.
    #[test]
    fn formula_lines_render_an_unsourced_step_without_parens() {
        let formula = vec![formula_step("burnProbModifier", None, FormulaOp::Multiply, 1.5, 0.18)];
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.12), formula), &no_localization);
        assert_eq!(lines[1], "    base burnProb    12.0%");
        assert_eq!(lines[2], "  x burnProbModifier 1.50");
        assert_eq!(lines[3], "  = 18.0%");
    }

    /// The eligibility model and `expected_fires` both read the clamped
    /// chance, so when the raw formula total runs past 100% the hover must
    /// show both values rather than only the disagreeing raw one.
    #[test]
    fn formula_lines_show_both_raw_and_clamped_when_they_disagree() {
        let formula = vec![formula_step("someBonus", None, FormulaOp::Add, 0.3, 1.2)];
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.9), formula), &no_localization);
        let total = lines.last().expect("a total line");
        assert!(total.contains("120.0%"), "got {total:?}");
        assert!(total.contains("100.0%"), "got {total:?}");
    }

    /// The raw and clamped values agree in the ordinary case, so only one
    /// number is shown.
    #[test]
    fn formula_lines_show_one_value_when_raw_and_clamped_agree() {
        let formula = vec![formula_step("someBonus", None, FormulaOp::Add, 0.01, 0.13)];
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.12), formula), &no_localization);
        assert_eq!(lines.last(), Some(&"  = 13.0%".to_owned()));
    }

    /// Column width is measured in characters, not bytes: a multi-byte
    /// localized name must not be padded as if it were wider than it displays.
    #[test]
    fn formula_lines_pad_by_character_count_not_byte_length() {
        // Ten U+00E9 ("e" with acute accent): 10 characters, but 20 bytes in
        // UTF-8, so it is longer than "base burnProb" (13 characters) by
        // byte count but shorter by character count. A width computed from
        // `.len()` would inflate the column to 20; the correct, char-counted
        // width is 13, driven by "base burnProb" instead.
        let name = "\u{e9}".repeat(10);
        let formula = vec![formula_step(&name, None, FormulaOp::Multiply, 1.0, 0.12)];
        let lines = fire_chance_formula_lines(&formula_fixture(Some(0.12), formula), &no_localization);
        assert_eq!(lines[1], "    base burnProb 12.0%");
        assert_eq!(lines[2], format!("  x {name}    1.00"));
    }
}

#[cfg(test)]
mod summary_source_selector_tests {
    use super::summary_source_selector;
    use crate::db::index::rows::SourceId;
    use crate::task::SourceSelector;

    /// A workspace with a resolved source reads that source explicitly,
    /// rather than falling back to the live source it may not even be.
    #[test]
    fn a_resolved_source_yields_explicit() {
        let selector = summary_source_selector(Some(SourceId(42)));
        assert_eq!(selector, SourceSelector::Explicit(SourceId(42)));
    }

    /// A workspace with no resolved source yet -- the live workspace for its
    /// whole life, or an imported workspace before its source is ensured --
    /// reads the live source. Getting this branch wrong strands the
    /// workspace's loading flag true forever, since the completion would
    /// never route back to it.
    #[test]
    fn no_resolved_source_yields_live() {
        let selector = summary_source_selector(None);
        assert_eq!(selector, SourceSelector::Live);
    }
}

#[cfg(test)]
mod alt_perspective_handoff_tests {
    use super::*;

    /// A minimal `ReplayFile`: a hand-built `ReplayMeta` round-tripped through
    /// `from_decrypted_parts`, the same entry point the app uses for a loaded
    /// replay's raw JSON.
    fn test_replay_file() -> ReplayFile {
        let meta = wows_replays::ReplayMeta {
            matchGroup: None,
            gameMode: 0,
            gameType: None,
            clientVersionFromExe: "0,0,0,0".to_string(),
            scenarioUiCategoryId: None,
            mapDisplayName: String::new(),
            mapId: 0,
            clientVersionFromXml: String::new(),
            weatherParams: None,
            duration: 0,
            gameLogic: None,
            name: String::new(),
            scenario: String::new(),
            playerID: wows_replays::types::AccountId(0),
            vehicles: Vec::new(),
            playersPerTeam: 0,
            dateTime: String::new(),
            mapName: String::new(),
            playerName: String::new(),
            scenarioConfigId: 0,
            teamsCount: 0,
            logic: None,
            playerVehicle: String::new(),
            battleDuration: None,
        };
        let meta_json = serde_json::to_vec(&meta).expect("ReplayMeta serializes");
        ReplayFile::from_decrypted_parts(meta_json, Vec::new()).expect("a ReplayMeta we just serialized parses back")
    }

    fn test_replay() -> Arc<RwLock<Replay>> {
        let resource_loader = Arc::new(
            wowsunpack::game_params::provider::GameMetadataProvider::from_params_no_specs(Vec::new())
                .expect("an empty param list is always valid"),
        );
        Arc::new(RwLock::new(Replay::new(test_replay_file(), resource_loader)))
    }

    /// The stash and the take must name one id, and the take must clear it. A
    /// mismatch costs an unreachable `ReplayFile` held for the session and a
    /// button that silently does nothing.
    #[test]
    fn a_stashed_request_is_delivered_once_and_leaves_nothing_behind() {
        let ctx = egui::Context::default();
        let replay = test_replay();
        let alt = Arc::new(test_replay_file());
        assert_eq!(Arc::strong_count(&alt), 1);

        stash_alt_reparse(
            &ctx,
            PendingAltRequest { workspace: WorkspaceId(4), replay: Arc::downgrade(&replay), alt: Arc::clone(&alt) },
        );
        assert_eq!(Arc::strong_count(&alt), 2, "the parked request holds the alt");

        let taken = take_alt_reparse(&ctx).expect("the request we just stashed comes back");
        assert_eq!(taken.workspace, WorkspaceId(4));
        assert!(Arc::ptr_eq(&taken.alt, &alt), "the delivered alt is the one that was picked");
        assert!(
            taken.replay.upgrade().is_some_and(|delivered| Arc::ptr_eq(&delivered, &replay)),
            "the delivered request names the replay it was stashed against"
        );

        drop(taken);
        assert_eq!(Arc::strong_count(&alt), 1, "the take leaves no copy of the alt in egui's store");
        assert!(take_alt_reparse(&ctx).is_none(), "a consumed request is not delivered a second time");
    }
}
