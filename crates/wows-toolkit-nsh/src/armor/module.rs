//! The commands the TSX interface drives the viewer with. Every command runs
//! its work off the interface thread.

use nsh_core::file_dialog::FileFilter;
use nsh_core::file_dialog::SaveDialog;
use nsh_core::prelude::Callback;
use nsh_core::prelude::blocking;
use nsh_core::prelude::module;
use nsh_core::prelude::pick_save_path;
use rust_i18n::t;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_toolkit_armor::load_ship::PlateKey;
use wows_toolkit_armor::load_ship::default_export_filename;
use wows_toolkit_armor::load_ship::export_options_from_selection;
use wows_toolkit_armor::load_ship::export_ship_glb;
use wows_toolkit_armor::scene::ArmorScene;
use wows_toolkit_armor::scene::Isolate;
use wows_toolkit_armor::upload::DisplaySettings;
use wows_toolkit_armor::viewport::types::LightingSettings;
use wows_toolkit_armor::visibility::SidebarHighlightKey;
use wows_toolkit_viewmodel::armor::penetration::resolve_ship_shells;
use wows_toolkit_viewmodel::formatting::byte_size;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::ship::ShipExportOptions;
use wowsunpack::game_params::types::Km;

use super::catalog::CatalogView;
use super::catalog::catalog_view;
use super::defaults;
use super::error::ArmorError;
use super::export::Contents;
use super::export::ExportResult;
use super::state;
use super::state::Bundle;
use super::state::PaneId;
use super::state::ShipSource;
use super::state::State;
use super::view::ArmorView;
use super::view::HoverView;
use super::view::Look;
use super::view::armor_view;
use super::view::parse_hex;

/// Which isolation an arc command asks for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, nsh_core::prelude::PropEnum)]
pub enum IsolateWhat {
    #[default]
    Plates,
    Zones,
}

impl From<IsolateWhat> for Isolate {
    fn from(what: IsolateWhat) -> Self {
        match what {
            IsolateWhat::Plates => Self::Plates,
            IsolateWhat::Zones => Self::Zones,
        }
    }
}

/// A lighting preset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, nsh_core::prelude::PropEnum)]
pub enum LightingPreset {
    #[default]
    InGame,
    Flat,
    Studio,
}

/// One lighting slider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, nsh_core::prelude::PropEnum)]
pub enum LightingField {
    #[default]
    Flat,
    Intensity,
    Azimuth,
    Elevation,
    Rim,
    Specular,
    Shininess,
}

/// One lighting colour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, nsh_core::prelude::PropEnum)]
pub enum LightingColor {
    #[default]
    Ambient,
    Key,
}

/// The texture cap of an export, as the interface names it.
pub use super::export::Resolution;

async fn run<T: Send + 'static>(work: impl FnOnce() -> Result<T, ArmorError> + Send + 'static) -> Result<T, String> {
    match blocking(work).await {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(source) => Err(ArmorError::Task { source }.to_string()),
    }
}

/// Applies `change` off the interface thread.
async fn change(change: impl FnOnce(&mut State) + Send + 'static) -> Result<(), String> {
    run(move || {
        state::update(change);
        Ok(())
    })
    .await
}

/// Applies `change` to the active pane's scene.
async fn scene(change: impl FnOnce(&mut ArmorScene) + Send + 'static) -> Result<(), String> {
    self::change(move |state| state.act(change)).await
}

async fn display(change: impl FnOnce(&mut DisplaySettings) + Send + 'static) -> Result<(), String> {
    scene(move |scene| scene.mutate_display_settings(change)).await
}

