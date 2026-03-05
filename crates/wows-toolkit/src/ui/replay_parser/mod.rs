mod damage_types;
mod models;
mod sorting;

use std::path::PathBuf;

use crate::icon_str;
pub use models::Achievement;
pub use models::Damage;
pub use models::DamageInteraction;
pub use models::Hits;
pub use models::PlayerReport;
pub use models::PotentialDamage;
pub use models::SkillInfo;
pub use models::TranslatedBuild;
pub use models::ship_class_icon_from_species;
pub use sorting::ReplayColumn;
pub use sorting::SortColumn;
use sorting::SortKey;
pub use sorting::SortOrder;
use wows_replays::analyzer::battle_controller::ConnectionChangeKind;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io::BufWriter;
use std::io::Write;

use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;

use rootcause::Report;
use wowsunpack::game_params::types::ParamData;

use crate::collab::Permissions;
use crate::collab::SessionCommand;
use crate::collab::SessionStatus;
use crate::icons;
use crate::replay_export::FlattenedVehicle;
use crate::replay_export::Match;
use crate::settings::ReplayGrouping;
use crate::settings::ReplaySettings;
use crate::task::BackgroundTask;
use crate::task::BackgroundTaskKind;
use crate::task::ReplayExportFormat;
use crate::task::ReplaySource;
use crate::task::ToastMessage;
use crate::update_background_task;
use crate::wows_data::GameAsset;
use crate::wows_data::SharedWoWsData;

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
use wows_replays::ReplayFile;
use wows_replays::VehicleInfoMeta;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::analyzer::battle_controller::BattleReport;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ChatChannel;
use wows_replays::analyzer::battle_controller::GameMessage;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::types::AccountId;

use itertools::Itertools;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Species;

use crate::app::ReplayParserTabState;
use crate::app::ToolkitTabViewer;
use crate::error::ToolkitError;
use crate::plaintext_viewer;
use crate::plaintext_viewer::FileType;
use crate::util;
use crate::util::build_ship_config_url;
use crate::util::build_short_ship_config_url;
use crate::util::build_wows_numbers_url;
use crate::util::player_color_for_team_relation;
use crate::util::separate_number;

const CHAT_VIEW_WIDTH: f32 = 500.0;

/// A single replay viewer tab inside the Replay Inspector dock area.
#[derive(Clone)]
pub struct ReplayTab {
    pub replay: Arc<RwLock<Replay>>,
    /// Unique identifier for this tab instance.
    pub id: u64,
}

pub type SharedReplayParserTabState = Arc<Mutex<ReplayParserTabState>>;

/// A replay file path paired with its parsed replay data.
type ReplayEntry = (std::path::PathBuf, Arc<RwLock<Replay>>);
/// A named group of replay entries (e.g., grouped by date or ship name).
type ReplayGroup = (String, Vec<ReplayEntry>);

use std::cmp::Reverse;

/// Colorize a label based on battle result. Selected items get white-on-dark.
fn colorize_label(label: &str, battle_result: Option<BattleResult>, is_selected: bool) -> RichText {
    if is_selected {
        RichText::new(label).color(Color32::WHITE).background_color(Color32::DARK_GRAY)
    } else {
        match battle_result {
            Some(BattleResult::Win(_)) => RichText::new(label).color(Color32::LIGHT_GREEN),
            Some(BattleResult::Loss(_)) => RichText::new(label).color(Color32::LIGHT_RED),
            Some(BattleResult::Draw) => RichText::new(label).color(Color32::LIGHT_YELLOW),
            None => RichText::new(label),
        }
    }
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
) {
    if ui.button(icon_str!(icons::BROWSER, "Open in New Tab")).clicked() {
        if let Some(r) = replay_weak.upgrade() {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("open_replay_new_tab"), Arc::downgrade(&r));
            });
        }
        ui.close_kind(UiKind::Menu);
    }
    ui.separator();
    if ui.button(icon_str!(icons::CLIPBOARD, "Copy Path")).clicked() {
        ui.ctx().copy_text(path.to_string_lossy().into_owned());
        ui.close_kind(UiKind::Menu);
    }
    if ui.button(icon_str!(icons::CLIPBOARD, "Copy Replay")).clicked() {
        copy_files_to_clipboard(std::slice::from_ref(path));
        ui.close_kind(UiKind::Menu);
    }
    if ui.button(icon_str!(icons::FOLDER, "Show in File Explorer")).clicked() {
        util::open_file_explorer(path);
        ui.close_kind(UiKind::Menu);
    }
    if !wows_dir.is_empty() {
        let alt_held = ui.input(|i| i.modifiers.alt);
        let label = if alt_held {
            icon_str!(icons::KEYBOARD, "Show Replay Controls")
        } else {
            icon_str!(icons::GAME_CONTROLLER, "Open in Game")
        };
        if ui.button(label).clicked() {
            if alt_held {
                ui.ctx().data_mut(|data| {
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
    if ui.button(icon_str!(icons::PLAY, "Render Replay")).clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("context_menu_render_replay"), replay_weak.clone());
        });
        ui.close_kind(UiKind::Menu);
    }
    ui.separator();
    if ui.button("Set as Session Stats (1 replay)").clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("pending_confirmation_request"),
                Some(crate::tab_state::ConfirmableAction::SetAsSessionStats { replays: vec![replay_weak.clone()] }),
            );
        });
        ui.close_kind(UiKind::Menu);
    }
    if ui.button("Add to Session Stats (1 replay)").clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("add_to_session_stats_request"), vec![replay_weak.clone()]);
        });
        ui.close_kind(UiKind::Menu);
    }
}

/// Show context menu items for a group node (date or ship).
fn show_group_context_menu(ui: &mut egui::Ui, paths: &[std::path::PathBuf], replays: &[Weak<RwLock<Replay>>]) {
    let count = replays.len();
    let copy_label = if count == 1 { "Copy Replay".to_string() } else { format!("Copy {} Replays", count) };
    if ui.button(copy_label).clicked() {
        copy_files_to_clipboard(paths);
        ui.close_kind(UiKind::Menu);
    }
    let session_label = if count == 1 {
        "Set as Session Stats (1 replay)".to_string()
    } else {
        format!("Set as Session Stats ({} replays)", count)
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
    let add_label = if count == 1 {
        "Add to Session Stats (1 replay)".to_string()
    } else {
        format!("Add to Session Stats ({} replays)", count)
    };
    if ui.button(add_label).clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("add_to_session_stats_request"), replays.to_vec());
        });
        ui.close_kind(UiKind::Menu);
    }
}

/// Lookup maps for a grouped tree view, bundling leaf and group ID mappings.
#[derive(Clone)]
struct GroupedTreeMaps {
    /// Leaf node ID → replay (weak ref)
    leaf_replays: HashMap<egui::Id, Weak<RwLock<Replay>>>,
    /// Leaf node ID → file path
    leaf_paths: HashMap<egui::Id, std::path::PathBuf>,
    /// Group node ID → child replays (weak refs)
    group_replays: HashMap<egui::Id, Vec<Weak<RwLock<Replay>>>>,
    /// Group node ID → child node IDs
    group_child_ids: HashMap<egui::Id, Vec<egui::Id>>,
    /// Group node ID → child file paths
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
    fn show_multi_selection_context_menu(&self, ui: &mut egui::Ui, selected_ids: &[egui::Id]) {
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
                    data.insert_temp(egui::Id::new("add_to_session_stats_request"), selected_replays);
                });
                ui.close_kind(UiKind::Menu);
            }
        }
    }
}

/// Transform raw battle results from positional arrays to named objects.
/// Takes ownership of the parsed JSON to avoid cloning.
///
/// Input shape:
///   `{ "commonList": [v0, v1, ...], "playersPublicInfo": { "db_id": [v0, v1, ..., {interactions}], ... } }`
///
/// Output shape:
///   `{ "commonList": { "winner_team_id": v, ... }, "playersPublicInfo": { "db_id": { "exp": v, ..., "interactions": { "victim_id": { "fires": v, ... } } } } }`
fn resolve_battle_results(mut results: serde_json::Value, constants: &serde_json::Value) -> serde_json::Value {
    // Resolve commonList: array → object using COMMON_RESULTS names
    if let Some(common_names) = constants.pointer("/COMMON_RESULTS").and_then(|v| v.as_array())
        && let Some(common_arr) = results.get("commonList").and_then(|v| v.as_array())
    {
        results["commonList"] = serde_json::Value::Object(resolve_array(common_names, common_arr));
    }

    // Resolve each player in playersPublicInfo: array → object using CLIENT_PUBLIC_RESULTS_INDICES
    let indices = constants.pointer("/CLIENT_PUBLIC_RESULTS_INDICES").and_then(|v| v.as_object()).cloned();
    let interaction_fields = constants.pointer("/CLIENT_VEH_INTERACTION_DETAILS").and_then(|v| v.as_array()).cloned();

    if let (Some(indices), Some(players)) =
        (indices.as_ref(), results.get_mut("playersPublicInfo").and_then(|v| v.as_object_mut()))
    {
        for (_db_id, player_val) in players.iter_mut() {
            if let Some(arr) = player_val.as_array() {
                let mut obj = serde_json::Map::new();
                for (name, idx_val) in indices {
                    if let Some(idx) = idx_val.as_u64().map(|i| i as usize)
                        && let Some(value) = arr.get(idx)
                    {
                        obj.insert(name.clone(), value.clone());
                    }
                }

                // Resolve interactions: each victim's array → object
                if let Some(fields) = interaction_fields.as_ref()
                    && let Some(interactions) = obj.get_mut("interactions").and_then(|v| v.as_object_mut())
                {
                    for (_victim_id, victim_val) in interactions.iter_mut() {
                        if let Some(victim_arr) = victim_val.as_array() {
                            *victim_val = serde_json::Value::Object(resolve_array(fields, victim_arr));
                        }
                    }
                }

                *player_val = serde_json::Value::Object(obj);
            }
        }
    }

    results
}

/// Convert a positional array to a named object using a parallel names array.
/// `names[i]` provides the key for `values[i]`.
fn resolve_array(
    names: &[serde_json::Value],
    values: &[serde_json::Value],
) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    for (i, name_val) in names.iter().enumerate() {
        if let Some(name) = name_val.as_str()
            && let Some(value) = values.get(i)
        {
            map.insert(name.to_string(), value.clone());
        }
    }
    map
}

#[allow(non_camel_case_types)]
pub struct UiReport {
    match_timestamp: Timestamp,
    self_player: Option<Arc<Player>>,
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
    battle_result: Option<BattleResult>,
    resolved_results: Option<serde_json::Value>,
}

