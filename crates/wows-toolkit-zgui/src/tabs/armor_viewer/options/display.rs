//! The display section: the model settings, then the camera rings and the lighting, each a group
//! that folds away.

use landaire_ui::ColorPickerProps;
use landaire_ui::Rgb;
use landaire_ui::SliderFieldProps;
use landaire_ui::SliderRange;
use rust_i18n::t;
use wows_toolkit_armor::upload::DisplaySettings;
use wows_toolkit_armor::viewport::types::LightingSettings;
use wows_toolkit_viewmodel::armor::camera_perspective::CameraPerspective;
use wows_toolkit_viewmodel::armor::camera_perspective::LookMode;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::prelude::Binding;

use crate::tabs::armor_viewer::options::HeadDepth;
use crate::tabs::armor_viewer::options::MatchState;
use crate::tabs::armor_viewer::options::OptionSection;
use crate::tabs::armor_viewer::options::OptionSectionsOpen;
use crate::tabs::armor_viewer::options::SectionCx;
use crate::tabs::armor_viewer::options::filter::MarkedLabel;
use crate::tabs::armor_viewer::options::filter::RailFilter;
use crate::tabs::armor_viewer::options::folding_head;
use crate::tabs::armor_viewer::options::tools::choice_button;
use crate::tabs::armor_viewer::options::tree_filter::filtered_checkbox;
use crate::tabs::armor_viewer::options::visibility::no_ship_loaded;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::tabs::armor_viewer::viewport::CameraRingSettings;

const SHEET: &str = "wt-armor-display";

const CSS: &str = css!(
    ".wt-armor-display { display: flex; flex-direction: column; gap: 8px; width: 100%; }
    .wt-armor-display__rows { display: flex; flex-direction: column; gap: 8px; }
    .wt-armor-display__tight { display: flex; flex-direction: column; gap: 4px; }
    .wt-armor-display__group-body { padding-left: 4px; }
    .wt-armor-display__choices { display: flex; align-items: center; gap: 4px; flex-wrap: wrap; }
    .wt-armor-display__indent { display: flex; flex-wrap: wrap; gap: 16px; padding-left: 20px; }
    .wt-armor-display__row { display: flex; align-items: center; gap: 4px; min-width: 0; }
    .wt-armor-display__row[data-dimmed] { opacity: 0.5; }
    .wt-armor-display__label { flex: none; width: 88px; font-size: 13px; }
    .wt-armor-display__label--small { flex: none; width: 88px; font-size: 12px; }
    .wt-armor-display__slider { flex: 1 1 auto; min-width: 48px; }
    .wt-armor-display__dim { font-size: 12px; color: var(--wt-color-text-dim); }"
);

/// The display section's body: `None` when the rail's filter keeps nothing in it.
pub fn body(cx: &SectionCx) -> Option<AnyView> {
    install_stylesheet(SHEET, CSS);
    let viewport = cx.viewport;
    let filter = &cx.filter;

    // Nothing here is drawn without a scene.
    let Some(features) = cx.features else {
        return (!filter.is_active()).then(no_ship_loaded);
    };

    let armor = features.armor;
    let ship = features.ship;
    let model = model_rows(cx, armor, filter);
    // The camera orbits are the ship's own, which a model has none of.
    let camera = ship
        .then(|| {
            display_group(cx, OptionSection::CameraRings, t!("ui.armor.camera_rings").into_owned(), |filter| {
                camera_rings_group(viewport, filter, cx.matching)
            })
        })
        .flatten();
    let lighting = display_group(cx, OptionSection::Lighting, t!("ui.armor.lighting").into_owned(), |filter| {
        lighting_group(viewport, filter)
    });

    let parts: Vec<AnyView> = model.into_iter().chain(camera).chain(lighting).collect();
    (!parts.is_empty()).then(|| AnyView::new(view! { column(class = "wt-armor-display") {{parts}} }))
}