/// Asks where to write `source`'s model, then writes it. `None` when the
/// save dialog was cancelled.
async fn write_export(
    bundle: std::sync::Arc<Bundle>,
    source: ShipSource,
    options: ShipExportOptions,
) -> Result<Option<ExportResult>, String> {
    let dialog = SaveDialog::new()
        .file_name(default_export_filename(&source.display_name))
        .filter(FileFilter::new("glTF Binary", ["glb"]))
        .title(t!("ui.armor.export_model").into_owned())
        .button_label(t!("ui.armor.export_button").into_owned());
    let picked =
        pick_save_path(dialog).await.map_err(|reason| ArmorError::SaveDialog { reason }.to_string())?;
    let Some(path) = picked else { return Ok(None) };

    run(move || {
        export_ship_glb(&bundle.assets, &source.param_index, &options, &path)?;
        tracing::info!("armor viewer: exported {} to {}", source.display_name, path.display());
        // Only knowable once the file is on disk.
        let bytes = std::fs::metadata(&path).map(|meta| meta.len()).ok();
        Ok(Some(ExportResult {
            ship: source.display_name,
            path: path.display().to_string(),
            bytes,
            size: bytes.map(byte_size),
        }))
    })
    .await
}

/// The armor viewer, as the TSX interface drives it.
#[module]
pub struct Armor;

#[module]
impl Armor {
    /// Starts the device and the ship data, once, and returns the view.
    pub async fn start() -> Result<ArmorView, String> {
        run(|| {
            state::update(State::ensure_started);
            Ok(state::read(armor_view))
        })
        .await
    }

    pub async fn view() -> Result<ArmorView, String> {
        run(|| Ok(state::read(armor_view))).await
    }

    /// Calls `on_change` with the view now and after every change.
    pub async fn watch(on_change: Callback<ArmorView>) -> Result<(), String> {
        let mut changes = state::subscribe_view();
        loop {
            changes.borrow_and_update();
            let view = run(|| Ok(state::read(armor_view))).await?;
            on_change.post(view);
            if changes.changed().await.is_err() {
                return Ok(());
            }
        }
    }

    /// Calls `on_change` with what the pointer is over, now and on every
    /// change.
    pub async fn watch_hover(on_change: Callback<HoverView>) -> Result<(), String> {
        let mut changes = state::subscribe_hover();
        loop {
            changes.borrow_and_update();
            let hover = run(|| Ok(state::read(|state| state.hover.clone()))).await?;
            on_change.post(hover);
            if changes.changed().await.is_err() {
                return Ok(());
            }
        }
    }

    /// The whole ships tree; empty until the ship data has loaded.
    pub async fn catalog() -> Result<CatalogView, String> {
        Self::filter_catalog(String::new()).await
    }

    /// The branches a search for `query` finds, accents folded.
    pub async fn filter_catalog(query: String) -> Result<CatalogView, String> {
        run(move || {
            let bundle = state::read(|state| state.bundle().cloned());
            Ok(bundle.map(|bundle| catalog_view(&bundle.catalog, &query)).unwrap_or_default())
        })
        .await
    }

    pub async fn open_ship(param_index: String, display_name: String) -> Result<(), String> {
        change(move |state| state.open_ship(ShipSource { param_index, display_name })).await
    }

    /// A new pane copying the active one; with a ship, that ship opens in it.
    pub async fn compare(param_index: Option<String>, display_name: Option<String>) -> Result<(), String> {
        let ship = param_index.zip(display_name).map(|(param_index, display_name)| ShipSource { param_index, display_name });
        change(move |state| state.compare(ship)).await
    }

    pub async fn activate_pane(pane_id: u32) -> Result<(), String> {
        change(move |state| state.activate(PaneId::new(pane_id))).await
    }

    pub async fn close_pane(pane_id: u32) -> Result<(), String> {
        change(move |state| state.close_pane(PaneId::new(pane_id))).await
    }

    pub async fn set_mirror_cameras(on: bool) -> Result<(), String> {
        change(move |state| state.set_mirror_cameras(on)).await
    }

    pub async fn set_sync_options(on: bool) -> Result<(), String> {
        change(move |state| state.set_sync_options(on)).await
    }