impl UiReport {
    pub fn new(
        replay_file: &ReplayFile,
        report: &BattleReport,
        wows_data: &SharedWoWsData,
        deps: &crate::wows_data::ReplayDependencies,
    ) -> Self {
        let wows_data_inner = wows_data.read();
        let metadata_provider = wows_data_inner.game_metadata.as_ref().expect("no game metadata?");
        let constants_inner = wows_data_inner.replay_constants.read();

        let match_timestamp = util::replay_timestamp(&replay_file.meta);

        let players = report.players().to_vec();

        let mut divisions: HashMap<u32, char> = Default::default();
        let mut remaining_div_identifiers: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().rev().collect();

        let self_player = players.iter().find(|player| player.relation().is_self()).cloned();

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

        let player_reports = players.iter().map(|player| {
            // Get the VehicleEntity for this player (may be None if they never spawned)
            let vehicle = player.vehicle_entity();
            let player_state = player.initial_state();
            let mut player_color = player_color_for_team_relation(player.relation());

            if let Some(self_player) = self_player.as_ref() {
                let self_state = self_player.initial_state();
                if self_state.db_id() != player_state.db_id()
                    && self_state.division_id() > 0
                    && player_state.division_id() == self_state.division_id()
                {
                    player_color = Color32::GOLD;
                }
            }

            let vehicle_param = player.vehicle();

            let known_species = vehicle_param.species().and_then(|r| r.known().cloned());

            let ship_species_text: String = known_species
                .as_ref()
                .and_then(|species| metadata_provider.localized_name_from_id(&species.translation_id()))
                .unwrap_or_else(|| "unk".to_string());

            let icon =
                known_species.as_ref().and_then(|species| ship_class_icon_from_species(*species, &wows_data_inner));

            let name_color = if player_state.is_abuser() {
                Color32::from_rgb(0xFF, 0xC0, 0xCB) // pink
            } else {
                player_color
            };

            // Assign division
            let div = player_state.division_id() as u32;
            let division_char = if div > 0 {
                Some(*divisions.entry(div).or_insert_with(|| remaining_div_identifiers.pop().unwrap_or('?')))
            } else {
                None
            };

            let div_text = division_char.map(|div| format!("({div})"));

            let clan_text = if !player_state.clan().is_empty() {
                Some(RichText::new(format!("[{}]", player_state.clan())).color(clan_color_for_player(player).unwrap()))
            } else {
                None
            };
            let display_name = if player_state.is_bot() && player_state.username().starts_with("IDS_") {
                metadata_provider
                    .localized_name_from_id(player_state.username())
                    .unwrap_or_else(|| player_state.username().to_string())
            } else {
                player_state.username().to_string()
            };
            let name_text = RichText::new(&display_name).color(name_color);

            // Look up this player's resolved results by db_id
            let player_results = resolved_results
                .as_ref()
                .and_then(|r| r.pointer(&format!("/playersPublicInfo/{}", player_state.db_id())));

            let (base_xp, base_xp_text) = if let Some(base_xp) = player_results.and_then(|pr| pr.get("exp")?.as_i64()) {
                let label_text = separate_number(base_xp, Some(locale));
                (Some(base_xp), Some(RichText::new(label_text).color(player_color)))
            } else {
                (None, None)
            };

            let (raw_xp, raw_xp_text) = if let Some(raw_xp) = player_results.and_then(|pr| pr.get("raw_exp")?.as_i64())
            {
                let label_text = separate_number(raw_xp, Some(locale));
                (Some(raw_xp), Some(label_text))
            } else {
                (None, None)
            };

            let ship_name = metadata_provider
                .localized_name_from_param(vehicle_param)
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("{}", vehicle_param.id()));

            let observed_damage = vehicle.map(|v| v.damage().ceil() as u64).unwrap_or(0);
            let observed_damage_text = separate_number(observed_damage, Some(locale));

            // Actual damage done to other players
            let (damage, damage_text, damage_hover_text, damage_report) = player_results
                .and_then(|pr| {
                    let damage_number = pr.get("damage")?.as_u64()?;

                    let longest_width = DAMAGE_DESCRIPTIONS
                        .iter()
                        .filter_map(|(key, description)| {
                            let num = pr.get(*key)?.as_u64()?;
                            if num > 0 { Some(description.len()) } else { None }
                        })
                        .max()
                        .unwrap_or_default()
                        + 1;

                    let (all_damage, breakdowns): (Vec<(String, u64)>, Vec<String>) = DAMAGE_DESCRIPTIONS
                        .iter()
                        .filter_map(|(key, description)| {
                            let num = pr.get(*key)?.as_u64()?;
                            if num > 0 {
                                let num_str = separate_number(num, Some(locale));
                                Some(((key.to_string(), num), format!("{description:<longest_width$}: {num_str}")))
                            } else {
                                None
                            }
                        })
                        .collect();

                    let all_damage: HashMap<String, u64> = HashMap::from_iter(all_damage);

                    let damage_report_text = separate_number(damage_number, Some(locale));
                    let damage_report_text = RichText::new(damage_report_text).color(player_color);
                    let damage_report_hover_text = RichText::new(breakdowns.join("\n")).font(FontId::monospace(12.0));

                    Some((
                        Some(damage_number),
                        Some(damage_report_text),
                        Some(damage_report_hover_text),
                        Some(Damage {
                            ap: all_damage.get(DAMAGE_MAIN_AP).copied(),
                            sap: all_damage.get(DAMAGE_MAIN_CS).copied(),
                            he: all_damage.get(DAMAGE_MAIN_HE).copied(),
                            he_secondaries: all_damage.get(DAMAGE_ATBA_HE).copied(),
                            sap_secondaries: all_damage.get(DAMAGE_ATBA_CS).copied(),
                            torps: all_damage.get(DAMAGE_TPD_NORMAL).copied(),
                            deep_water_torps: all_damage.get(DAMAGE_TPD_DEEP).copied(),
                            fire: all_damage.get(DAMAGE_FIRE).copied(),
                            flooding: all_damage.get(DAMAGE_FLOOD).copied(),
                        }),
                    ))
                })
                .unwrap_or_default();

            // Armament hit information
            let (hits, hits_text, hits_hover_text, hits_report) = player_results
                .map(|pr| {
                    let longest_width = HITS_DESCRIPTIONS
                        .iter()
                        .filter_map(|(key, description)| {
                            let num = pr.get(*key)?.as_u64()?;
                            if num > 0 { Some(description.len()) } else { None }
                        })
                        .max()
                        .unwrap_or_default()
                        + 1;

                    let (all_hits, breakdowns): (Vec<(String, u64)>, Vec<String>) = HITS_DESCRIPTIONS
                        .iter()
                        .filter_map(|(key, description)| {
                            let num = pr.get(*key)?.as_u64()?;
                            if num > 0 {
                                let num_str = separate_number(num, Some(locale));
                                Some(((key.to_string(), num), format!("{description:<longest_width$}: {num_str}")))
                            } else {
                                None
                            }
                        })
                        .collect();

                    let all_hits: HashMap<String, u64> = HashMap::from_iter(all_hits);

                    let main_hits = all_hits.get(HITS_MAIN_HE).copied().unwrap_or(0)
                        + all_hits.get(HITS_MAIN_CS).copied().unwrap_or(0)
                        + all_hits.get(HITS_MAIN_AP).copied().unwrap_or(0);

                    let plane_hits = all_hits.get(HITS_ROCKET).copied().unwrap_or(0)
                        + all_hits.get(HITS_ROCKET_AIRSUPPORT).copied().unwrap_or(0)
                        + all_hits.get(HITS_SKIP).copied().unwrap_or(0)
                        + all_hits.get(HITS_SKIP_ALT).copied().unwrap_or(0)
                        + all_hits.get(HITS_SKIP_AIRSUPPORT).copied().unwrap_or(0);

                    let relevant_hits_number = if vehicle_param
                        .species()
                        .and_then(|r| r.known())
                        .map(|s| *s == Species::AirCarrier)
                        .unwrap_or(false)
                    {
                        plane_hits
                    } else {
                        main_hits
                    };

                    let main_hits_text = separate_number(relevant_hits_number, Some(locale));
                    let main_hits_text = RichText::new(main_hits_text).color(player_color);
                    let hits_hover_text = RichText::new(breakdowns.join("\n")).font(FontId::monospace(12.0));

                    (
                        Some(relevant_hits_number),
                        Some(main_hits_text),
                        Some(hits_hover_text),
                        Some(Hits {
                            ap: all_hits.get(HITS_MAIN_AP).copied(),
                            sap: all_hits.get(HITS_MAIN_CS).copied(),
                            he: all_hits.get(HITS_MAIN_HE).copied(),
                            he_secondaries: all_hits.get(HITS_ATBA_HE).copied(),
                            sap_secondaries: all_hits.get(HITS_ATBA_CS).copied(),
                            ap_secondaries_manual: all_hits.get(HITS_ATBA_AP_MANUAL).copied(),
                            he_secondaries_manual: all_hits.get(HITS_ATBA_HE_MANUAL).copied(),
                            sap_secondaries_manual: all_hits.get(HITS_ATBA_CS_MANUAL).copied(),
                            torps: all_hits.get(HITS_TPD_NORMAL).copied(),
                        }),
                    )
                })
                .unwrap_or_default();

            // Received damage
            let (received_damage, received_damage_text, received_damage_hover_text, received_damage_report) =
                player_results
                    .map(|pr| {
                        let longest_width = RECEIVED_DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let num = pr.get(format!("received_{key}"))?.as_u64()?;
                                if num > 0 { Some(description.len()) } else { None }
                            })
                            .max()
                            .unwrap_or_default()
                            + 1;

                        let (all_damage, breakdowns): (Vec<(String, u64)>, Vec<String>) = RECEIVED_DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let num = pr.get(format!("received_{key}"))?.as_u64()?;
                                if num > 0 {
                                    let num_str = separate_number(num, Some(locale));
                                    Some(((key.to_string(), num), format!("{description:<longest_width$}: {num_str}")))
                                } else {
                                    None
                                }
                            })
                            .collect();

                        let all_damage: HashMap<String, u64> = HashMap::from_iter(all_damage);
                        let total_received: u64 = all_damage.values().sum();

                        let received_damage_report_text = separate_number(total_received, Some(locale));
                        let received_damage_report_text =
                            RichText::new(received_damage_report_text).color(player_color);
                        let received_damage_report_hover_text =
                            RichText::new(breakdowns.iter().join("\n")).font(FontId::monospace(12.0));