/// One folding group inside the display section.
///
/// While filtering the group is open whatever the reader left it at, and a match on its title
/// keeps everything in it; with nothing matched it is not drawn at all. Its head is inert then, so
/// the open state the reader left is what comes back when the filter is cleared.
fn display_group(
    cx: &SectionCx,
    which: OptionSection,
    title: String,
    body: impl FnOnce(&RailFilter) -> Option<AnyView>,
) -> Option<AnyView> {
    let filter = &cx.filter;
    let marked = filter.label(title.clone());
    let (open, content) = if filter.is_active() {
        let inner = if marked.is_some() { RailFilter::default() } else { filter.clone() };
        let content = body(&inner);
        if content.is_none() && marked.is_none() {
            return None;
        }
        (true, content)
    } else {
        let open = cx.sections.is_open(which);
        (open, if open { body(filter) } else { None })
    };
    let title = marked.unwrap_or_else(|| MarkedLabel::plain(title));

    let workspace = cx.workspace;
    let head = folding_head(
        format!("armor-options-group-{}", OptionSectionsOpen::id(which)),
        title,
        open,
        HeadDepth::Group,
        (!filter.is_active()).then_some(move || workspace.sections.update(|sections| sections.toggle(which))),
    );
    let content = content.map(|content| view! { box(class = "wt-armor-display__group-body") {{content}} });

    Some(AnyView::new(view! {
        column(class = "wt-armor-display__rows") {
            {head}
            {content}
        }
    }))
}

/// A checkbox over one display setting.
fn setting_checkbox(
    viewport: ArmorViewportState,
    test_id: &str,
    label: String,
    filter: &RailFilter,
    read: fn(&DisplaySettings) -> bool,
    apply: fn(&mut DisplaySettings, bool),
) -> Option<AnyView> {
    filtered_checkbox(
        test_id.to_owned(),
        label,
        Signal::derive_local(move || viewport.read(|model| read(&model.display_settings)).unwrap_or(false)),
        false,
        filter,
        move |on| viewport.mutate_display_settings(move |settings| apply(settings, on)),
    )
}