    pub async fn set_stack_panes(on: bool) -> Result<(), String> {
        change(move |state| state.stack_panes = on).await
    }

    pub async fn set_options_open(open: bool) -> Result<(), String> {
        change(move |state| state.set_options_open(open)).await
    }

    pub async fn set_pen_check_open(open: bool) -> Result<(), String> {
        change(move |state| state.pen_open = open).await
    }

    pub async fn set_show_hidden(on: bool) -> Result<(), String> {
        scene(move |scene| scene.set_show_hidden_only(on)).await
    }

    pub async fn set_show_gaps(on: bool) -> Result<(), String> {
        scene(move |scene| scene.set_show_gaps(on)).await
    }

    pub async fn set_splash_mode(on: bool) -> Result<(), String> {
        scene(move |scene| scene.set_splash_mode(on)).await
    }

    pub async fn set_splash_boxes(on: bool) -> Result<(), String> {
        scene(move |scene| scene.set_show_splash_boxes(on)).await
    }

    pub async fn set_trajectory_mode(on: bool) -> Result<(), String> {
        scene(move |scene| scene.set_trajectory_mode(on)).await
    }

    pub async fn undo_visibility() -> Result<(), String> {
        scene(ArmorScene::undo_visibility).await
    }

    pub async fn redo_visibility() -> Result<(), String> {
        scene(ArmorScene::redo_visibility).await
    }

    pub async fn toggle_plate(zone: String, material: String, tenths: i32) -> Result<(), String> {
        change(move |state| {
            state.toggle_plate(PlateKey { zone, material_name: material, thickness_tenths: tenths })
        })
        .await
    }

    pub async fn show_all_plates() -> Result<(), String> {
        scene(ArmorScene::show_all_plates).await
    }

    pub async fn disable_material(zone: String, material: String) -> Result<(), String> {
        scene(move |scene| scene.disable_material(&zone, &material)).await
    }

    /// The tree's All and None.
    pub async fn set_all_parts(visible: bool) -> Result<(), String> {
        scene(move |scene| {
            if visible {
                scene.set_all_parts_visible();
            } else {
                scene.set_all_parts_hidden();
            }
        })
        .await
    }

    pub async fn reset_plate_overrides() -> Result<(), String> {
        scene(ArmorScene::reset_plate_overrides).await
    }

    /// A zone checkbox; `solo` is a Ctrl-click.
    pub async fn set_zone(zone: String, checked: bool, solo: bool) -> Result<(), String> {
        scene(move |scene| scene.toggle_zone(zone, checked, solo)).await
    }

    pub async fn set_part(zone: String, part: String, checked: bool) -> Result<(), String> {
        change(move |state| {
            let plates = state.part_thicknesses(&zone, &part);
            state.act(|scene| scene.toggle_part(zone, part, plates, checked));
        })
        .await
    }

    pub async fn hover_tree_zone(zone: String) -> Result<(), String> {
        scene(move |scene| scene.set_sidebar_hover(SidebarHighlightKey::Zone(zone))).await
    }

    pub async fn hover_tree_part(zone: String, part: String) -> Result<(), String> {
        scene(move |scene| scene.set_sidebar_hover(SidebarHighlightKey::Part(zone, part))).await
    }

    pub async fn hover_tree_plate(zone: String, part: String, tenths: i32) -> Result<(), String> {
        let key = PlateKey { zone, material_name: part, thickness_tenths: tenths };
        scene(move |scene| scene.set_sidebar_hover(SidebarHighlightKey::Plate(key))).await
    }

    pub async fn clear_tree_hover() -> Result<(), String> {
        scene(ArmorScene::clear_sidebar_hover).await
    }

    pub async fn set_hull_all(visible: bool) -> Result<(), String> {
        change(move |state| {
            let meshes = state.hull_meshes(None);
            state.set_hull_visible(meshes, visible);
        })
        .await
    }