                        (
                            Some(total_received),
                            Some(received_damage_report_text),
                            Some(received_damage_report_hover_text),
                            Some(Damage {
                                ap: all_damage.get(DAMAGE_MAIN_AP).copied(),
                                sap: all_damage.get(DAMAGE_MAIN_CS).copied(),
                                he: all_damage.get(DAMAGE_MAIN_HE).copied(),
                                he_secondaries: all_damage.get(DAMAGE_ATBA_HE).copied(),
                                sap_secondaries: all_damage.get(DAMAGE_ATBA_CS).copied(),
                                torps: all_damage.get(DAMAGE_TPD_NORMAL).copied(),
                                deep_water_torps: all_damage.get(DAMAGE_TPD_DEEP).copied(),
                                fire: all_damage.get(DAMAGE_FIRE).copied(),
                                flooding: all_damage.get(DAMAGE_FLOOD).copied(),
                            }),
                        )
                    })
                    .unwrap_or_default();

            // Spotting damage
            let (spotting_damage, spotting_damage_text) =
                if let Some(damage_number) = player_results.and_then(|pr| pr.get("scouting_damage")?.as_u64()) {
                    (Some(damage_number), Some(separate_number(damage_number, Some(locale))))
                } else {
                    (None, None)
                };

            let (potential_damage, potential_damage_text, potential_damage_hover_text, potential_damage_report) =
                player_results
                    .map(|pr| {
                        let longest_width = POTENTIAL_DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let num =
                                    pr.get(*key)?.as_u64().or_else(|| pr.get(*key)?.as_f64().map(|f| f as u64))?;
                                if num > 0 { Some(description.len()) } else { None }
                            })
                            .max()
                            .unwrap_or_default()
                            + 1;

                        let (all_agro, breakdowns): (Vec<(String, u64)>, Vec<String>) = POTENTIAL_DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter_map(|(key, description)| {
                                let num =
                                    pr.get(*key)?.as_u64().or_else(|| pr.get(*key)?.as_f64().map(|f| f as u64))?;
                                if num > 0 {
                                    let num_str = separate_number(num, Some(locale));
                                    Some(((key.to_string(), num), format!("{description:<longest_width$}: {num_str}")))
                                } else {
                                    None
                                }
                            })
                            .unzip();
                        let all_agro: HashMap<String, u64> = HashMap::from_iter(all_agro);

                        let total_agro = all_agro.values().sum();
                        let damage_report_text = separate_number(total_agro, Some(locale));
                        let damage_report_hover_text =
                            RichText::new(breakdowns.join("\n")).font(FontId::monospace(12.0));

                        (
                            Some(total_agro),
                            Some(damage_report_text),
                            Some(damage_report_hover_text),
                            Some(PotentialDamage {
                                artillery: all_agro.get("agro_art").copied().unwrap_or_default(),
                                torpedoes: all_agro.get("agro_tpd").copied().unwrap_or_default(),
                                planes: all_agro.get("agro_air").copied().unwrap_or_default(),
                            }),
                        )
                    })
                    .unwrap_or_default();

            let (time_lived, time_lived_text) = vehicle
                .and_then(|v| v.death_info())
                .map(|death_info| {
                    let secs = death_info.time_lived().as_secs();
                    (Some(secs), Some(format!("{}:{:02}", secs / 60, secs % 60)))
                })
                .unwrap_or_default();

            let species = vehicle_param.species().and_then(|r| r.known()).cloned().expect("ship has no species?");
            let (skill_points, num_skills, highest_tier, num_tier_1_skills) = vehicle
                .and_then(|v| v.commander_skills(species))
                .map(|skills| {
                    let points =
                        skills.iter().fold(0usize, |accum, skill| accum + skill.tier().get_for_species(species));
                    let highest_tier = skills.iter().map(|skill| skill.tier().get_for_species(species)).max();
                    let num_tier_1_skills = skills.iter().fold(0, |mut accum, skill| {
                        if skill.tier().get_for_species(species) == 1 {
                            accum += 1;
                        }
                        accum
                    });

                    (points, skills.len(), highest_tier.unwrap_or(0), num_tier_1_skills)
                })
                .unwrap_or((0, 0, 0, 0));

            let (label, hover_text) = util::colorize_captain_points(
                skill_points,
                num_skills,
                highest_tier,
                num_tier_1_skills,
                vehicle.and_then(|v| v.commander_skills(species)),
            );

            let skill_info =
                SkillInfo { skill_points, num_skills, highest_tier, num_tier_1_skills, hover_text, label_text: label };

            let (damage_interactions, fires, floods, cits, crits) = player_results
                .and_then(|pr| pr.get("interactions")?.as_object())
                .map(|interactions| {
                    let mut damage_interactions = HashMap::new();
                    let mut fires = 0u64;
                    let mut floods = 0u64;
                    let mut cits = 0u64;
                    let mut crits = 0u64;

                    for (victim, victim_data) in interactions {
                        let victim_id = AccountId(victim.parse::<i64>().unwrap_or_default());

                        fires += victim_data.get("fires").and_then(|v| v.as_u64()).unwrap_or(0);
                        floods += victim_data.get("floods").and_then(|v| v.as_u64()).unwrap_or(0);
                        cits += victim_data.get("citadels").and_then(|v| v.as_u64()).unwrap_or(0);
                        crits += victim_data.get("crits").and_then(|v| v.as_u64()).unwrap_or(0);

                        let mut damage_interaction = DamageInteraction::default();

                        let longest_width = DAMAGE_DESCRIPTIONS
                            .iter()
                            .filter(|(key, _)| {
                                victim_data.get(*key).and_then(|v| v.as_u64()).is_some_and(|n| n > 0)
                            })
                            .map(|(_, desc)| desc.len())
                            .max()
                            .unwrap_or_default()
                            + 1;

                        let mut per_type = Vec::new();
                        let (all_damage, breakdowns): (u64, Vec<String>) = DAMAGE_DESCRIPTIONS
                            .iter()
                            .fold((0u64, Vec::new()), |(sum, mut lines), (key, description)| {
                                let num = victim_data.get(*key).and_then(|v| v.as_u64()).unwrap_or(0);
                                if num > 0 {
                                    per_type.push((key.to_string(), num));
                                    let num_str = separate_number(num, Some(locale));
                                    lines.push(format!("{description:<longest_width$}: {num_str}"));
                                }
                                (sum + num, lines)
                            });

                        damage_interaction.damage_dealt = all_damage;
                        if damage_interaction.damage_dealt > 0 {
                            damage_interaction.damage_dealt_text =
                                separate_number(damage_interaction.damage_dealt, Some(locale));
                            damage_interaction.damage_dealt_hover_text = breakdowns.join("\n");

                            if let Some(total_damage) = damage {
                                damage_interaction.damage_dealt_percentage =
                                    (all_damage as f64 / total_damage as f64) * 100.0;
                                damage_interaction.damage_dealt_percentage_text =
                                    format!("{:.0}%", damage_interaction.damage_dealt_percentage);
                            }
                        }

                        damage_interactions.insert(victim_id, damage_interaction);
                    }

                    (Some(damage_interactions), Some(fires), Some(floods), Some(cits), Some(crits))
                })
                .unwrap_or_default();

            let distance_traveled = player_results.and_then(|pr| pr.get("distance")?.as_f64());

            let kills = player_results.and_then(|pr| pr.get("ships_killed")?.as_i64());
            let observed_kills = vehicle.map(|v| v.frags().len() as i64).unwrap_or(0);

            let is_test_ship = vehicle_param
                .data()
                .vehicle_ref()
                .map(|vehicle| vehicle.group().starts_with("demo"))
                .unwrap_or_default();

            let achievements = player_results
                .and_then(|pr| pr.get("achievements")?.as_array())
                .map(|achievements_array| {
                    achievements_array
                        .iter()
                        .filter_map(|achievement_info| {
                            let achievement_info = achievement_info.as_array()?;
                            let achievement_id = achievement_info[0].as_u64()?;
                            let achievement_count = achievement_info[1].as_u64()?;

                            // Look this achievement up from game params
                            let game_param = <GameMetadataProvider as GameParamProvider>::game_param_by_id(
                                metadata_provider,
                                (achievement_id as u32).into(),
                            )?;

                            let ParamData::Achievement(achievement_data) = game_param.data() else {
                                return None;
                            };

                            let ui_name = achievement_data.ui_name().to_string();
                            let achievement_name = wowsunpack::game_params::translations::translate_achievement_name(
                                &ui_name,
                                metadata_provider.as_ref(),
                            )?;

                            let achievement_description =
                                wowsunpack::game_params::translations::translate_achievement_description(
                                    &ui_name,
                                    metadata_provider.as_ref(),
                                )?;

                            Some(Achievement {
                                game_param,
                                display_name: achievement_name,
                                description: achievement_description,
                                icon_key: ui_name,
                                count: achievement_count as usize,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            // Extract ribbons from resolved player results
            // Ribbon keys start with RIBBON_ in the resolved object
            let ribbons = player_results
                .and_then(|pr| pr.as_object())
                .map(|pr_obj| {
                    let mut ribbons = HashMap::new();

                    for (key, value) in pr_obj {
                        if !key.starts_with("RIBBON_") {
                            continue;
                        }

                        let count = value.as_u64().unwrap_or(0);
                        if count == 0 {
                            continue;
                        }

                        // Look up the display name and description via shared translation helper.
                        let Some(ribbon_translation) =
                            wowsunpack::game_params::translations::translate_ribbon(key, metadata_provider.as_ref())
                        else {
                            continue;
                        };

                        let display_name = ribbon_translation.display_name;
                        let is_subribbon = ribbon_translation.is_subribbon;
                        let description = ribbon_translation.description;
                        let icon_key = ribbon_translation.icon_key;

                        ribbons.insert(
                            key.to_string(),
                            models::Ribbon {
                                name: key.to_string(),
                                display_name,
                                description,
                                icon_key,
                                is_subribbon,
                                count,
                            },
                        );
                    }

                    ribbons
                })
                .unwrap_or_default();

            PlayerReport {
                player: Arc::clone(player),
                color: player_color,
                name_text,
                clan_text,
                icon,
                division_label: div_text,
                base_xp,
                base_xp_text,
                raw_xp,
                raw_xp_text,
                observed_damage,
                observed_damage_text,
                actual_damage: damage,
                actual_damage_report: damage_report,
                actual_damage_text: damage_text,
                actual_damage_hover_text: damage_hover_text,
                ship_name,
                spotting_damage,
                spotting_damage_text,
                potential_damage,
                potential_damage_hover_text,
                potential_damage_report,
                time_lived_secs: time_lived,
                time_lived_text,
                skill_info,
                potential_damage_text,
                ship_species_text,
                received_damage,
                received_damage_text,
                received_damage_hover_text,
                fires,
                floods,
                citadels: cits,
                crits,
                distance_traveled,
                is_test_ship,
                relation: player.relation(),
                manual_stat_hide_toggle: false,
                received_damage_report,
                kills,
                observed_kills,
                translated_build: TranslatedBuild::new(player, metadata_provider),
                hits,
                hits_report,
                hits_text,
                hits_hover_text,
                damage_interactions,
                achievements,
                ribbons,
                personal_rating: None,
                has_vehicle_entity: vehicle.is_some(),
            }
        });

        let mut player_reports: Vec<PlayerReport> = player_reports.collect();
        let mut all_received_damages = HashMap::new();

        // For each player report, we need to update the damage interactions so they
        // have the correct received damage
        for report in &player_reports {
            let mut received_damages = HashMap::new();
            let Some(damage_interactions) = report.damage_interactions.as_ref() else {
                continue;
            };

            let this_player = report.player();
            let this_player_state = this_player.initial_state();

            for player_id in damage_interactions.keys() {
                let Some(other_player) =
                    player_reports.iter().find(|report| report.player().initial_state().db_id() == *player_id)
                else {
                    continue;
                };

                if let Some(interactions) = other_player.damage_interactions.as_ref() {
                    let Some(interaction_with_this_player) = interactions.get(&this_player_state.db_id()) else {
                        continue;
                    };

                    received_damages.insert(
                        *player_id,
                        (
                            interaction_with_this_player.damage_dealt,
                            interaction_with_this_player.damage_dealt_text.clone(),
                            interaction_with_this_player.damage_dealt_hover_text.clone(),
                        ),
                    );
                }
            }

            all_received_damages.insert(this_player_state.db_id(), received_damages);
        }

        for report in &mut player_reports {
            let this_player = report.player();
            let this_player_state = this_player.initial_state();

            let Some(this_player_received_damages) = all_received_damages.remove(&this_player_state.db_id()) else {
                continue;
            };

            let Some(interaction_report) = report.damage_interactions.as_mut() else {
                continue;
            };

            // Sum from per-interaction attacker data so all damage types (including
            // depth charges) are consistently counted in both numerator and denominator.
            let total_received_damage: u64 = this_player_received_damages.values().map(|(dmg, _, _)| *dmg).sum();

            for (interacted_player_id, (received_damage, received_damage_text, received_damage_hover_text)) in
                this_player_received_damages
            {
                let interacted_player = interaction_report.entry(interacted_player_id).or_default();
                interacted_player.damage_received = received_damage;
                interacted_player.damage_received_text = received_damage_text;
                interacted_player.damage_received_hover_text = received_damage_hover_text;

                if total_received_damage > 0 {
                    interacted_player.damage_received_percentage =
                        (received_damage as f64 / total_received_damage as f64) * 100.0;
                    interacted_player.damage_received_percentage_text =
                        format!("{:.0}%", interacted_player.damage_received_percentage);
                }
            }
        }

        // Third pass: compute inverse percentages using the other player's totals.
        // dealt_inverse = damage_dealt / victim's total received damage
        // received_inverse = damage_received / attacker's total dealt damage
        {
            // Collect totals first to avoid borrow issues.
            // For received damage, sum per-interaction values so all damage types
            // (including depth charges) are counted consistently.
            let totals: HashMap<AccountId, (u64, u64)> = player_reports
                .iter()
                .map(|r| {
                    let id = r.player().initial_state().db_id();
                    let dealt = r.actual_damage().unwrap_or_default();
                    let received: u64 = r
                        .damage_interactions
                        .as_ref()
                        .map(|interactions| interactions.values().map(|i| i.damage_received).sum())
                        .unwrap_or_default();
                    (id, (dealt, received))
                })
                .collect();

            for report in &mut player_reports {
                let Some(interactions) = report.damage_interactions.as_mut() else {
                    continue;
                };

                for (other_id, interaction) in interactions.iter_mut() {
                    let Some(&(other_dealt, other_received)) = totals.get(other_id) else {
                        continue;
                    };

                    if other_received > 0 && interaction.damage_dealt > 0 {
                        interaction.damage_dealt_inverse_percentage =
                            (interaction.damage_dealt as f64 / other_received as f64) * 100.0;
                        interaction.damage_dealt_inverse_percentage_text =
                            format!("{:.0}%", interaction.damage_dealt_inverse_percentage);
                    }

                    if other_dealt > 0 && interaction.damage_received > 0 {
                        interaction.damage_received_inverse_percentage =
                            (interaction.damage_received as f64 / other_dealt as f64) * 100.0;
                        interaction.damage_received_inverse_percentage_text =
                            format!("{:.0}%", interaction.damage_received_inverse_percentage);
                    }
                }
            }
        }

        drop(constants_inner);
        drop(wows_data_inner);

        Self {
            match_timestamp,
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
                ReplayColumn::ReceivedDamage,
                ReplayColumn::PotentialDamage,
                ReplayColumn::SpottingDamage,
                ReplayColumn::TimeLived,
                ReplayColumn::Fires,
                ReplayColumn::Floods,
                ReplayColumn::Citadels,
                ReplayColumn::Crits,
                ReplayColumn::DistanceTraveled,
                ReplayColumn::Skills,
            ],
            row_heights: Default::default(),
            background_task_sender: Some(deps.background_task_sender.clone()),
            selected_row: None,
            debug_mode: deps.is_debug_mode,
            resolved_results,
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
            (ReplayColumn::Fires, settings.show_fires),
            (ReplayColumn::Floods, settings.show_floods),
            (ReplayColumn::Citadels, settings.show_citadels),
            (ReplayColumn::Crits, settings.show_crits),
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
                    if let Some(clan_text) = interaction_player.clan_text() {
                        clan_text.clone().append_to(
                            &mut hover_layout,
                            &style,
                            egui::FontSelection::Default,
                            egui::Align::Center,
                        );
                        hover_layout.append(" ", 0.0, Default::default());
                    }
                    interaction_player.name_text.clone().append_to(
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
                                RichText::new(&interaction.1.damage_received_hover_text)
                                    .font(FontId::monospace(12.0)),
                            );
                        });
                    }
                }
            };
        });
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
                    if let Some(clan_text) = interaction_player.clan_text() {
                        clan_text.clone().append_to(
                            &mut hover_layout,
                            &style,
                            egui::FontSelection::Default,
                            egui::Align::Center,
                        );
                        hover_layout.append(" ", 0.0, Default::default());
                    }
                    interaction_player.name_text.clone().append_to(
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
                                RichText::new(&interaction.1.damage_dealt_hover_text)
                                    .font(FontId::monospace(12.0)),
                            );
                        });
                    }
                }
            };
        });
    }

    fn cell_content_ui(&mut self, row_nr: u64, col_nr: usize, ui: &mut egui::Ui) {
        let is_expanded = self.is_row_expanded.get(&row_nr).copied().unwrap_or_default();
        let expandedness = ui.ctx().animate_bool(Id::new(row_nr), is_expanded);

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
                            .tint(report.color)
                            .fit_to_exact_size((20.0, 20.0).into())
                            .rotate(90.0_f32.to_radians(), Vec2::splat(0.5));

                            ui.add(image).on_hover_text(&report.ship_species_text);
                        } else {
                            ui.label(&report.ship_species_text);
                        }

                        // Add division ID
                        if let Some(div) = report.division_label.as_ref() {
                            ui.label(div);
                        }

                        // Add player clan
                        if let Some(clan_text) = report.clan_text.clone() {
                            ui.label(clan_text);
                        }

                        // Add player name
                        ui.label(report.name_text.clone());

                        // Add icons for player properties
                        {
                            let player = report.player();
                            // Hidden profile icon
                            if player.initial_state().is_hidden() {
                                ui.label(icons::EYE_SLASH).on_hover_text("Player has a hidden profile");
                            }

                            // Stream sniper icon
                            if let Some(timestamps) = self.twitch_state.read().player_is_potential_stream_sniper(
                                player.initial_state().username(),
                                self.match_timestamp,
                            ) {
                                let hover_text = timestamps
                                    .iter()
                                    .map(|(name, timestamps)| {
                                        format!(
                                            "Possible stream name: {}\nSeen: {} minutes after match start",
                                            name,
                                            timestamps
                                                .iter()
                                                .map(|ts| {
                                                    let delta = *ts - self.match_timestamp;
                                                    delta.total(jiff::Unit::Minute).unwrap_or(0.0) as i64
                                                })
                                                .join(", ")
                                        )
                                    })
                                    .join("\n\n");
                                ui.label(icons::TWITCH_LOGO).on_hover_text(hover_text);
                            }

                            let disconnect_hover_text = if player.connection_change_info().is_empty() {
                                Some("Player never connected to match".to_string())
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
                        if let Some(base_xp_text) = report.base_xp_text.clone() {
                            ui.label(base_xp_text);
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
                            ui.label("NDA");
                        } else {
                            ui.label(&report.observed_damage_text);
                        }
                    }
                    ReplayColumn::ActualDamage => {
                        if let Some(damage_text) = report.actual_damage_text.clone() {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                let response = ui.label(damage_text);
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
                        if let Some(received_damage_text) = report.received_damage_text.clone() {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                let response = ui.label(received_damage_text);
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
                                ui.label("NDA");
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
                            ui.label(spotting_damage_text);
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
                    ReplayColumn::Fires => {
                        if let Some(fires) = report.fires {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                ui.label(fires.to_string());
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Floods => {
                        if let Some(floods) = report.floods {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                ui.label(floods.to_string());
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Citadels => {
                        if let Some(citadels) = report.citadels {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                ui.label(citadels.to_string());
                            }
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Crits => {
                        if let Some(crits) = report.crits {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                ui.label(crits.to_string());
                            }
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
                            ui.label(RichText::new(icon_str!(icons::EXCLAMATION_MARK, "-")).color(Color32::LIGHT_RED))
                                .on_hover_text("This ship was never spotted. Build info unavailable.");
                        } else {
                            let response = ui.label(report.skill_info.label_text.clone());
                            if let Some(hover_text) = &report.skill_info.hover_text {
                                response.on_hover_text(hover_text);
                            }
                        }
                    }
                    ReplayColumn::PersonalRating => {
                        if let Some(pr) = report.personal_rating.as_ref() {
                            ui.label(RichText::new(format!("{:.0}", pr.pr)).color(pr.category.color()))
                                .on_hover_text(pr.category.name());
                        } else {
                            ui.label("-");
                        }
                    }
                    ReplayColumn::Actions => {
                        ui.menu_button(icons::DOTS_THREE, |ui| {
                            if (!report.relation().is_enemy() || self.debug_mode) && report.has_vehicle_entity {
                                if ui.small_button(icon_str!(icons::SHARE, "Open Build in Browser")).clicked() {
                                    let metadata_provider = self.metadata_provider();

                                    if let Some(url) = build_ship_config_url(report.player(), &metadata_provider) {
                                        ui.ctx().open_url(OpenUrl::new_tab(url));
                                    }
                                    ui.close_kind(UiKind::Menu);
                                }

                                if ui.small_button(icon_str!(icons::COPY, "Copy Build Link")).clicked() {
                                    let metadata_provider = self.metadata_provider();

                                    if let Some(url) = build_ship_config_url(report.player(), &metadata_provider) {
                                        ui.ctx().copy_text(url);

                                        let _ = self.background_task_sender.as_ref().map(|sender| {
                                            sender.send(BackgroundTask {
                                                receiver: None,
                                                kind: BackgroundTaskKind::UpdateTimedMessage(ToastMessage::success(
                                                    "Build link copied",
                                                )),
                                            })
                                        });
                                    }

                                    ui.close_kind(UiKind::Menu);
                                }

                                if ui.small_button(icon_str!(icons::COPY, "Copy Short Build Link")).clicked() {
                                    let metadata_provider = self.metadata_provider();

                                    if let Some(url) = build_short_ship_config_url(report.player(), &metadata_provider)
                                    {
                                        ui.ctx().copy_text(url);
                                        let _ = self.background_task_sender.as_ref().map(|sender| {
                                            sender.send(BackgroundTask {
                                                receiver: None,
                                                kind: BackgroundTaskKind::UpdateTimedMessage(ToastMessage::success(
                                                    "Build link copied",
                                                )),
                                            })
                                        });
                                    }

                                    ui.close_kind(UiKind::Menu);
                                }

                                ui.separator();
                            }

                            if ui.small_button(icon_str!(icons::SHARE, "Open WoWs Numbers Page")).clicked() {
                                if let Some(url) = build_wows_numbers_url(report.player()) {
                                    ui.ctx().open_url(OpenUrl::new_tab(url));
                                }

                                ui.close_kind(UiKind::Menu);
                            }

                            if self.debug_mode {
                                ui.separator();

                                if let Some(player) = Some(report.player())
                                    && ui.small_button(icon_str!(icons::BUG, "View Raw Player Metadata")).clicked()
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
                        if let Some(hits_text) = report.hits_text.clone() {
                            if report.should_hide_stats() && !self.debug_mode {
                                ui.label("NDA");
                            } else {
                                let response = ui.label(hits_text);
                                if let Some(hover_text) = report.hits_hover_text.clone() {
                                    response.on_hover_text(hover_text);
                                }
                            }
                        } else {
                            ui.label("-");
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
                                ui.strong("Achievements");

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
                                ui.strong("Ribbons");

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
                                        // Try to find the icon - check subribbons first, then ribbons
                                        let icon = if ribbon.is_subribbon {
                                            wows_data.subribbon_icons.get(&format!("sub{}", ribbon.icon_key))
                                        } else {
                                            wows_data.ribbon_icons.get(&ribbon.icon_key)
                                        };

                                        let size = if ribbon.is_subribbon { (32.0, 32.0) } else { (64.0, 64.0) };

                                        if let Some(icon) = icon {
                                            let image = Image::new(ImageSource::Bytes {
                                                uri: icon.path.clone().into(),
                                                bytes: icon.data.clone().into(),
                                            })
                                            .fit_to_exact_size(size.into());
                                            ui.add(image).on_hover_text(&ribbon.description);
                                        } else {
                                            tracing::warn!(
                                                "Failed to resolve ribbon icon: {} (is_subribbon: {})",
                                                ribbon.icon_key,
                                                ribbon.is_subribbon,
                                            );
                                        }

                                        ui.label(format!("{} ({}x)", &ribbon.display_name, ribbon.count))
                                            .on_hover_text(&ribbon.description);
                                    });
                                }
                            }
                        });
                    }
                    ReplayColumn::ActualDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label("NDA");
                        } else if report.actual_damage_hover_text().is_some() || report.damage_interactions.is_some() {
                            self.dealt_damage_details(report, ui);
                        }
                    }
                    ReplayColumn::PotentialDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label("NDA");
                        } else if let Some(damage_extended_info) = report.potential_damage_hover_text.clone() {
                            ui.label(damage_extended_info);
                        }
                    }
                    ReplayColumn::ReceivedDamage => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label("NDA");
                        } else if report.received_damage_hover_text.is_some() || report.damage_interactions.is_some() {
                            self.received_damage_details(report, ui);
                        }
                    }
                    ReplayColumn::Skills => {
                        if !report.relation().is_enemy() || self.debug_mode {
                            ui.vertical(|ui| {
                                if let Some(hover_text) = &report.skill_info.hover_text {
                                    ui.label(hover_text);
                                }
                                if let Some(build_info) = &report.translated_build {
                                    ui.separator();

                                    if build_info.modules.is_empty() {
                                        ui.label("No Modules");
                                    } else {
                                        ui.label("Modules:");
                                        for module in &build_info.modules {
                                            if let Some(name) = &module.name {
                                                let label = ui.label(name);
                                                if let Some(hover_text) = module.description.as_ref() {
                                                    label.on_hover_text(hover_text);
                                                }
                                            }
                                        }
                                    }

                                    ui.separator();

                                    if build_info.abilities.is_empty() {
                                        ui.label("No Abilities");
                                    } else {
                                        ui.label("Abilities:");
                                        for ability in &build_info.abilities {
                                            if let Some(name) = &ability.name {
                                                ui.label(name);
                                            }
                                        }
                                    }

                                    ui.separator();

                                    if let Some(captain_skills) = build_info.captain_skills.as_ref() {
                                        ui.label("Captain Skills:");
                                        if captain_skills.is_empty() {
                                            ui.label("No Captain Skills");
                                        } else {
                                            for skill in captain_skills {
                                                if let Some(name) = &skill.name {
                                                    let label = ui.label(format!("({}) {}", skill.tier, name));
                                                    if let Some(hover_text) = skill.description.as_ref() {
                                                        label.on_hover_text(hover_text);
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        ui.label("No Captain Skills");
                                    }
                                }
                            });
                        }
                    }
                    ReplayColumn::Hits => {
                        if report.should_hide_stats() && !self.debug_mode {
                            ui.label("NDA");
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

    /// Populate Personal Rating for all players using the provided PR data
    pub fn populate_personal_ratings(&mut self, pr_data: &crate::personal_rating::PersonalRatingData) {
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

            let stats = crate::personal_rating::ShipBattleStats {
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
                    ui.label("Actions");
                }
                ReplayColumn::Name => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Player Name",
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
                            "Base XP",
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
                            "Raw XP",
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
                            "Ship Name",
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
                        .strong(column_name_with_sort_order("Hits", false, *self.replay_sort.lock(), SortColumn::Hits))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Hits);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Kills => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Kills",
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
                            "Observed Damage",
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
                            "Actual Damage",
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
                            "Spotting Damage",
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
                            "Potential Damage",
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
                    ui.strong("Time Lived");
                }
                ReplayColumn::Fires => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Fires",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Fires,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Fires);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Floods => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Floods",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Floods,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Floods);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Citadels => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Citadels",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Citadels,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Citadels);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::Crits => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Crits",
                            false,
                            *self.replay_sort.lock(),
                            SortColumn::Crits,
                        ))
                        .clicked()
                    {
                        let new_sort = self.replay_sort.lock().update_column(SortColumn::Crits);

                        self.sort_players(new_sort);
                    };
                }
                ReplayColumn::ReceivedDamage => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "Received Damage",
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
                            "Distance Traveled",
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
                    ui.strong("Skills");
                }
                ReplayColumn::PersonalRating => {
                    if ui
                        .strong(column_name_with_sort_order(
                            "PR",
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
                let how_expanded = ctx.animate_bool(Id::new(expanded_row_nr), *expanded);
                how_expanded * self.row_heights.get(expanded_row_nr).copied().unwrap()
            })
            .sum::<f32>()
            + row_nr as f32 * ROW_HEIGHT
    }
}