/// The settings at the top of the display section, about the model itself. `armor` shows the
/// legend, plate edges, 0 mm plates and the armor and marker opacity.
fn model_rows(cx: &SectionCx, armor: bool, filter: &RailFilter) -> Option<AnyView> {
    let viewport = cx.viewport;
    let workspace = cx.workspace;
    let factory = DisplaySettings::default();

    // The legend belongs to the workspace, not to this viewport, but the reader looks for it here.
    let legend = filtered_checkbox(
        "armor-display-legend".to_owned(),
        t!("ui.armor.show_armor_thickness").into_owned(),
        Signal::derive_local(move || workspace.legend.get().visible),
        false,
        filter,
        move |on| {
            workspace.legend.update(|legend| legend.visible = on);
            workspace.save_defaults();
        },
    );
    let armor_rows = armor.then(|| {
        [
            legend,
            setting_checkbox(
                viewport,
                "armor-display-plate-edges",
                t!("ui.armor.plate_edges").into_owned(),
                filter,
                |d| d.show_plate_edges,
                |d, on| d.show_plate_edges = on,
            ),
            setting_checkbox(
                viewport,
                "armor-display-zero-mm",
                t!("ui.armor.zero_mm_plates").into_owned(),
                filter,
                |d| d.show_zero_mm,
                |d, on| d.show_zero_mm = on,
            ),
        ]
    });
    let opacity_rows = armor.then(|| {
        // A marker control with nothing to act on is left out.
        let marker_label = t!("ui.armor.marker_opacity").into_owned();
        let marker = (filter.keeps(&marker_label) && cx.matching.is_none_or(|state| state.markers)).then(|| {
            let filter = filter.clone();
            AnyView::new(view! {
                if move || viewport.hits_drawn() > 0 {
                    {labeled_slider_row(
                        marker_label.clone(),
                        Signal::derive_local(move || {
                            viewport.read(|model| f64::from(model.display_settings.marker_opacity)).unwrap_or(0.0)
                        }),
                        const { SliderRange::new(0.0, 1.0, 0.01) },
                        f64::from(factory.marker_opacity),
                        false,
                        &filter,
                        move |value| viewport.mutate_display_settings(move |d| d.marker_opacity = value as f32),
                    )}
                }
            })
        });
        [
            labeled_slider_row(
                t!("ui.armor.armor_opacity").into_owned(),
                Signal::derive_local(move || {
                    viewport.read(|model| f64::from(model.display_settings.armor_opacity)).unwrap_or(0.0)
                }),
                const { SliderRange::new(0.1, 1.0, 0.01) },
                f64::from(factory.armor_opacity),
                false,
                filter,
                move |value| viewport.mutate_display_settings(move |d| d.armor_opacity = value as f32),
            ),
            marker,
        ]
    });

    // The range of the egui slider: a hull heels this far in a hard turn, and further reads as a
    // capsize rather than a camera angle worth checking armor against.
    const ROLL_LIMIT_DEG: f64 = 25.0;
    let rows: Vec<AnyView> = armor_rows
        .into_iter()
        .flatten()
        .chain([
            setting_checkbox(
                viewport,
                "armor-display-grid",
                t!("ui.armor.grid").into_owned(),
                filter,
                |d| d.show_grid,
                |d, on| d.show_grid = on,
            ),
            setting_checkbox(
                viewport,
                "armor-display-length-marks",
                t!("ui.armor.length_marks").into_owned(),
                filter,
                |d| d.show_length_marks,
                |d, on| d.show_length_marks = on,
            ),
            setting_checkbox(
                viewport,
                "armor-display-ship-center",
                t!("ui.armor.ship_center").into_owned(),
                filter,
                |d| d.show_ship_center,
                |d, on| d.show_ship_center = on,
            ),
        ])
        .chain(opacity_rows.into_iter().flatten())
        // Heeling the hull over is what says whether a belt is still a belt at the angle the ship
        // is fighting at.
        .chain([labeled_slider_row(
            t!("ui.armor.roll").into_owned(),
            Signal::derive_local(move || f64::from(viewport.model_roll_deg())),
            const { SliderRange::new(-ROLL_LIMIT_DEG, ROLL_LIMIT_DEG, 0.5) },
            0.0,
            false,
            filter,
            move |value| viewport.set_model_roll_deg(value as f32),
        )])
        .flatten()
        .collect();

    (!rows.is_empty()).then(|| AnyView::new(view! { column(class = "wt-armor-display__rows") {{rows}} }))
}

/// Adopts `change` applied to the camera rings as they are now.
fn change_rings(viewport: ArmorViewportState, change: impl FnOnce(&mut CameraRingSettings)) {
    let mut next = viewport.camera_rings();
    change(&mut next);
    viewport.set_camera_rings(next);
}