    pub async fn set_hull_group(group: String, visible: bool) -> Result<(), String> {
        change(move |state| {
            let meshes = state.hull_meshes(Some(&group));
            state.set_hull_visible(meshes, visible);
        })
        .await
    }

    pub async fn set_hull_mesh(name: String, visible: bool) -> Result<(), String> {
        change(move |state| state.set_hull_visible(vec![name], visible)).await
    }

    pub async fn set_hull_opaque(on: bool) -> Result<(), String> {
        scene(move |scene| scene.set_hull_opaque(on)).await
    }

    pub async fn select_hull_upgrade(key: String) -> Result<(), String> {
        change(move |state| {
            let mut reload = false;
            state.act(|scene| reload = scene.select_hull_upgrade(key));
            if reload {
                state.reload_active();
            }
        })
        .await
    }

    /// `component` is the component type as the hull view names it.
    pub async fn select_module(component: String, name: String) -> Result<(), String> {
        change(move |state| {
            let found = state.active_pane().scene.current_armor().and_then(|armor| {
                armor
                    .module_alternatives
                    .iter()
                    .map(|(kind, _)| *kind)
                    .find(|kind| kind.to_string() == component)
            });
            let Some(kind) = found else { return };
            let mut reload = false;
            state.act(|scene| reload = scene.select_module_alternative(kind, name));
            if reload {
                state.reload_active();
            }
        })
        .await
    }

    pub async fn select_lod(lod: u32) -> Result<(), String> {
        change(move |state| {
            let mut reload = false;
            state.act(|scene| reload = scene.select_hull_lod(lod as usize));
            if reload {
                state.reload_active();
            }
        })
        .await
    }

    /// `None` is the stock appearance.
    pub async fn select_camo(id: Option<u32>) -> Result<(), String> {
        change(move |state| state.select_camo(id.map(|id| CamoSchemeId(id as usize)))).await
    }

    pub async fn set_legend_visible(on: bool) -> Result<(), String> {
        change(move |state| state.set_legend(|legend| legend.visible = on)).await
    }

    pub async fn set_legend_collapsed(on: bool) -> Result<(), String> {
        change(move |state| state.set_legend(|legend| legend.collapsed = on)).await
    }

    /// Where the legend was dragged to.
    pub async fn move_legend(x: f32, y: f32) -> Result<(), String> {
        change(move |state| {
            state.set_legend(|legend| {
                legend.x = x;
                legend.y = y;
            })
        })
        .await
    }

    pub async fn set_plate_edges(on: bool) -> Result<(), String> {
        display(move |display| display.show_plate_edges = on).await
    }

    pub async fn set_waterline(on: bool) -> Result<(), String> {
        display(move |display| display.show_waterline = on).await
    }

    pub async fn set_waterline_opacity(value: f32) -> Result<(), String> {
        display(move |display| display.waterline_opacity = value.clamp(0.05, 1.0)).await
    }

    pub async fn set_zero_mm(on: bool) -> Result<(), String> {
        display(move |display| display.show_zero_mm = on).await
    }

    pub async fn set_ship_center(on: bool) -> Result<(), String> {
        display(move |display| display.show_ship_center = on).await
    }

    pub async fn set_armor_opacity(value: f32) -> Result<(), String> {
        display(move |display| display.armor_opacity = value.clamp(0.1, 1.0)).await
    }

    pub async fn set_marker_opacity(value: f32) -> Result<(), String> {
        display(move |display| display.marker_opacity = value.clamp(0.0, 1.0)).await
    }

    pub async fn set_roll(degrees: f32) -> Result<(), String> {
        scene(move |scene| scene.set_model_roll_deg(degrees)).await
    }

    pub async fn set_rings_shown(on: bool) -> Result<(), String> {
        scene(move |scene| {
            let mut rings = scene.camera_rings().clone();
            rings.shown = on;
            scene.set_camera_rings(rings);
        })
        .await
    }