const ROW_HEIGHT: f32 = 28.0;

pub struct Replay {
    pub replay_file: ReplayFile,

    pub resource_loader: Arc<GameMetadataProvider>,

    pub battle_report: Option<BattleReport>,
    pub ui_report: Option<UiReport>,

    pub game_constants: Option<Arc<wows_replays::game_constants::GameConstants>>,

    /// Original file path this replay was loaded from, if available.
    pub source_path: Option<PathBuf>,
}

fn clan_color_for_player(player: &Player) -> Option<Color32> {
    let state = player.initial_state();
    if state.clan().is_empty() {
        None
    } else {
        let clan_color = state.raw_with_names().get("clanColor").expect("no clan color?");
        let clan_color = clan_color.as_i64().expect("clan color is not an i64");
        Some(Color32::from_rgb(
            ((clan_color & 0xFF0000) >> 16) as u8,
            ((clan_color & 0xFF00) >> 8) as u8,
            (clan_color & 0xFF) as u8,
        ))
    }
}

impl Replay {
    pub fn new(replay_file: ReplayFile, resource_loader: Arc<GameMetadataProvider>) -> Self {
        Replay {
            replay_file,
            resource_loader,
            battle_report: None,
            ui_report: None,
            game_constants: None,
            source_path: None,
        }
    }