/// The camera-rings group: whether the orbits are drawn, which mode's, and where on them the
/// camera sits.
///
/// Every control but the first is refused while the rings are off, and the group says so when the
/// ship names no modes: a slider that moves nothing is worse than one that will not move.
fn camera_rings_group(
    viewport: ArmorViewportState,
    filter: &RailFilter,
    matching: Option<MatchState>,
) -> Option<AnyView> {
    let modes = viewport
        .shown_untracked()
        .map(|armor| armor.camera_trajectories.iter().map(|(name, _)| name.clone()).collect::<Vec<String>>())
        // A scene with no armor loaded names no modes.
        .unwrap_or_default();
    let has_modes = !modes.is_empty();
    let off = Signal::derive_local(move || !(has_modes && viewport.camera_rings().shown));

    let shown = filtered_checkbox(
        "armor-camera-rings".to_owned(),
        t!("ui.armor.show_camera_rings").into_owned(),
        Signal::derive_local(move || viewport.camera_rings().shown),
        !has_modes,
        filter,
        move |on| change_rings(viewport, |rings| rings.shown = on),
    );
    if !has_modes {
        return shown.map(|shown| {
            AnyView::new(view! {
                column(class = "wt-armor-display__tight") {
                    {shown}
                    text(class = "wt-armor-display__dim") {{t!("ui.armor.no_ship_loaded").into_owned()}}
                }
            })
        });
    }

    // One button per mode rather than a combo: a ship names two or three, and which one is showing
    // is the thing being read.
    let mode_buttons: Vec<AnyView> = modes
        .iter()
        .enumerate()
        .filter_map(|(index, mode)| {
            let label = filter.label(mode.clone())?;
            let chosen = mode.clone();
            let selected = Signal::derive_local(move || viewport.camera_rings().mode.as_ref() == Some(&chosen));
            let mode = mode.clone();
            let button =
                choice_button(format!("armor-camera-mode-button-{index}"), label, selected, off, None, move || {
                    change_rings(viewport, |rings| rings.mode = Some(mode.clone()))
                });
            Some(AnyView::new(view! {
                box(
                    attr:data-testid = format!("armor-camera-mode-{index}"),
                    attr:data-selected = move || selected.get().then(String::new),
                    a11y:selected = move || selected.get()
                ) {{button}}
            }))
        })
        .collect();

    let zoom_path = filtered_checkbox(
        "armor-camera-zoom-path".to_owned(),
        t!("ui.armor.show_camera_zoom_path").into_owned(),
        Signal::derive_local(move || viewport.camera_rings().zoom_path),
        off,
        filter,
        move |on| change_rings(viewport, |rings| rings.zoom_path = on),
    );
    // Offered only while the zoom path is drawn.
    let zoom_path_kept = [t!("ui.armor.zoom_path_regular_fov"), t!("ui.armor.zoom_path_max_fov")]
        .iter()
        .any(|label| filter.keeps(label))
        && matching.is_none_or(|state| state.zoom_path);
    let zoom_path_row = zoom_path_kept.then(|| {
        let filter = filter.clone();
        AnyView::new(view! {
            if move || viewport.camera_rings().zoom_path {
                row(class = "wt-armor-display__indent") {{zoom_path_choices(viewport, off, &filter)}}
            }
        })
    });

    let rows: Vec<AnyView> = [
        shown,
        (!mode_buttons.is_empty())
            .then(|| AnyView::new(view! { row(class = "wt-armor-display__choices") {{mode_buttons}} })),
        labeled_slider_row(
            t!("ui.armor.camera_fov").into_owned(),
            Signal::derive_local(move || f64::from(viewport.camera_rings().fov)),
            const { SliderRange::new(0.0, 1.0, 0.01) },
            0.0,
            off,
            filter,
            move |value| change_rings(viewport, |rings| rings.fov = value as f32),
        ),
        labeled_slider_row(
            t!("ui.armor.camera_height").into_owned(),
            Signal::derive_local(move || f64::from(viewport.camera_rings().height)),
            const { SliderRange::new(-1.0, 1.0, 0.01) },
            0.0,
            off,
            filter,
            move |value| change_rings(viewport, |rings| rings.height = value as f32),
        ),
        zoom_path,
        zoom_path_row,
    ]
    .into_iter()
    .flatten()
    .chain(perspective_rows(viewport, filter, matching))
    .collect();

    (!rows.is_empty()).then(|| AnyView::new(view! { column(class = "wt-armor-display__tight") {{rows}} }))
}