    pub async fn set_rings_mode(mode: String) -> Result<(), String> {
        scene(move |scene| {
            if !scene.camera_modes().contains(&mode) {
                return;
            }
            let mut rings = scene.camera_rings().clone();
            rings.mode = Some(mode);
            scene.set_camera_rings(rings);
        })
        .await
    }

    pub async fn set_rings_fov(value: f32) -> Result<(), String> {
        scene(move |scene| {
            let mut rings = scene.camera_rings().clone();
            rings.fov = value.clamp(0.0, 1.0);
            scene.set_camera_rings(rings);
        })
        .await
    }

    pub async fn set_rings_height(value: f32) -> Result<(), String> {
        scene(move |scene| {
            let mut rings = scene.camera_rings().clone();
            rings.height = value.clamp(-1.0, 1.0);
            scene.set_camera_rings(rings);
        })
        .await
    }

    pub async fn set_zoom_path(on: bool) -> Result<(), String> {
        scene(move |scene| {
            let mut rings = scene.camera_rings().clone();
            rings.zoom_path = on;
            scene.set_camera_rings(rings);
        })
        .await
    }

    pub async fn set_zoom_path_at_fov(on: bool) -> Result<(), String> {
        scene(move |scene| {
            let mut rings = scene.camera_rings().clone();
            rings.zoom_path_at_fov = on;
            scene.set_camera_rings(rings);
        })
        .await
    }

    pub async fn set_zoom_path_at_max_fov(on: bool) -> Result<(), String> {
        scene(move |scene| {
            let mut rings = scene.camera_rings().clone();
            rings.zoom_path_at_max_fov = on;
            scene.set_camera_rings(rings);
        })
        .await
    }

    pub async fn set_perspective(on: bool) -> Result<(), String> {
        scene(move |scene| scene.set_perspective_enabled(on)).await
    }

    pub async fn set_perspective_fov(degrees: f32) -> Result<(), String> {
        scene(move |scene| scene.set_perspective_fov(degrees)).await
    }

    pub async fn set_perspective_look(look: Look) -> Result<(), String> {
        scene(move |scene| scene.set_perspective_look_mode(look.into())).await
    }

    pub async fn set_lighting_enabled(on: bool) -> Result<(), String> {
        scene(move |scene| scene.mutate_lighting(|lighting| lighting.enabled = on)).await
    }

    pub async fn set_lighting_preset(preset: LightingPreset) -> Result<(), String> {
        let preset = match preset {
            LightingPreset::InGame => LightingSettings::in_game(),
            LightingPreset::Flat => LightingSettings::flat(),
            LightingPreset::Studio => LightingSettings::studio(),
        };
        scene(move |scene| scene.set_lighting_preset(preset)).await
    }

    pub async fn set_lighting(field: LightingField, value: f32) -> Result<(), String> {
        scene(move |scene| {
            scene.mutate_lighting(|lighting| match field {
                LightingField::Flat => lighting.flat_intensity = value.clamp(0.0, 1.5),
                LightingField::Intensity => lighting.key_intensity = value.clamp(0.0, 1.5),
                LightingField::Azimuth => lighting.azimuth_deg = value.clamp(0.0, 360.0),
                LightingField::Elevation => lighting.elevation_deg = value.clamp(-90.0, 90.0),
                LightingField::Rim => lighting.rim_strength = value.clamp(0.0, 1.0),
                LightingField::Specular => lighting.specular_strength = value.clamp(0.0, 1.0),
                LightingField::Shininess => lighting.shininess = value.clamp(1.0, 128.0),
            })
        })
        .await
    }

    /// `color` is `"#rrggbb"`.
    pub async fn set_lighting_color(which: LightingColor, color: String) -> Result<(), String> {
        let Some(rgb) = parse_hex(&color) else { return Err(format!("not a #rrggbb colour: {color}")) };
        scene(move |scene| {
            scene.mutate_lighting(|lighting| match which {
                LightingColor::Ambient => lighting.flat_color = rgb,
                LightingColor::Key => lighting.key_color = rgb,
            })
        })
        .await
    }