    pub fn player_vehicle(&self) -> Option<&VehicleInfoMeta> {
        let meta = &self.replay_file.meta;
        meta.vehicles.iter().find(|vehicle| vehicle.relation == 0)
    }

    pub fn vehicle_name(&self, metadata_provider: &GameMetadataProvider) -> String {
        self.player_vehicle()
            .and_then(|vehicle| metadata_provider.param_localization_id(vehicle.shipId.raw().into()))
            .and_then(|id| metadata_provider.localized_name_from_id(id))
            .unwrap_or_else(|| "Spectator".to_string())
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
            &self.replay_file.meta.gameType.to_string(),
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

    pub fn label(&self, metadata_provider: &GameMetadataProvider) -> String {
        [
            self.vehicle_name(metadata_provider).as_str(),
            self.map_name(metadata_provider).as_str(),
            self.scenario(metadata_provider).as_str(),
            self.game_mode(metadata_provider).as_str(),
            self.game_time(),
        ]
        .iter()
        .join("\n")
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

        // Parse packets one at a time
        let packet_data = &self.replay_file.packet_data;
        let mut controller = BattleController::new(
            &self.replay_file.meta,
            self.resource_loader.as_ref(),
            self.game_constants.as_deref(),
        );
        let mut p = wows_replays::packet2::Parser::new(self.resource_loader.entity_specs());

        let mut remaining = &packet_data[..];
        while !remaining.is_empty() {
            match p.parse_packet(&mut remaining) {
                Ok(packet) => {
                    controller.process(&packet);
                }
                Err(e) => {
                    debug!("Packet parse error: {:?}", e);
                    break;
                }
            }
        }
        controller.finish();

        Ok(controller.build_report())
    }

    pub fn build_ui_report(&mut self, deps: &crate::wows_data::ReplayDependencies) {
        if let Some(battle_report) = &self.battle_report {
            let replay_version =
                wowsunpack::data::Version::from_client_exe(&self.replay_file.meta.clientVersionFromExe);

            // Resolve version-matched data so the UI report uses the correct constants
            let Some(wows_data) = deps.resolve_versioned_deps(&replay_version) else {
                tracing::warn!("Could not resolve versioned data for build {}", replay_version.build);
                return;
            };

            self.ui_report = Some(UiReport::new(&self.replay_file, battle_report, &wows_data, deps));
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
    pub fn to_battle_stats(&self) -> Option<crate::personal_rating::ShipBattleStats> {
        let vehicle = self.player_vehicle()?;
        let battle_result = self.battle_result()?;
        let ui_report = self.ui_report.as_ref()?;
        let self_report = ui_report.player_reports().iter().find(|report| report.relation().is_self())?;

        let is_win = matches!(battle_result, BattleResult::Win(_));

        Some(crate::personal_rating::ShipBattleStats {
            ship_id: vehicle.shipId,
            battles: 1,
            damage: self_report.actual_damage().unwrap_or_default(),
            wins: if is_win { 1 } else { 0 },
            frags: self_report.kills().unwrap_or_default(),
        })
    }
}

fn column_name_with_sort_order(
    text: &'static str,
    has_info: bool,
    sort_order: SortOrder,
    column: SortColumn,
) -> Cow<'static, str> {
    if sort_order.column() == column {
        let text_with_icon = if has_info {
            format!("{} {} {}", text, icons::INFO, sort_order.icon())
        } else {
            format!("{} {}", text, sort_order.icon())
        };
        Cow::Owned(text_with_icon)
    } else if has_info {
        Cow::Owned(format!("{} {}", text, icons::INFO))
    } else {
        Cow::Borrowed(text)
    }
}

fn copy_files_to_clipboard(paths: &[std::path::PathBuf]) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.set().file_list(paths);
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

        ui_report.update_visible_columns(&self.tab_state.settings.replay_settings);

        let mut columns =
            vec![egui_table::Column::new(100.0).range(10.0..=500.0).resizable(true); ui_report.columns.len()];
        let action_label_layout = ui.painter().layout_no_wrap(
            "Actions".to_string(),
            egui::FontId::default(),
            ui.style().visuals.text_color(),
        );
        let action_label_width = action_label_layout.rect.width() + 4.0;
        columns[ReplayColumn::Actions as usize] = egui_table::Column::new(action_label_width).resizable(false);

        let table = egui_table::Table::new()
            .id_salt("replay_player_list")
            .num_rows(ui_report.player_reports.len() as u64)
            .columns(columns)
            .num_sticky_cols(3)
            .headers([egui_table::HeaderRow { height: 14.0f32, groups: Default::default() }])
            .auto_size_mode(egui_table::AutoSizeMode::Never);
        table.show(ui, ui_report);
    }

    fn build_replay_view(&self, replay_file: &mut Replay, ui: &mut egui::Ui, metadata_provider: &GameMetadataProvider) {
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
                    let text = RichText::new(icon_str!(icons::INFO, "Incomplete Match Results")).color(Color32::ORANGE);
                    let hover_text = "The replay does not yet have end-of-match results. Data will be automatically re-loaded when the match ends and end-of-match results are added to the replay.";
                    ui.strong(text).on_hover_text(hover_text);
                }

                if let Some(battle_result) = replay_file.battle_result() {
                    let text = match battle_result {
                        BattleResult::Win(_) => RichText::new(icon_str!(icons::TROPHY, "Victory")).color(Color32::LIGHT_GREEN),
                        BattleResult::Loss(_) => RichText::new(icon_str!(icons::SMILEY_SAD, "Defeat")).color(Color32::LIGHT_RED),
                        BattleResult::Draw => RichText::new(icon_str!(icons::NOTCHES, "Draw")).color(Color32::LIGHT_YELLOW),
                    };
                    ui.label(text);
                }

