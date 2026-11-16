//! The plates a shell struck, as the trajectory editor and the incoming-fire log list them: each
//! plate's thickness and strike angle, coloured by how square the strike was, its zone and
//! material, and what the shell did there.

use rust_i18n::t;
use wows_toolkit_viewmodel::armor::arc::ArcOutcome;
use wows_toolkit_viewmodel::armor::arc::StoppingPlate;
use wows_toolkit_viewmodel::armor::plate_detail::AngleBand;
use wows_toolkit_viewmodel::armor::plate_detail::Arrival;
use wows_toolkit_viewmodel::armor::plate_detail::Detonation;
use wows_toolkit_viewmodel::armor::plate_detail::FlatCheck;
use wows_toolkit_viewmodel::armor::plate_detail::PlateDetail;
use wows_toolkit_viewmodel::armor::plate_detail::PlateVerdict;
use wowsunpack::game_params::types::Degrees;
use zgui::css;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;

use crate::tabs::armor_viewer::viewport::ArmorViewportState;

const SHEET: &str = "wt-armor-plates";

const CSS: &str = css!(
    ".wt-armor-plates { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
    .wt-armor-plates__plate { display: flex; flex-direction: column; gap: 2px; min-width: 0; padding: 2px 0 2px 8px;
        border-left: 3px solid var(--wt-color-text-faint); }
    .wt-armor-plates__plate[data-band=good] { border-left-color: var(--wt-color-armor-angle-good); }
    .wt-armor-plates__plate[data-band=mid] { border-left-color: var(--wt-color-armor-angle-mid); }
    .wt-armor-plates__plate[data-band=bad] { border-left-color: var(--wt-color-armor-angle-bad); }
    .wt-armor-plates__head { display: flex; align-items: center; gap: 8px; font-size: 12px; }
    .wt-armor-plates__number { color: var(--wt-color-text-dim); }
    .wt-armor-plates__thickness { font-weight: 700; }
    .wt-armor-plates__server { color: var(--wt-color-text-dim); }
    .wt-armor-plates__where { font-size: 12px; color: var(--wt-color-text-dim); overflow: hidden;
        white-space: nowrap; text-overflow: ellipsis; }
    .wt-armor-plates__line { font-size: 12px; }
    .wt-armor-plates__line[data-tone=pen] { color: var(--wt-color-armor-pen); }
    .wt-armor-plates__line[data-tone=overpen] { color: var(--wt-color-armor-overpen); }
    .wt-armor-plates__line[data-tone=ricochet] { color: var(--wt-color-armor-ricochet); }
    .wt-armor-plates__line[data-tone=shatter] { color: var(--wt-color-armor-shatter); }
    .wt-armor-plates__line[data-tone=dim] { color: var(--wt-color-text-dim); }
    .wt-armor-plates__line[data-strong] { font-weight: 700; }
    .wt-armor-plates__fuse { flex: none; padding: 0 4px; border-radius: 3px; font-size: 11px;
        color: var(--wt-color-warn); border: 1px solid var(--wt-color-warn); }"
);

/// One line of a reading, in the tone of what it says.
pub(super) fn line(text: String, tone: &'static str, test_id: &'static str) -> AnyView {
    install_stylesheet(SHEET, CSS);

    AnyView::new(view! {
        text(class = "wt-armor-plates__line", attr:data-tone = tone, attr:data-testid = test_id) {{text}}
    })
}

/// How the shell arrived: its speed, its time of flight and how steeply it fell.
pub(super) fn arrival_text(arrival: Arrival) -> String {
    t!(
        "ui.armor.plates.arrival",
        velocity = format!("{:.0}", arrival.velocity.value()),
        time = format!("{:.1}", arrival.flight_time.value()),
        fall = format!("{:.1}", arrival.fall.value()),
    )
    .into_owned()
}