    pub async fn set_cast_range(km: f32) -> Result<(), String> {
        scene(move |scene| scene.set_cast_range(Km::new(km))).await
    }

    pub async fn set_continue_on_ricochet(on: bool) -> Result<(), String> {
        scene(move |scene| scene.set_continue_on_ricochet(on)).await
    }

    pub async fn clear_arcs() -> Result<(), String> {
        scene(ArmorScene::clear_trajectories).await
    }

    pub async fn remove_arc(index: u32) -> Result<(), String> {
        scene(move |scene| scene.remove_trajectory(index as usize)).await
    }

    pub async fn set_arc_range(index: u32, km: f32) -> Result<(), String> {
        scene(move |scene| scene.set_trajectory_range(index as usize, Km::new(km))).await
    }

    pub async fn isolate_arc(index: u32, what: IsolateWhat, on: bool) -> Result<(), String> {
        scene(move |scene| scene.isolate_arc(index as usize, what.into(), on)).await
    }

    pub async fn isolate_all_arcs(what: IsolateWhat) -> Result<(), String> {
        scene(move |scene| scene.isolate_all_arcs(what.into())).await
    }

    pub async fn set_pen_search(text: String) -> Result<(), String> {
        change(move |state| state.pen.search = text).await
    }

    pub async fn add_pen_ship(param_index: String) -> Result<(), String> {
        run(move || {
            state::update(|state| {
                let bundle = state.bundle().cloned().ok_or(ArmorError::CatalogNotReady)?;
                let ship = resolve_ship_shells(bundle.assets.metadata(), &param_index)
                    .ok_or(ArmorError::NoComparisonData { param_index })?;
                state.pen.add(ship);
                state.push_cast_ship();
                Ok(())
            })
        })
        .await
    }

    pub async fn remove_pen_ship(index: u32) -> Result<(), String> {
        change(move |state| {
            state.pen.remove(index as usize);
            state.push_cast_ship();
        })
        .await
    }

    pub async fn clear_pen_ships() -> Result<(), String> {
        change(|state| {
            state.pen.clear();
            state.push_cast_ship();
        })
        .await
    }

    pub async fn set_firing_ship(param_index: String) -> Result<(), String> {
        change(move |state| {
            state.pen.set_firing_ship(&param_index);
            state.push_cast_ship();
        })
        .await
    }

    pub async fn set_firing_shell(index: u32) -> Result<(), String> {
        change(move |state| {
            state.pen.set_firing_shell(index as usize);
            state.push_cast_ship();
        })
        .await
    }

    pub async fn set_ifhe(on: bool) -> Result<(), String> {
        change(move |state| state.pen.ifhe = on).await
    }

    /// `None` is every enemy.
    pub async fn set_incoming_attacker(entity: Option<u32>) -> Result<(), String> {
        change(move |state| state.incoming.filter.attacker = entity.map(EntityId::from)).await
    }

    pub async fn set_incoming_secondaries(on: bool) -> Result<(), String> {
        change(move |state| state.incoming.filter.secondaries = on).await
    }

    /// Moves playback to `seconds` of battle clock.
    pub async fn seek_to(seconds: f32) -> Result<(), String> {
        run(move || {
            state::seek(GameClock(seconds));
            Ok(())
        })
        .await
    }

    /// Moves playback to where a press `along` (0..1) the health strip lands.
    pub async fn seek_strip(along: f32) -> Result<(), String> {
        run(move || {
            let clock = state::read(|state| {
                state.incoming.context.health.as_ref().map(|strip| strip.clock_at(along.clamp(0.0, 1.0)))
            });
            if let Some(clock) = clock {
                state::seek(clock);
            }
            Ok(())
        })
        .await
    }