                if let Some(battle_stats) = replay_file.to_battle_stats() {
                    let pr_data = self.tab_state.personal_rating_data.read();
                    if let Some(pr_result) = pr_data.calculate_pr(&[battle_stats]) {
                        ui.label(RichText::new(format!("PR: {:.0} ({})", pr_result.pr, pr_result.category.name())).color(pr_result.category.color()));
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

                ui.menu_button("Export", |ui| {
                    ui.label(RichText::new("Chat").strong());
                    if ui.small_button(icon_str!(icons::FLOPPY_DISK, "Save To File")).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!("{} {} {} - Game Chat.txt", report.game_type(), report.game_mode(), report.map_name()))
                            .save_file()
                            && let Ok(mut file) = std::fs::File::create(path)
                        {
                            for message in report.game_chat() {
                                let GameMessage { sender_relation: _, sender_name, channel, message, entity_id: _, player, clock: _ } = message;
                                match player {
                                    Some(player) if !player.initial_state().clan().is_empty() => {
                                        let _ = writeln!(file, "[{}] {} ({:?}): {}", player.initial_state().clan(), sender_name, channel, message);
                                    }
                                    _ => {
                                        let _ = writeln!(file, "{sender_name} ({channel:?}): {message}");
                                    }
                                }
                            }
                        }
                        ui.close_kind(UiKind::Menu);
                    }
                    if ui.small_button(icon_str!(icons::COPY, "Copy")).clicked() {
                        let mut buf = BufWriter::new(Vec::new());
                        for message in report.game_chat() {
                            let GameMessage { sender_relation: _, sender_name, channel, message, entity_id: _, player, clock: _ } = message;
                            match player {
                                Some(player) if !player.initial_state().clan().is_empty() => {
                                    let _ = writeln!(buf, "[{}] {} ({:?}): {}", player.initial_state().clan(), sender_name, channel, message);
                                }
                                _ => {
                                    let _ = writeln!(buf, "{sender_name} ({channel:?}): {message}");
                                }
                            }
                        }
                        let game_chat = String::from_utf8(buf.into_inner().expect("failed to get buf inner")).expect("failed to convert game chat buffer to string");
                        ui.ctx().copy_text(game_chat);
                        ui.close_kind(UiKind::Menu);
                    }

                    ui.separator();
                    ui.label(RichText::new("Results").strong());
                    let format = if ui.button("JSON").clicked() {
                        Some(ReplayExportFormat::Json)
                    } else if ui.button("CBOR").clicked() {
                        Some(ReplayExportFormat::Cbor)
                    } else if ui.button("CSV").clicked() {
                        Some(ReplayExportFormat::Csv)
                    } else {
                        None
                    };
                    if let Some(format) = format
                        && let Some(path) =
                            rfd::FileDialog::new().set_file_name(format!("{}.{}", replay_file.better_file_name(metadata_provider), format.extension())).save_file()
                        && let Ok(mut file) = std::fs::File::create(path)
                    {
                        let transformed_results = Match::new(replay_file, self.tab_state.settings.debug_mode);
                        let result = match format {
                            ReplayExportFormat::Json => serde_json::to_writer(&mut file, &transformed_results).map_err(|e| Box::new(e) as Box<dyn std::error::Error>),
                            ReplayExportFormat::Cbor => serde_cbor::to_writer(&mut file, &transformed_results).map_err(|e| Box::new(e) as Box<dyn std::error::Error>),
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
                });

                {
                    let has_chat = !report.game_chat().is_empty();
                    let show_chat: bool = ui.ctx().data(|d| d.get_temp(egui::Id::new("show_game_chat"))).unwrap_or(false);
                    let response = ui.add_enabled(has_chat, egui::Button::new(icon_str!(icons::CHAT_TEXT, "Chat")).selected(show_chat));
                    if !has_chat {
                        response.on_disabled_hover_text("No chat messages were sent in this replay");
                    } else if response.clicked() {
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(egui::Id::new("show_game_chat"), !show_chat);
                        });
                    }
                }

                if self.tab_state.settings.debug_mode && ui.button("Raw Metadata").clicked() {
                    let parsed_meta: serde_json::Value = serde_json::from_str(&replay_file.replay_file.raw_meta).expect("failed to parse replay metadata");
                    let pretty_meta = serde_json::to_string_pretty(&parsed_meta).expect("failed to serialize replay metadata");
                    let viewer = plaintext_viewer::PlaintextFileViewer {
                        title: Arc::new("metadata.json".to_owned()),
                        file_info: Arc::new(Mutex::new(FileType::PlainTextFile { ext: ".json".to_owned(), contents: pretty_meta })),
                        open: Arc::new(AtomicBool::new(true)),
                    };
                    self.tab_state.file_viewer.lock().push(viewer);
                }
                if self.tab_state.settings.debug_mode {
                    let has_results = report.battle_results().is_some();
                    ui.add_enabled_ui(has_results, |ui| {
                        ui.menu_button("View Results", |ui| {
                            if ui.button("Raw JSON").on_hover_text("The raw battle results as serialized by WG.").clicked() {
                                if let Some(results_json) = report.battle_results() {
                                    let parsed_results: serde_json::Value = serde_json::from_str(results_json).expect("failed to parse battle results");
                                    let pretty = serde_json::to_string_pretty(&parsed_results).expect("failed to serialize battle results");
                                    let viewer = plaintext_viewer::PlaintextFileViewer {
                                        title: Arc::new("results_raw.json".to_owned()),
                                        file_info: Arc::new(Mutex::new(FileType::PlainTextFile { ext: ".json".to_owned(), contents: pretty })),
                                        open: Arc::new(AtomicBool::new(true)),
                                    };
                                    self.tab_state.file_viewer.lock().push(viewer);
                                }
                                ui.close_kind(UiKind::Menu);
                            }
                            if ui.button("Mapped JSON").on_hover_text("Battle results with positional arrays resolved to named fields.").clicked() {
                                if let Some(resolved) = replay_file.ui_report.as_ref().and_then(|r| r.resolved_results.as_ref()) {
                                    let pretty = serde_json::to_string_pretty(resolved).expect("failed to serialize resolved results");
                                    let viewer = plaintext_viewer::PlaintextFileViewer {
                                        title: Arc::new("results_mapped.json".to_owned()),
                                        file_info: Arc::new(Mutex::new(FileType::PlainTextFile { ext: ".json".to_owned(), contents: pretty })),
                                        open: Arc::new(AtomicBool::new(true)),
                                    };
                                    self.tab_state.file_viewer.lock().push(viewer);
                                }
                                ui.close_kind(UiKind::Menu);
                            }
                        });
                    });
                }