/// The camera lock: put the eye on the ship's own orbit and look out from it.
///
/// Under the orbits because it follows the same trajectory and the same two sliders: which orbit is
/// drawn is which orbit the eye rides.
fn perspective_rows(viewport: ArmorViewportState, filter: &RailFilter, matching: Option<MatchState>) -> Vec<AnyView> {
    let on = move || viewport.perspective().enabled;
    let mut rows: Vec<AnyView> = filtered_checkbox(
        "armor-camera-perspective".to_owned(),
        t!("ui.armor.camera_perspective").into_owned(),
        Signal::derive_local(on),
        false,
        filter,
        move |checked| viewport.set_perspective_enabled(checked),
    )
    .into_iter()
    .collect();

    // Offered only while the lock is on.
    let locked_kept = [
        t!("ui.armor.perspective_fov"),
        t!("ui.armor.perspective_projection"),
        t!("ui.armor.perspective_proj_game"),
        t!("ui.armor.perspective_proj_center"),
    ]
    .iter()
    .any(|label| filter.keeps(label))
        && matching.is_none_or(|state| state.perspective);
    if locked_kept {
        let filter = filter.clone();
        rows.push(AnyView::new(view! {
            if move || viewport.perspective().enabled {
                column(class = "wt-armor-display__tight") {{locked_rows(viewport, &filter)}}
            }
        }));
    }
    rows
}

