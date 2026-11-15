//! The splash readout: each zone the burst on screen reached, its plating and health, and what each
//! HE and SAP shell of the ships being compared does to that plating. The readout itself comes from
//! `wows_toolkit_armor::splash::splash_readout`.

use rust_i18n::t;
use wows_toolkit_armor::splash::ShellVerdict;
use wows_toolkit_armor::splash::SplashReadout;
use wows_toolkit_armor::splash::ZoneReadout;
use wows_toolkit_armor::splash::bursting_shell;
use wows_toolkit_viewmodel::armor::penetration::Ifhe;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

use crate::tabs::armor_viewer::panels::pen::PenetrationState;
use crate::tabs::armor_viewer::panels::pen::ammo_label;
use crate::tabs::armor_viewer::panels::pen::result_verdict;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;

const SHEET: &str = "wt-armor-splash";

const CSS: &str = css!(
    ".wt-armor-splash { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
    .wt-armor-splash__dim { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-splash__triangles { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-splash__heading { font-size: 12px; font-weight: 700; }
    .wt-armor-splash__zones { display: flex; flex-direction: column; gap: 4px; }
    .wt-armor-splash__zone { display: flex; flex-direction: column; }
    .wt-armor-splash__zone-head { display: flex; align-items: center; gap: 8px; min-width: 0; }
    .wt-armor-splash__zone-name { font-size: 12px; font-weight: 700; min-width: 0; overflow: hidden;
        white-space: nowrap; text-overflow: ellipsis; }
    .wt-armor-splash__direct { font-size: 12px; color: var(--wt-color-warn); }
    .wt-armor-splash__shell { display: flex; align-items: center; gap: 8px; width: 100%; padding: 1px 4px 1px 16px; }
    .wt-armor-splash__shell-name { flex: 1; min-width: 0; font-size: 12px; overflow: hidden; white-space: nowrap;
        text-overflow: ellipsis; }
    .wt-armor-splash__shell-pen { flex: none; width: 88px; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-splash__verdict { flex: none; font-size: 12px; }
    .wt-armor-splash__verdict[data-tone=ok] { color: var(--wt-color-ok); }
    .wt-armor-splash__verdict[data-tone=error] { color: var(--wt-color-error); }
    .wt-armor-splash__verdict[data-tone=warn] { color: var(--wt-color-warn); }
    .wt-armor-splash__verdict[data-tone=dim] { color: var(--wt-color-text-dim); }"
);

/// The readout for the burst `viewport` shows, read against the ships `pen` compares.
#[component]
pub fn SplashReadoutPanel(
    pen: RwSignal<PenetrationState, LocalStorage>,
    viewport: ArmorViewportState,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let body = move || {
        let (ships, ifhe) = pen.with(|pen| (pen.ships.clone(), Ifhe::from_enabled(pen.ifhe)));
        let has_bursting_shell = ships.iter().any(|ship| bursting_shell(&ship.shells).is_some());
        match viewport.splash_readout(&ships, ifhe) {
            Some(readout) => readout_view(&readout, has_bursting_shell),
            None => {
                let hint = t!("ui.armor.splash.hint").into_owned();
                AnyView::new(view! {
                    text(class = "wt-armor-splash__dim", attr:data-testid = "armor-splash-hint") {{hint}}
                })
            }
        }
    };

    view! {
        column(class = "wt-armor-splash", attr:data-testid = "armor-splash-readout") {{body}}
    }
}

/// The burst's reach and triangles, then its zones, or why there are none to read.
fn readout_view(readout: &SplashReadout, has_bursting_shell: bool) -> AnyView {
    let half_extent =
        t!("ui.armor.splash.half_extent", value = format!("{:.4}", readout.half_extent.value())).into_owned();
    let triangles = readout.triangles.map(|triangles| {
        let said = t!("ui.armor.splash.triangles", total = triangles.in_volume, penetrated = triangles.penetrated)
            .into_owned();
        view! { text(class = "wt-armor-splash__triangles", attr:data-testid = "armor-splash-triangles") {{said}} }
    });

    let zones = if readout.zones.is_empty() {
        let none = t!("ui.armor.splash.no_zones").into_owned();
        AnyView::new(
            view! { text(class = "wt-armor-splash__dim", attr:data-testid = "armor-splash-no-zones") {{none}} },
        )
    } else {
        let heading = t!("ui.armor.splash.zones").into_owned();
        // Without a bursting shell compared, each zone lists no verdicts; this says why.
        let no_shells = (!has_bursting_shell).then(|| {
            let said = t!("ui.armor.splash_no_shells").into_owned();
            view! { text(class = "wt-armor-splash__dim", attr:data-testid = "armor-splash-no-shells") {{said}} }
        });
        let rows: Vec<AnyView> = readout.zones.iter().enumerate().map(|(index, zone)| zone_view(index, zone)).collect();
        AnyView::new(view! {
            column(class = "wt-armor-splash__zones") {
                text(class = "wt-armor-splash__heading") {{heading}}
                {no_shells}
                {rows}
            }
        })
    };

    AnyView::new(view! {
        column(class = "wt-armor-splash") {
            text(class = "wt-armor-splash__dim", attr:data-testid = "armor-splash-half-extent") {{half_extent}}
            {triangles}
            {zones}
        }
    })
}

/// One zone: its name, whether the burst started in it, its plating and health, and a row per
/// compared shell under it.
fn zone_view(index: usize, zone: &ZoneReadout) -> AnyView {
    let name = zone.zone_name.clone();
    let direct = zone.is_direct_hit.then(|| {
        let said = t!("ui.armor.splash.direct_hit").into_owned();
        view! { text(class = "wt-armor-splash__direct", attr:data-testid = "armor-splash-direct-hit") {{said}} }
    });
    let thickness = zone.thickness.map(|thickness| {
        let said = format!("{:.0} mm", thickness.value());
        view! { text(class = "wt-armor-splash__dim") {{said}} }
    });
    let hp = zone.max_hp.map(|hp| {
        let said = t!("ui.armor.splash.hp", value = format!("{hp:.0}")).into_owned();
        view! { text(class = "wt-armor-splash__dim") {{said}} }
    });
    let verdicts: Vec<AnyView> = zone.verdicts.iter().map(verdict_view).collect();

    AnyView::new(view! {
        column(class = "wt-armor-splash__zone", attr:data-testid = format!("armor-splash-zone-{index}")) {
            row(class = "wt-armor-splash__zone-head") {
                text(class = "wt-armor-splash__zone-name") {{name}}
                {direct}
                {thickness}
                {hp}
            }
            {verdicts}
        }
    })
}

/// One compared shell against a zone's plating: whose and what it is, what it brings, and what
/// happens.
fn verdict_view(verdict: &ShellVerdict) -> AnyView {
    let (key, tone) = result_verdict(verdict.result);
    let name = format!("{} {}", verdict.ship_name, ammo_label(&verdict.ammo_type, verdict.caliber));
    let pen = match verdict.penetration {
        Some(pen) if verdict.ifhe_applied => format!("{:.0}mm {}", pen.value(), t!("ui.armor.pen.ifhe")),
        Some(pen) => format!("{:.0}mm", pen.value()),
        None => "-".to_owned(),
    };
    let said = t!(key).into_owned();

    AnyView::new(view! {
        row(class = "wt-armor-splash__shell") {
            text(class = "wt-armor-splash__shell-name") {{name}}
            text(class = "wt-armor-splash__shell-pen") {{pen}}
            text(class = "wt-armor-splash__verdict", attr:data-tone = tone.name()) {{said}}
        }
    })
}