                if !self.tab_state.settings.wows_dir.is_empty()
                    && replay_file.source_path.is_some()
                {
                    let alt_held = ui.input(|i| i.modifiers.alt);
                    let label = if alt_held {
                        icon_str!(icons::KEYBOARD, "Show Replay Controls")
                    } else {
                        icon_str!(icons::GAME_CONTROLLER, "Open in Game")
                    };
                    if ui.button(label).clicked() {
                        if alt_held {
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(egui::Id::new("open_replay_controls_window"), true);
                            });
                        } else {
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(
                                    egui::Id::new("pending_confirmation_request"),
                                    Some(crate::tab_state::ConfirmableAction::OpenInGame {
                                        replay_path: replay_file.source_path.clone().unwrap(),
                                    }),
                                );
                            });
                        }
                    }
                }

                if self.tab_state.wows_data_map.is_some()
                    && ui.button(icon_str!(icons::PLAY, "Render")).clicked()
                {
                    let raw_meta = replay_file.replay_file.raw_meta.clone().into_bytes();
                    let pkt_data = replay_file.replay_file.packet_data.clone();
                    let map_name = replay_file.replay_file.meta.mapName.clone();
                    let translated_map = replay_file.map_name(metadata_provider);
                    let base = format!("{} - {}", replay_file.replay_file.meta.playerName, translated_map);
                    let replay_name = if let Some(stem) = replay_file.source_path.as_ref()
                        .and_then(|p: &PathBuf| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    {
                        format!("{} - {}", base, stem)
                    } else {
                        base
                    };
                    let game_duration = replay_file.replay_file.meta.duration as f32;
                    let replay_version =
                        wowsunpack::data::Version::from_client_exe(&replay_file.replay_file.meta.clientVersionFromExe);
                    let Some(wows_data) = self
                        .tab_state
                        .wows_data_map
                        .as_ref()
                        .and_then(|map| map.resolve(&replay_version))
                    else {
                        tracing::warn!("No data for build {}", replay_version.build);
                        return;
                    };
                    let asset_cache = self.tab_state.renderer_asset_cache.clone();
                    let viewer = crate::replay_renderer::launch_replay_renderer(
                        raw_meta,
                        pkt_data,
                        map_name,
                        replay_name,
                        game_duration,
                        wows_data,
                        asset_cache,
                        &self.tab_state.settings.renderer_options,
                        Arc::clone(&self.tab_state.suppress_gpu_encoder_warning),
                    );
                    self.tab_state.replay_renderers.lock().push(viewer);
                }

                if let Some(self_report) = self_report
                    && self_report.is_test_ship()
                    && ui.checkbox(&mut hide_my_stats, "Hide My Test Ship Stats").changed()
                {
                    hide_my_stats_changed = true;
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
                    let locale = self.tab_state.settings.locale.as_ref().map(|s| s.as_ref());
                    let mut job = LayoutJob::default();
                    let weak_fmt = TextFormat { color: weak, ..Default::default() };
                    job.append("Team Damage: ", 0.0, weak_fmt.clone());
                    job.append(
                        &separate_number(team_damage, locale),
                        0.0,
                        TextFormat { color: Color32::LIGHT_GREEN, ..Default::default() },
                    );
                    job.append(" : ", 0.0, weak_fmt.clone());
                    job.append(
                        &separate_number(red_team_damage, locale),
                        0.0,
                        TextFormat { color: Color32::LIGHT_RED, ..Default::default() },
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

            egui::CentralPanel::default().show_inside(ui, |ui| {
                egui::ScrollArea::horizontal().id_salt("replay_player_list_scroll_area").show(ui, |ui| {
                    if let Some(ui_report) = replay_file.ui_report.as_mut() {
                        ui_report.debug_mode = self.tab_state.settings.debug_mode;
                        self.build_replay_player_list(ui_report, ui);
                    }
                });
            });
        }
    }

    fn build_file_listing(&mut self, ui: &mut egui::Ui) {
        let grouping = self.tab_state.settings.replay_settings.grouping;

        match grouping {
            ReplayGrouping::None => self.build_file_listing_ungrouped(ui),
            ReplayGrouping::Date | ReplayGrouping::Ship => self.build_file_listing_grouped(ui, grouping),
        }
    }

    fn build_file_listing_ungrouped(&mut self, ui: &mut egui::Ui) {
        let mut replay_to_open: Option<Arc<RwLock<Replay>>> = None;
        let mut replay_to_open_new: Option<Arc<RwLock<Replay>>> = None;

        ui.vertical(|ui| {
            egui::Grid::new("replay_files_grid").num_columns(1).striped(true).show(ui, |ui| {
                if let Some(mut files) = self
                    .tab_state
                    .replay_files
                    .as_ref()
                    .map(|files| files.iter().map(|(x, y)| (x.clone(), y.clone())).collect::<Vec<_>>())
                {
                    files.sort_by(|a, b| b.0.cmp(&a.0));
                    let metadata_provider = self.metadata_provider().unwrap();
                    let focused = self.tab_state.focused_replay();
                    for (path, replay) in files {
                        let replay_guard = replay.read();
                        let label = replay_guard.label(&metadata_provider);
                        let battle_result = replay_guard.battle_result();
                        drop(replay_guard);

                        let is_selected = focused.as_ref().map(|c| Arc::ptr_eq(c, &replay)).unwrap_or(false);
                        let label_text = colorize_label(&label, battle_result, is_selected);

                        let replay_weak = Arc::downgrade(&replay);
                        let path_clone = path.clone();
                        let wows_dir = self.tab_state.settings.wows_dir.clone();
                        let label_response = ui
                            .add(Label::new(label_text).selectable(false).sense(Sense::click()))
                            .on_hover_text(label.as_str());
                        label_response.context_menu(|ui| {
                            show_leaf_context_menu(ui, &replay_weak, &path_clone, &wows_dir);
                        });

                        if label_response.double_clicked() {
                            replay_to_open = Some(replay.clone());
                        }
                        ui.end_row();
                    }
                }
            });
        });

        self.handle_context_menu_render(ui);
        self.handle_replay_open_actions(ui, &mut replay_to_open, &mut replay_to_open_new);
    }

    fn build_file_listing_grouped(&mut self, ui: &mut egui::Ui, grouping: ReplayGrouping) {
        let Some(mut files) = self
            .tab_state
            .replay_files
            .as_ref()
            .map(|files| files.iter().map(|(x, y)| (x.clone(), y.clone())).collect::<Vec<_>>())
        else {
            return;
        };

        files.sort_by(|a, b| b.0.cmp(&a.0));
        let metadata_provider = self.metadata_provider().unwrap();

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
            let group_id = egui::Id::new((group_id_salt, group_name));
            let mut grp_replays = Vec::new();
            let mut child_ids = Vec::new();
            let mut grp_paths = Vec::new();
            for (path, replay) in replays {
                let id = egui::Id::new(path);
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

        let tree = egui_ltreeview::TreeView::new(ui.make_persistent_id(tree_id_salt))
            .allow_multi_selection(true)
            .fallback_context_menu(move |ui, selected_ids| {
                fallback_maps.show_multi_selection_context_menu(ui, selected_ids);
            });

        let (_response, actions) = tree.show(ui, |builder| {
            for (group_name, replays) in &groups {
                let win_rate = win_rate_label(replays);
                let group_id = egui::Id::new((group_id_salt, group_name));
                let group_replays = tree_maps.group_replays.get(&group_id).cloned().unwrap_or_default();
                let group_paths = tree_maps.group_paths.get(&group_id).cloned().unwrap_or_default();
                let dir_node = egui_ltreeview::NodeBuilder::dir(group_id)
                    .label(format!("{} ({}){}", group_name, replays.len(), win_rate))
                    .context_menu(move |ui| {
                        show_group_context_menu(ui, &group_paths, &group_replays);
                    });
                let is_open = builder.node(dir_node);
                if is_open {
                    for (path, _replay) in replays {
                        let id = egui::Id::new(path);
                        let path_clone = path.clone();
                        let wows_dir = self.tab_state.settings.wows_dir.clone();
                        let replay_weak = tree_maps.leaf_replays.get(&id).cloned().unwrap();

                        let replay_guard = id_to_replay.get(&id).unwrap().read();
                        let battle_result = replay_guard.battle_result();
                        let label = match grouping {
                            ReplayGrouping::Date => {
                                let ship_name = replay_guard.vehicle_name(&metadata_provider);
                                let map_name = replay_guard.map_name(&metadata_provider);
                                let game_time = replay_guard.game_time().to_string();
                                let time_part = game_time.split(' ').nth(1).unwrap_or(&game_time).to_string();
                                format!("{} - {} ({})", ship_name, map_name, time_part)
                            }
                            ReplayGrouping::Ship => {
                                let map_name = replay_guard.map_name(&metadata_provider);
                                let game_time = replay_guard.game_time().to_string();
                                format!("{} - {}", map_name, game_time)
                            }
                            ReplayGrouping::None => unreachable!(),
                        };
                        drop(replay_guard);

                        let label_text = colorize_label(&label, battle_result, false);
                        let node = egui_ltreeview::NodeBuilder::leaf(id).label(label_text).context_menu(move |ui| {
                            show_leaf_context_menu(ui, &replay_weak, &path_clone, &wows_dir);
                        });
                        builder.node(node);
                    }
                }
                builder.close_dir();
            }
        });

        self.handle_context_menu_render(ui);

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
                        let tree_id = ui.make_persistent_id(tree_id_salt);
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

        self.handle_replay_open_actions(ui, &mut replay_to_open, &mut replay_to_open_new);
    }

    /// Check for "Open in New Tab" from context menu, then open replays in the appropriate tab.
    fn handle_replay_open_actions(
        &mut self,
        ui: &mut egui::Ui,
        replay_to_open: &mut Option<Arc<RwLock<Replay>>>,
        replay_to_open_new: &mut Option<Arc<RwLock<Replay>>>,
    ) {
        if let Some(replay) = ui
            .ctx()
            .data_mut(|data| data.remove_temp::<Weak<RwLock<Replay>>>(egui::Id::new("open_replay_new_tab")))
            .and_then(|w| w.upgrade())
        {
            *replay_to_open_new = Some(replay);
        }

        if let Some(replay) = replay_to_open_new.take() {
            self.tab_state.open_replay_in_new_tab(replay.clone());
            if let Some(deps) = self.tab_state.replay_dependencies() {
                update_background_task!(
                    self.tab_state.background_tasks,
                    deps.load_replay(replay, ReplaySource::FileListing)
                );
            }
        } else if let Some(replay) = replay_to_open.take() {
            self.tab_state.open_replay_in_focused_tab(replay.clone());
            if let Some(deps) = self.tab_state.replay_dependencies() {
                update_background_task!(
                    self.tab_state.background_tasks,
                    deps.load_replay(replay, ReplaySource::FileListing)
                );
            }
        }
    }

    fn build_replay_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(icon_str!(icons::FOLDER_OPEN, "Manually Open Replay File...")).clicked()
                && let Some(file) = rfd::FileDialog::new().add_filter("WoWs Replays", &["wowsreplay"]).pick_file()
            {
                self.tab_state.settings.current_replay_path = file;

                if let Some(deps) = self.tab_state.replay_dependencies() {
                    update_background_task!(
                        self.tab_state.background_tasks,
                        deps.parse_replay_from_path(
                            self.tab_state.settings.current_replay_path.clone(),
                            ReplaySource::ManualOpen
                        )
                    );
                }
            }

            ui.checkbox(&mut self.tab_state.auto_load_latest_replay, "Autoload Latest Replay");

            ComboBox::from_id_salt("replay_grouping")
                .selected_text(format!("Group: {}", self.tab_state.settings.replay_settings.grouping.label()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.tab_state.settings.replay_settings.grouping,
                        ReplayGrouping::Date,
                        "Date",
                    );
                    ui.selectable_value(
                        &mut self.tab_state.settings.replay_settings.grouping,
                        ReplayGrouping::Ship,
                        "Ship",
                    );
                    ui.selectable_value(
                        &mut self.tab_state.settings.replay_settings.grouping,
                        ReplayGrouping::None,
                        "None",
                    );
                });

            ComboBox::from_id_salt("column_filters")
                .selected_text("Column Filters")
                .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                .show_ui(ui, |ui| {
                    ui.checkbox(&mut self.tab_state.settings.replay_settings.show_raw_xp, "Raw XP");
                    ui.checkbox(&mut self.tab_state.settings.replay_settings.show_entity_id, "Entity ID");
                    ui.checkbox(&mut self.tab_state.settings.replay_settings.show_observed_damage, "Observed Damage");
                    ui.checkbox(&mut self.tab_state.settings.replay_settings.show_fires, "Fires");
                    ui.checkbox(&mut self.tab_state.settings.replay_settings.show_floods, "Floods");
                    ui.checkbox(&mut self.tab_state.settings.replay_settings.show_citadels, "Citadels");
                    ui.checkbox(&mut self.tab_state.settings.replay_settings.show_crits, "Critical Module Hits");
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
                    egui::Button::new(icon_str!(icons::MAP_TRIFOLD, "Tactics Board")),
                );
                let btn = if !has_data {
                    btn.on_hover_text("Waiting for game data to load\u{2026}")
                } else if at_limit {
                    btn.on_hover_text(format!(
                        "Maximum {} tactics boards open",
                        crate::collab::protocol::MAX_TACTICS_BOARDS
                    ))
                } else {
                    btn
                };
                if btn.clicked() {
                    let session_handle =
                        self.tab_state.host_session.as_ref().or(self.tab_state.client_session.as_ref());
                    let owner_user_id =
                        session_handle.map(|_| self.tab_state.session_state.lock().my_user_id).unwrap_or(0);
                    let mut board = crate::minimap_view::tactics::TacticsBoardViewer::new(
                        rand::random(),
                        owner_user_id,
                        std::sync::Arc::clone(&self.tab_state.cap_layout_db),
                        std::sync::Arc::clone(&self.tab_state.renderer_asset_cache),
                        std::sync::Arc::clone(self.tab_state.world_of_warships_data.as_ref().unwrap()),
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
            RichText::new(icon_str!(icons::BROADCAST, "Session")).color(Color32::WHITE)
        } else {
            RichText::new(icon_str!(icons::BROADCAST, "Session"))
        };
        let mut button = egui::Button::new(label);
        if any_active {
            button = button.fill(Color32::from_rgb(220, 50, 50));
        }
        let btn = ui.add(button);

        egui::Popup::from_toggle_button_response(&btn).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(
            |ui| {
                ui.set_min_width(260.0);

                if host_active && matches!(host_status, Some(SessionStatus::Starting)) {
                    // ── Host session is starting ──
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Starting session\u{2026}");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Cancel").clicked() {
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
                        ui.label(RichText::new("Session Active").strong().color(Color32::from_rgb(220, 50, 50)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Stop").clicked() {
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
                        ui.label("Session Token:");
                        let visible = self.tab_state.session_token_visible;
                        ui.horizontal(|ui| {
                            let mut display_token = token.clone();
                            let te = egui::TextEdit::singleline(&mut display_token)
                                .password(!visible)
                                .interactive(false)
                                .desired_width(160.0);
                            ui.add(te);

                            let eye_icon = if visible { icons::EYE } else { icons::EYE_SLASH };
                            if ui.button(eye_icon).on_hover_text("Toggle token visibility").clicked() {
                                self.tab_state.session_token_visible = !visible;
                            }

                            if ui.button(icons::COPY).on_hover_text("Copy token").clicked() {
                                ui.ctx().copy_text(token.clone());
                                self.tab_state.toasts.lock().info("Token copied to clipboard");
                            }
                        });

                        // Copy web link buttons
                        if ui
                            .button(icon_str!(icons::BROWSER, "Copy Web Link"))
                            .on_hover_text("Copy link for the web client")
                            .clicked()
                        {
                            let url = format!("{}#{}", crate::collab::WEB_CLIENT_URL, token);
                            ui.ctx().copy_text(url);
                            self.tab_state.toasts.lock().info("Web link copied to clipboard");
                        }
                        #[cfg(debug_assertions)]
                        if ui
                            .button(icon_str!(icons::BROWSER, "Copy Localhost Link"))
                            .on_hover_text("Copy localhost link (dev)")
                            .clicked()
                        {
                            let url = format!("http://localhost:8080/#{}", token);
                            ui.ctx().copy_text(url);
                            self.tab_state.toasts.lock().info("Localhost link copied to clipboard");
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
                        ui.label(format!("{} connected", peer_count));
                    });
                    // Show each connected user with color dot, name, and role
                    for user in &connected_users {
                        if user.id == my_id {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            let color = Color32::from_rgb(user.color[0], user.color[1], user.color[2]);
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 4.0, color);
                            ui.label(&user.name);
                            if user.role == crate::collab::PeerRole::CoHost {
                                ui.label(RichText::new(icons::CROWN).small().color(Color32::from_rgb(255, 195, 0)));
                            }
                            if user.role != crate::collab::PeerRole::Host
                                && user.role != crate::collab::PeerRole::CoHost
                                && ui.small_button(icons::CROWN).on_hover_text("Promote to co-host").clicked()
                                && let Some(ref handle) = self.tab_state.host_session
                            {
                                let _ = handle.command_tx.send(SessionCommand::PromoteToCoHost { user_id: user.id });
                                self.tab_state.toasts.lock().info(format!("Promoted {} to co-host", user.name));
                            }
                        });
                    }

                    ui.add_space(4.0);
                    ui.separator();

                    // Permission controls
                    ui.label(RichText::new("Permissions").small().strong());
                    let (mut lock_ann, mut lock_settings) = {
                        let s = self.tab_state.session_state.lock();
                        (s.permissions.annotations_locked, s.permissions.settings_locked)
                    };

                    let mut perms_changed = false;
                    perms_changed |= ui.checkbox(&mut lock_ann, "Lock Annotations").changed();
                    perms_changed |= ui.checkbox(&mut lock_settings, "Lock Settings").changed();

                    if perms_changed {
                        let perms = Permissions { annotations_locked: lock_ann, settings_locked: lock_settings };
                        self.tab_state.session_state.lock().permissions = perms.clone();
                        if let Some(ref handle) = self.tab_state.host_session {
                            let _ = handle.command_tx.send(SessionCommand::SetPermissions(perms));
                        }
                    }

                    ui.add_space(4.0);
                    if ui
                        .button("Reset Client Overrides")
                        .on_hover_text("Reset all client display setting changes")
                        .clicked()
                        && let Some(ref handle) = self.tab_state.host_session
                    {
                        let _ = handle.command_tx.send(SessionCommand::ResetClientOverrides);
                    }
                } else if client_active {
                    // ── Active client session ──
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Connected to Session").strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Leave").clicked() {
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
                            let color = Color32::from_rgb(user.color[0], user.color[1], user.color[2]);
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                            ui.painter().circle_filled(rect.center(), 4.0, color);
                            if user.id == my_id {
                                ui.label(RichText::new(&user.name).italics());
                                ui.label(RichText::new("(you)").small().weak());
                            } else {
                                ui.label(&user.name);
                            }
                            match user.role {
                                crate::collab::PeerRole::Host => {
                                    ui.label(RichText::new(icons::CROWN).small().color(Color32::from_rgb(255, 195, 0)))
                                        .on_hover_text("Host");
                                }
                                crate::collab::PeerRole::CoHost => {
                                    ui.label(
                                        RichText::new(icons::CROWN).small().color(Color32::from_rgb(136, 84, 208)),
                                    )
                                    .on_hover_text("Co-host");
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
                            RichText::new("Please enter a display name").color(Color32::from_rgb(220, 50, 50)).small(),
                        );
                    }
                    ui.label("Display name:");
                    let name_response = ui.add(
                        egui::TextEdit::singleline(&mut self.tab_state.settings.collab_display_name)
                            .hint_text("Your name...")
                            .desired_width(160.0)
                            .text_color(if self.tab_state.show_display_name_error {
                                Color32::from_rgb(220, 50, 50)
                            } else {
                                ui.visuals().text_color()
                            }),
                    );
                    if self.tab_state.show_display_name_error {
                        ui.painter().rect_stroke(
                            name_response.rect,
                            name_response.rect.height() * 0.15,
                            egui::Stroke::new(1.5, Color32::from_rgb(220, 50, 50)),
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

                    ui.label(RichText::new("Host a Session").strong());
                    if ui.button("Start Session").clicked() {
                        if self.tab_state.settings.collab_display_name.trim().is_empty() {
                            self.tab_state.show_display_name_error = true;
                            self.tab_state.toasts.lock().error("Enter a display name first");
                        } else {
                            self.tab_state.pending_host = true;
                            if !self.tab_state.settings.suppress_p2p_ip_warning {
                                self.tab_state.show_ip_warning = true;
                            }
                        }
                    }

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // ── Join a session ──
                    ui.label(RichText::new("Join a Session").strong());
                    ui.add_space(2.0);

                    // Paste token → validate → auto-join
                    if ui.button(icon_str!(icons::CLIPBOARD, "Paste token & join")).clicked()
                        && let Ok(mut clipboard) = arboard::Clipboard::new()
                        && let Ok(text) = clipboard.get_text()
                    {
                        let trimmed = text.trim().to_string();
                        if trimmed.is_empty() {
                            self.tab_state.toasts.lock().error("Clipboard is empty");
                        } else if self.tab_state.settings.collab_display_name.trim().is_empty() {
                            self.tab_state.show_display_name_error = true;
                            self.tab_state.toasts.lock().error("Enter a display name first");
                        } else if let Err(e) = crate::collab::protocol::decode_token(&trimmed) {
                            self.tab_state.toasts.lock().error(format!("Invalid token: {e}"));
                        } else {
                            self.tab_state.join_session_token = trimmed;
                            self.tab_state.pending_join = true;
                            if !self.tab_state.settings.suppress_p2p_ip_warning {
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
        ui.label(RichText::new("Shared Windows").small().strong());

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
                    if ui.small_button("Open").clicked() {
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
                            let saved_options = &self.tab_state.settings.renderer_options;
                            let suppress = std::sync::Arc::clone(&self.tab_state.suppress_gpu_encoder_warning);
                            let viewer = crate::replay_renderer::launch_client_renderer(
                                replay.replay_name.clone(),
                                replay.map_image_png.clone(),
                                replay.game_version.clone(),
                                saved_options,
                                suppress,
                                self.tab_state.world_of_warships_data.as_ref(),
                                &self.tab_state.renderer_asset_cache,
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
                    if ui.small_button("Open").clicked()
                        && let Some(ref wows_data) = self.tab_state.world_of_warships_data
                    {
                        let board_count = self.tab_state.tactics_boards.lock().len();
                        if board_count < crate::collab::protocol::MAX_TACTICS_BOARDS {
                            let mut board = crate::minimap_view::tactics::TacticsBoardViewer::new(
                                *bid,
                                *owner_uid,
                                std::sync::Arc::clone(&self.tab_state.cap_layout_db),
                                std::sync::Arc::clone(&self.tab_state.renderer_asset_cache),
                                std::sync::Arc::clone(wows_data),
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
                    && ui.small_button("Open for everyone").clicked()
                {
                    let _ = handle
                        .command_tx
                        .send(crate::collab::SessionCommand::OpenWindowForEveryone { window_id: *bid });
                }
            });
        }
    }

    /// Builds the replay parser tab
    pub fn build_replay_parser_tab(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            self.build_replay_header(ui);

            {
                let panel_id = egui::Id::new("replay_listing_panel");

                // Auto-size the panel to the widest label when files are first populated.
                // Uses a flag on TabState (not egui temp data) to survive GC.
                let has_files = self.tab_state.replay_files.as_ref().is_some_and(|f| !f.is_empty());

                let mut default_width = 250.0f32;

                if has_files
                    && !self.tab_state.replay_listing_auto_sized
                    && let Some(metadata_provider) = self.metadata_provider()
                {
                    let font_id = egui::TextStyle::Body.resolve(ui.style());
                    let max_width = self
                        .tab_state
                        .replay_files
                        .as_ref()
                        .unwrap()
                        .values()
                        .map(|replay| {
                            let guard = replay.read();
                            let label = guard.label(&metadata_provider);
                            label
                                .lines()
                                .map(|line| {
                                    ui.painter()
                                        .layout_no_wrap(line.to_string(), font_id.clone(), Color32::WHITE)
                                        .size()
                                        .x
                                })
                                .fold(0.0f32, f32::max)
                        })
                        .fold(0.0f32, f32::max);

                    // Add padding for tree indentation, margins, scrollbar
                    default_width = (max_width + 60.0).max(200.0);

                    self.tab_state.replay_listing_auto_sized = true;

                    // Clear stored panel state so default_width takes effect
                    ui.ctx().data_mut(|d| {
                        d.remove::<egui::containers::panel::PanelState>(panel_id);
                    });
                }

                egui::SidePanel::left("replay_listing_panel")
                    .default_width(default_width)
                    .width_range(100.0..=f32::INFINITY)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::both().id_salt("replay_listing_scroll_area").show(ui, |ui| {
                            self.build_file_listing(ui);
                        });
                    });
            }

            egui::CentralPanel::default().show_inside(ui, |ui| {
                let has_tabs = self.tab_state.replay_dock_state.iter_all_tabs().next().is_some();
                if has_tabs {
                    let mut dock_state =
                        std::mem::replace(&mut self.tab_state.replay_dock_state, egui_dock::DockState::new(vec![]));
                    let mut viewer = ReplayTabViewer { tab_state: self.tab_state };
                    egui_dock::DockArea::new(&mut dock_state)
                        .id(egui::Id::new("replay_parser_dock"))
                        .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                        .show_close_buttons(true)
                        .show_leaf_collapse_buttons(false)
                        .show_leaf_close_all_buttons(false)
                        .allowed_splits(egui_dock::AllowedSplits::All)
                        .show_inside(ui, &mut viewer);
                    self.tab_state.replay_dock_state = dock_state;
                } else {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                        ui.heading("Click a replay to view, or double-click to open in a new tab");
                    });
                }
            });
        });

        self.show_game_chat_window(ui.ctx());
        self.pick_up_replay_controls_request(ui.ctx());
        self.show_replay_controls_window(ui.ctx());
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

        egui::Window::new(icon_str!(icons::CHAT_TEXT, "Game Chat"))
            .open(&mut open)
            .default_width(CHAT_VIEW_WIDTH)
            .default_height(400.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(icon_str!(icons::COPY, "Copy All")).clicked() {
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
                        toasts.lock().success("Chat copied");
                    }
                    if ui.button(icon_str!(icons::FLOPPY_DISK, "Save To File")).clicked()
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

    fn handle_context_menu_render(&mut self, ui: &mut egui::Ui) {
        let replay_weak: Option<Weak<RwLock<Replay>>> =
            ui.ctx().data_mut(|data| data.remove_temp(egui::Id::new("context_menu_render_replay")));
        if let Some(weak) = replay_weak
            && let Some(arc) = weak.upgrade()
            && self.tab_state.wows_data_map.is_some()
        {
            let guard = arc.read();
            let raw_meta = guard.replay_file.raw_meta.clone().into_bytes();
            let pkt_data = guard.replay_file.packet_data.clone();
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
                tracing::warn!("No data for build {}", replay_version.build);
                return;
            };
            let asset_cache = self.tab_state.renderer_asset_cache.clone();
            let viewer = crate::replay_renderer::launch_replay_renderer(
                raw_meta,
                pkt_data,
                map_name,
                replay_name,
                game_duration,
                wows_data,
                asset_cache,
                &self.tab_state.settings.renderer_options,
                Arc::clone(&self.tab_state.suppress_gpu_encoder_warning),
            );
            self.tab_state.replay_renderers.lock().push(viewer);
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
                            let groups = crate::controls::parse_commands_scheme(&buf);
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

        egui::Window::new("Replay Controls")
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
                                        ui.label(
                                            egui::RichText::new(binding)
                                                .monospace()
                                                .color(egui::Color32::from_rgb(180, 210, 255)),
                                        );
                                        ui.end_row();
                                    }
                                },
                            );
                            ui.separator();
                        }
                    });
                } else {
                    ui.label("Controls not available (commands.scheme.xml not found in game files).");
                }
            });
    }
}