/// Every plate in `plates`, in the order the shell struck them. `flat` is an HE or SAP shell's
/// reading against the first plate, and `server_angle` the strike angle the server reported there.
pub(super) fn plate_list(
    plates: &[PlateDetail],
    flat: Option<FlatCheck>,
    server_angle: Option<Degrees>,
    viewport: ArmorViewportState,
) -> AnyView {
    install_stylesheet(SHEET, CSS);
    if plates.is_empty() {
        return line(t!("ui.armor.realtime.no_armor_hit").into_owned(), "dim", "armor-plates-none");
    }

    let blocks: Vec<AnyView> = plates
        .iter()
        .map(|plate| {
            let first = plate.index.value() == 0;
            plate_block(plate, flat.filter(|_| first), server_angle.filter(|_| first), viewport)
        })
        .collect();

    AnyView::new(view! {
        column(class = "wt-armor-plates", attr:data-testid = "armor-plates") {
            {blocks}
        }
    })
}

/// One plate: its number, thickness and strike angle, where it sits, and what the shell did to it.
fn plate_block(
    plate: &PlateDetail,
    flat: Option<FlatCheck>,
    server_angle: Option<Degrees>,
    viewport: ArmorViewportState,
) -> AnyView {
    let number = plate.index.number();
    let band = band_name(plate.band);
    let server = server_angle.map(|angle| {
        AnyView::new(view! {
            text(class = "wt-armor-plates__server") {{t!("ui.armor.plates.server_angle", angle = format!("{:.1}", angle.value())).into_owned()}}
        })
    });
    let place = format!("{} / {}", plate.zone, viewport.material_label(plate.material));

    let fuse = plate.simulated.filter(|simulated| simulated.fuse_armed).map(|_| {
        AnyView::new(view! {
            text(class = "wt-armor-plates__fuse") {{t!("ui.armor.plates.fuse_armed").into_owned()}}
        })
    });
    let verdict = plate.simulated.map(|simulated| {
        let (said, tone) = verdict_text(simulated.verdict, plate.angle);
        AnyView::new(view! {
            text(class = "wt-armor-plates__line", attr:data-tone = tone) {{said}}
        })
    });
    let flat = flat.map(|flat| {
        let key = if flat.penetrates() { "ui.armor.plates.flat_penetrates" } else { "ui.armor.plates.flat_shatters" };
        let said = t!(
            key,
            ammo = flat.ammo.display_name(),
            penetration = format!("{:.0}", flat.penetration.value()),
            thickness = format!("{:.0}", flat.thickness.value()),
        )
        .into_owned();
        line(said, if flat.penetrates() { "pen" } else { "shatter" }, "armor-plate-flat")
    });
    let detonation = plate.detonation.as_ref().map(|fuse| {
        AnyView::new(view! {
            text(class = "wt-armor-plates__line", attr:data-tone = "pen", attr:data-strong = "", attr:data-testid = "armor-plate-detonation") {
                {detonation_text(fuse)}
            }
        })
    });

    AnyView::new(view! {
        column(class = "wt-armor-plates__plate", attr:data-band = band, attr:data-testid = format!("armor-plate-{number}")) {
            row(class = "wt-armor-plates__head") {
                text(class = "wt-armor-plates__number") {{format!("#{number}")}}
                text(class = "wt-armor-plates__thickness") {{format!("{:.0} mm", plate.thickness.value())}}
                text {{format!("{:.1} deg", plate.angle.value())}}
                {server}
                {fuse}
            }
            text(class = "wt-armor-plates__where") {{place}}
            {verdict}
            {flat}
            {detonation}
        }
    })
}

fn band_name(band: AngleBand) -> &'static str {
    match band {
        AngleBand::Good => "good",
        AngleBand::Mid => "mid",
        AngleBand::Bad => "bad",
    }
}