    /// Opens the export dialog on a catalog ship.
    pub async fn open_export_dialog(param_index: String, display_name: String) -> Result<(), String> {
        run(move || {
            // The remembered choices are a convenience: an unreadable database
            // offers the first-export defaults, which is what it would hold.
            let remembered = defaults::load_export_defaults().unwrap_or_else(|error| {
                tracing::warn!("armor viewer: the export defaults could not be read: {error}");
                Default::default()
            });
            state::update(|state| state.open_export_dialog(ShipSource { param_index, display_name }, remembered))
        })
        .await
    }

    pub async fn set_export_contents(contents: Contents) -> Result<(), String> {
        change(move |state| state.edit_export(|draft| draft.contents = contents.into())).await
    }

    pub async fn set_export_hull(key: String) -> Result<(), String> {
        change(move |state| state.edit_export(|draft| draft.hull = Some(key))).await
    }

    pub async fn set_export_lod(lod: u32) -> Result<(), String> {
        change(move |state| state.edit_export(|draft| draft.lod = lod as usize)).await
    }

    pub async fn set_export_resolution(resolution: Resolution) -> Result<(), String> {
        change(move |state| state.edit_export(|draft| draft.texture_res = resolution.into())).await
    }

    pub async fn set_export_camo(id: u32, on: bool) -> Result<(), String> {
        change(move |state| {
            state.edit_export(|draft| {
                let id = CamoSchemeId(id as usize);
                if on {
                    draft.camos.insert(id);
                } else {
                    draft.camos.remove(&id);
                }
            })
        })
        .await
    }

    pub async fn export_camos_none() -> Result<(), String> {
        change(|state| state.edit_export(|draft| draft.camos.clear())).await
    }

    pub async fn export_camos_reset() -> Result<(), String> {
        change(State::reset_export_camos).await
    }

    pub async fn retry_export_pricing() -> Result<(), String> {
        change(State::retry_export_pricing).await
    }

    pub async fn cancel_export_dialog() -> Result<(), String> {
        change(|state| state.export = None).await
    }

    /// Remembers the dialog's choices, closes it, asks where to write the
    /// model and writes it. `None` when the save dialog was cancelled.
    pub async fn confirm_export_dialog() -> Result<Option<ExportResult>, String> {
        let confirmed = run(|| state::update(State::confirm_export_dialog)).await?;
        let remembered = confirmed.defaults;
        state::spawn_named("armor-export-defaults", move || {
            if let Err(error) = defaults::save_export_defaults(remembered) {
                tracing::warn!("armor viewer: the export defaults were not saved: {error}");
            }
        });
        write_export(confirmed.bundle, confirmed.source, confirmed.options).await
    }

    /// Puts the inline export confirmation over the active pane.
    pub async fn open_export_confirm() -> Result<(), String> {
        run(|| {
            state::update(|state| {
                let index = state.active_index();
                if !state.panes[index].scene.has_armor() {
                    return Err(ArmorError::NoShip);
                }
                state.panes[index].export_confirm = true;
                Ok(())
            })
        })
        .await
    }

    pub async fn cancel_export_confirm() -> Result<(), String> {
        change(|state| {
            let index = state.active_index();
            state.panes[index].export_confirm = false;
        })
        .await
    }

    /// Writes the active pane's ship as shown: its hull, level of detail and
    /// modules, textured.
    pub async fn confirm_export() -> Result<Option<ExportResult>, String> {
        let (bundle, source, options) = run(|| {
            state::update(|state| {
                let index = state.active_index();
                state.panes[index].export_confirm = false;
                let bundle = state.bundle().cloned().ok_or(ArmorError::CatalogNotReady)?;
                let source = state.active_source().cloned().ok_or(ArmorError::NoShip)?;
                let selection = state.active_pane().scene.hull_selection();
                let options = export_options_from_selection(selection.lod, selection.hull, selection.modules);
                Ok((bundle, source, options))
            })
        })
        .await?;
        write_export(bundle, source, options).await
    }
}