struct ReplayTabViewer<'a> {
    tab_state: &'a mut crate::tab_state::TabState,
}

impl egui_dock::TabViewer for ReplayTabViewer<'_> {
    type Tab = ReplayTab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("replay_tab", tab.id))
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        let replay = tab.replay.read();
        let viewer = ToolkitTabViewer { tab_state: self.tab_state };
        if let Some(mp) = viewer.metadata_provider() {
            let ship = replay.vehicle_name(&mp);
            let map = replay.map_name(&mp);
            format!("{ship} - {map}").into()
        } else {
            "Loading...".into()
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        let viewer = ToolkitTabViewer { tab_state: self.tab_state };
        let metadata_provider = viewer.metadata_provider().expect("no metadata provider?");
        let mut replay = tab.replay.write();
        viewer.build_replay_view(&mut replay, ui, metadata_provider.as_ref());
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        true
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }
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
                let translated_user =
                    metadata_provider.and_then(|provider| provider.localized_name_from_id(sender_name).map(Cow::Owned));
                let translated_text =
                    metadata_provider.and_then(|provider| provider.localized_name_from_id(message).map(Cow::Owned));
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
            player_color_for_team_relation(*relation)
        } else {
            Color32::GRAY
        };

        let mut job = LayoutJob::default();
        if let Some(player) = player
            && !player.initial_state().clan().is_empty()
        {
            job.append(
                &format!("[{}] ", player.initial_state().clan()),
                0.0,
                TextFormat { color: clan_color_for_player(player).unwrap(), ..Default::default() },
            );
        }
        job.append(&format!("{sender_name}:\n"), 0.0, TextFormat { color: name_color, ..Default::default() });

        let text_color = match channel {
            ChatChannel::Division => Color32::GOLD,
            ChatChannel::Global => Color32::WHITE,
            ChatChannel::Team => Color32::LIGHT_GREEN,
            _ => Color32::ORANGE,
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
            if child.small_button(crate::icons::COPY).on_hover_text("Copy message").clicked() {
                ui.ctx().copy_text(text);
            }
        }
        ui.add(Separator::default());
        ui.end_row();
    }
}