/// The zoom path's two field-of-view checkboxes.
fn zoom_path_choices(
    viewport: ArmorViewportState,
    off: Signal<bool, LocalStorage>,
    filter: &RailFilter,
) -> Vec<AnyView> {
    [
        filtered_checkbox(
            "armor-camera-zoom-path-fov".to_owned(),
            t!("ui.armor.zoom_path_regular_fov").into_owned(),
            Signal::derive_local(move || viewport.camera_rings().zoom_path_at_fov),
            off,
            filter,
            move |on| change_rings(viewport, |rings| rings.zoom_path_at_fov = on),
        ),
        filtered_checkbox(
            "armor-camera-zoom-path-max".to_owned(),
            t!("ui.armor.zoom_path_max_fov").into_owned(),
            Signal::derive_local(move || viewport.camera_rings().zoom_path_at_max_fov),
            off,
            filter,
            move |on| change_rings(viewport, |rings| rings.zoom_path_at_max_fov = on),
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The locked camera's field of view and which way it aims.
fn locked_rows(viewport: ArmorViewportState, filter: &RailFilter) -> Vec<AnyView> {
    let mut rows: Vec<AnyView> = Vec::new();
    rows.extend(labeled_slider_row(
        t!("ui.armor.perspective_fov").into_owned(),
        Signal::derive_local(move || f64::from(viewport.perspective().camera.fov_deg)),
        const { SliderRange::new(30.0, 120.0, 1.0) },
        f64::from(CameraPerspective::default().fov_deg),
        false,
        filter,
        move |value| viewport.set_perspective_fov(value as f32),
    ));

    let projection = t!("ui.armor.perspective_projection").into_owned();
    let modes = [
        ("armor-perspective-proj-game", t!("ui.armor.perspective_proj_game").into_owned(), LookMode::Game),
        ("armor-perspective-proj-center", t!("ui.armor.perspective_proj_center").into_owned(), LookMode::ThroughCenter),
    ];
    // The heading names the choice, so a match on it keeps both buttons.
    let heading = filter.label(projection.clone());
    let inner = if heading.is_some() { RailFilter::default() } else { filter.clone() };
    let buttons: Vec<AnyView> = modes
        .into_iter()
        .filter_map(|(id, label, mode)| {
            let label = inner.label(label)?;
            let selected = Signal::derive_local(move || viewport.perspective().camera.look_mode == mode);
            Some(choice_button(id.to_owned(), label, selected, false, None, move || {
                viewport.set_perspective_look_mode(mode)
            }))
        })
        .collect();
    if !buttons.is_empty() {
        let heading = heading.unwrap_or_else(|| MarkedLabel::plain(projection)).view();
        rows.push(AnyView::new(view! {
            row(class = "wt-armor-display__row") {
                box(class = "wt-armor-display__label--small") {{heading}}
                row(class = "wt-armor-display__choices") {{buttons}}
            }
        }));
    }
    rows
}

/// One lighting slider, reading and writing the term `field` names.
fn lighting_slider(
    viewport: ArmorViewportState,
    label: String,
    range: SliderRange,
    reset_to: f32,
    off: Signal<bool, LocalStorage>,
    filter: &RailFilter,
    field: fn(&mut LightingSettings) -> &mut f32,
) -> Option<AnyView> {
    labeled_slider_row(
        label,
        Signal::derive_local(move || f64::from(*field(&mut viewport.lighting()))),
        range,
        f64::from(reset_to),
        off,
        filter,
        move |value| viewport.mutate_lighting(move |lighting| *field(lighting) = value as f32),
    )
}

/// One lighting color, reading and writing the color `field` names.
fn lighting_color(
    viewport: ArmorViewportState,
    label: String,
    off: Signal<bool, LocalStorage>,
    filter: &RailFilter,
    field: fn(&mut LightingSettings) -> &mut [f32; 3],
) -> Option<AnyView> {
    labeled_color_row(
        label,
        Signal::derive_local(move || rgb_of(*field(&mut viewport.lighting()))),
        off,
        filter,
        move |color| viewport.mutate_lighting(move |lighting| *field(lighting) = channels_of(color)),
    )
}

/// The lighting group: whether the hull is lit, three presets, and the terms the presets set.
fn lighting_group(viewport: ArmorViewportState, filter: &RailFilter) -> Option<AnyView> {
    let off = Signal::derive_local(move || !viewport.lighting().enabled);
    let factory = LightingSettings::default();

    let enabled = filtered_checkbox(
        "armor-display-lighting-enabled".to_owned(),
        t!("ui.armor.lighting_enabled").into_owned(),
        Signal::derive_local(move || viewport.lighting().enabled),
        false,
        filter,
        move |on| viewport.mutate_lighting(move |lighting| lighting.enabled = on),
    );

    let presets: Vec<AnyView> = [
        (
            "armor-lighting-preset-ingame",
            t!("ui.armor.lighting_preset_ingame").into_owned(),
            LightingSettings::in_game as fn() -> LightingSettings,
        ),
        ("armor-lighting-preset-flat", t!("ui.armor.lighting_preset_flat").into_owned(), LightingSettings::flat),
        ("armor-lighting-preset-studio", t!("ui.armor.lighting_preset_studio").into_owned(), LightingSettings::studio),
    ]
    .into_iter()
    .filter_map(|(id, label, preset)| {
        let label = filter.label(label)?.view();
        Some(AnyView::new(view! {
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Xs,
                disabled = off,
                attr:data-testid = id,
                on:click = move |_| viewport.set_lighting_preset(preset())
            ) {{label}}
        }))
    })
    .collect();

    let rows: Vec<AnyView> = [
        enabled,
        (!presets.is_empty()).then(|| AnyView::new(view! { row(class = "wt-armor-display__choices") {{presets}} })),
        lighting_slider(
            viewport,
            t!("ui.armor.lighting_flat").into_owned(),
            const { SliderRange::new(0.0, 1.5, 0.01) },
            factory.flat_intensity,
            off,
            filter,
            |lighting| &mut lighting.flat_intensity,
        ),
        lighting_color(viewport, t!("ui.armor.lighting_ambient_color").into_owned(), off, filter, |lighting| {
            &mut lighting.flat_color
        }),
        lighting_slider(
            viewport,
            t!("ui.armor.lighting_intensity").into_owned(),
            const { SliderRange::new(0.0, 1.5, 0.01) },
            factory.key_intensity,
            off,
            filter,
            |lighting| &mut lighting.key_intensity,
        ),
        lighting_color(viewport, t!("ui.armor.lighting_key_color").into_owned(), off, filter, |lighting| {
            &mut lighting.key_color
        }),
        lighting_slider(
            viewport,
            t!("ui.armor.lighting_azimuth").into_owned(),
            const { SliderRange::new(0.0, 360.0, 1.0) },
            factory.azimuth_deg,
            off,
            filter,
            |lighting| &mut lighting.azimuth_deg,
        ),
        lighting_slider(
            viewport,
            t!("ui.armor.lighting_elevation").into_owned(),
            const { SliderRange::new(-90.0, 90.0, 1.0) },
            factory.elevation_deg,
            off,
            filter,
            |lighting| &mut lighting.elevation_deg,
        ),
        lighting_slider(
            viewport,
            t!("ui.armor.lighting_rim").into_owned(),
            const { SliderRange::new(0.0, 1.0, 0.01) },
            factory.rim_strength,
            off,
            filter,
            |lighting| &mut lighting.rim_strength,
        ),
        lighting_slider(
            viewport,
            t!("ui.armor.lighting_specular").into_owned(),
            const { SliderRange::new(0.0, 1.0, 0.01) },
            factory.specular_strength,
            off,
            filter,
            |lighting| &mut lighting.specular_strength,
        ),
        lighting_slider(
            viewport,
            t!("ui.armor.lighting_shininess").into_owned(),
            const { SliderRange::new(1.0, 128.0, 1.0) },
            factory.shininess,
            off,
            filter,
            |lighting| &mut lighting.shininess,
        ),
    ]
    .into_iter()
    .flatten()
    .collect();

    (!rows.is_empty()).then(|| AnyView::new(view! { column(class = "wt-armor-display__rows") {{rows}} }))
}

/// One labelled color row: what it tints, and the picker that sets it.
///
/// Dimmed rather than taken away when the lighting is off: the color it would be lit in is still
/// what this sets, and a row that vanished would read as a setting that had gone.
fn labeled_color_row(
    label: String,
    value: Signal<Rgb, LocalStorage>,
    dimmed: Signal<bool, LocalStorage>,
    filter: &RailFilter,
    on_change: impl Fn(Rgb) + 'static,
) -> Option<AnyView> {
    let marked = filter.label(label.clone())?.view();
    install_stylesheet(SHEET, CSS);

    Some(AnyView::new(view! {
        row(class = "wt-armor-display__row", attr:data-dimmed = move || dimmed.get().then(String::new)) {
            box(class = "wt-armor-display__label") {{marked}}
            ColorPicker(value = Binding::controlled(value, on_change), label = label)
        }
    }))
}

/// One labelled slider row: `label`, the slider, and the field's number and reset, which puts
/// `reset_to` back.
pub(super) fn labeled_slider_row(
    label: String,
    value: Signal<f64, LocalStorage>,
    range: SliderRange,
    reset_to: f64,
    disabled: impl Into<Signal<bool, LocalStorage>>,
    filter: &RailFilter,
    on_change: impl Fn(f64) + 'static,
) -> Option<AnyView> {
    let marked = filter.label(label.clone())?.view();
    install_stylesheet(SHEET, CSS);
    let disabled: Signal<bool, LocalStorage> = disabled.into();

    Some(AnyView::new(view! {
        row(class = "wt-armor-display__row") {
            box(class = "wt-armor-display__label") {{marked}}
            SliderField(
                value = Binding::controlled(value, on_change),
                range = range,
                reset_to = reset_to,
                disabled = disabled,
                label = label,
                reset_label = t!("ui.buttons.reset").into_owned(),
                class = "wt-armor-display__slider"
            )
        }
    }))
}

/// A lighting color in 0 to 1 per channel, as the picker holds it.
fn rgb_of(channels: [f32; 3]) -> Rgb {
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;

    Rgb::from_channels(byte(channels[0]), byte(channels[1]), byte(channels[2]))
}

/// What the picker holds, as a lighting color in 0 to 1 per channel.
fn channels_of(color: Rgb) -> [f32; 3] {
    let unit = |byte: u8| f32::from(byte) / 255.0;

    [unit(color.red()), unit(color.green()), unit(color.blue())]
}