/// What the shell did at a plate struck at `angle`, and the tone it reads in.
fn verdict_text(verdict: PlateVerdict, angle: Degrees) -> (String, &'static str) {
    let mm = |value: f32| format!("{value:.0}");
    match verdict {
        PlateVerdict::Overmatch { penetration, velocity } => (
            t!(
                "ui.armor.plates.overmatch",
                penetration = mm(penetration.value()),
                velocity = format!("{:.0}", velocity.value()),
            )
            .into_owned(),
            "pen",
        ),
        PlateVerdict::Penetrated { penetration, effective_thickness, velocity } => (
            t!(
                "ui.armor.plates.penetrated",
                penetration = mm(penetration.value()),
                effective = mm(effective_thickness.value()),
                velocity = format!("{:.0}", velocity.value()),
            )
            .into_owned(),
            "pen",
        ),
        PlateVerdict::Ricocheted => {
            (t!("ui.armor.plates.ricochet", angle = format!("{:.1}", angle.value())).into_owned(), "ricochet")
        }
        PlateVerdict::Shattered { penetration, effective_thickness } => (
            t!(
                "ui.armor.plates.shatter",
                penetration = mm(penetration.value()),
                effective = mm(effective_thickness.value()),
            )
            .into_owned(),
            "shatter",
        ),
    }
}

/// Where the fuse went off, and how far past the plate that armed it.
fn detonation_text(fuse: &Detonation) -> String {
    let distance = format!("{:.1}", fuse.travel.value());
    let number = fuse.armed_at.number();
    match &fuse.zone {
        Some(zone) => {
            t!("ui.armor.plates.detonation", zone = zone.clone(), distance = distance, number = number).into_owned()
        }
        None => t!("ui.armor.plates.detonation_past_armor", distance = distance, number = number).into_owned(),
    }
}

/// What an outcome reads as.
pub(super) fn describe_outcome(outcome: &ArcOutcome, viewport: ArmorViewportState) -> String {
    match outcome {
        ArcOutcome::NotSimulated => t!("ui.armor.arc_no_shell").into_owned(),
        ArcOutcome::Detonated { zone } => match zone {
            Some(zone) => t!("ui.armor.arc_detonated_in", zone = zone.clone()).into_owned(),
            None => t!("ui.armor.arc_detonated_past_armor").into_owned(),
        },
        ArcOutcome::Ricocheted { plate } => {
            t!("ui.armor.arc_ricochet", plate = describe_plate(plate, viewport)).into_owned()
        }
        ArcOutcome::Shattered { plate } => {
            t!("ui.armor.arc_shatter", plate = describe_plate(plate, viewport)).into_owned()
        }
        ArcOutcome::Stopped { plate } => {
            t!("ui.armor.arc_stopped", plate = describe_plate(plate, viewport)).into_owned()
        }
        ArcOutcome::Overpenetrated { fuse_armed: true } => t!("ui.armor.arc_overpen").into_owned(),
        ArcOutcome::Overpenetrated { fuse_armed: false } => t!("ui.armor.arc_overpen_unarmed").into_owned(),
    }
}

/// The plate a shell stopped at, or that nothing is known about it, which a cast whose plate list
/// is shorter than its simulation reports.
fn describe_plate(plate: &Option<StoppingPlate>, viewport: ArmorViewportState) -> String {
    match plate {
        Some(plate) => {
            let named = viewport.material_label(plate.material);
            format!("#{} {:.0}mm {}", plate.number, plate.thickness.value(), named)
        }
        None => t!("ui.armor.arc_plate_unknown").into_owned(),
    }
}

/// The tone an outcome reads in: the armor palette the viewers paint their verdicts in, where a
/// detonation is the result the reader was after.
pub(super) fn outcome_tone(outcome: &ArcOutcome) -> &'static str {
    match outcome {
        ArcOutcome::Detonated { .. } => "pen",
        ArcOutcome::Overpenetrated { .. } => "overpen",
        ArcOutcome::Ricocheted { .. } => "ricochet",
        ArcOutcome::Shattered { .. } | ArcOutcome::Stopped { .. } => "shatter",
        ArcOutcome::NotSimulated => "dim",
    }
}
