//! The menu a right-click on a ship opens: its trail, its ranges, and its armor.

use rust_i18n::t;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_replays::types::EntityId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_replay_data::replay_render::armor::has_armor_to_show;
use wows_toolkit_replay_data::replay_render::playback::Playback;
use wows_toolkit_replay_data::replay_render::ships::ALL_RANGES;
use wows_toolkit_replay_data::replay_render::view::WindowPoint;
use zgui::component;
use zgui::css;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::OverlayState;
use zgui_ui::PopoverContentProps;
use zgui_ui::PopoverProps;
use zgui_ui_primitives::Align;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Placement;
use zgui_ui_primitives::Side;

use crate::icons::AppIconProps;
use crate::windows::renderer::options::CSS as OPTIONS_CSS;
use crate::windows::renderer::options::CheckRowProps;
use crate::windows::renderer::options::SHEET as OPTIONS_SHEET;
use crate::windows::renderer::session::RendererSession;

const SHEET: &str = "wt-renderer-ship-menu";

const CSS: &str = css!(
    ".wt-renderer-ship-menu__area { position: absolute; left: 0; top: 0; right: 0; bottom: 0; pointer-events: none; }
    .wt-renderer-ship-menu__anchor { position: absolute; width: 0; height: 0; }
    .wt-renderer-ship-menu { display: flex; flex-direction: column; gap: 4px; width: 220px; padding: 8px; }
    .wt-renderer-ship-menu__header { display: flex; flex-direction: row; align-items: center; justify-content: space-between; }
    .wt-renderer-ship-menu__player { font-size: 12px; font-weight: 700; }
    .wt-renderer-ship-menu__ranges { padding-top: 4px; font-size: 11px; color: var(--wt-color-text-dim); }"
);

/// The per-ship menu, opened where the right-click landed. Closed by its own button, a press
/// outside it, or Escape.
#[component]
pub fn ShipMenuView(session: RendererSession) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    install_stylesheet(OPTIONS_SHEET, OPTIONS_CSS);
    let open = Signal::derive_local(move || session.menu.with(Option::is_some));

    view! {
        Popover(open = Binding::controlled(open, move |open: bool| {
            if !open {
                session.menu.set(None);
            }
        })) {
            MenuAnchor(session = session)
            PopoverContent(
                placement = Placement::new(Side::Bottom, Align::Start),
                offset = 0.0,
                class = "wt-renderer-ship-menu",
                attr:data-testid = "replay-renderer-ship-menu"
            ) {
                ShipMenuBody(session = session)
            }
        }
    }
}

/// A point over the viewport where the menu was asked for, which the menu is anchored to.
#[component]
fn MenuAnchor(session: RendererSession) -> impl IntoView {
    let area = NodeRef::new();
    let trigger = OverlayState::current().map_or_else(NodeRef::new, |state| state.trigger());
    // Read against the area as the menu opens: a right-click is in window CSS pixels, the area's
    // box in device pixels.
    let offset = move |at: WindowPoint| {
        let scale = area.scale();
        let origin =
            area.window_bounds().map_or((0.0, 0.0), |bounds| (bounds.origin.x.0 / scale, bounds.origin.y.0 / scale));
        (at.x - origin.0, at.y - origin.1)
    };
    let at = move || session.menu.with(|menu| menu.as_ref().map(|menu| menu.at)).map(offset);

    view! {
        box(class = "wt-renderer-ship-menu__area", node_ref = area) {
            box(
                class = "wt-renderer-ship-menu__anchor",
                node_ref = trigger,
                style:left = move || at().map(|(left, _)| format!("{left}px")),
                style:top = move || at().map(|(_, top)| format!("{top}px")),
                a11y:hidden = true
            )
        }
    }
}

/// What the menu holds for the ship it is open on.
#[component]
fn ShipMenuBody(session: RendererSession) -> impl IntoView {
    move || {
        session.menu.get().map(|menu| {
            view! { ShipMenuItems(session = session, entity_id = menu.entity_id, player = menu.player_name) }
                .into_view()
        })
    }
}

/// One ship's trail, armor and ranges.
#[component]
fn ShipMenuItems(session: RendererSession, entity_id: EntityId, player: String) -> impl IntoView {
    let trail_player = player.clone();
    let trail_shown = Signal::derive_local(move || session.choices.with(|choices| choices.trail_shown(&trail_player)));
    let ranges_player = player.clone();
    let all_on =
        Signal::derive_local(move || session.choices.with(|choices| choices.ranges_for(&ranges_player) == ALL_RANGES));

    let set_trail_player = player.clone();
    let set_trail = UnsyncCallback::new(move |shown: bool| {
        session.choose(|choices, drawn, options| choices.set_trail_shown(&set_trail_player, shown, drawn, options));
    });
    let only_trail_player = player.clone();
    let only_trail =
        move || session.choose(|choices, drawn, options| choices.only_trail(&only_trail_player, drawn, options));
    let every_player = player.clone();
    let every_range = move || {
        let on = !all_on.get_untracked();
        session.choose(|choices, _, options| choices.set_every_range_for(&every_player, on, options));
    };
    let only_player = player.clone();
    let only_ranges = move || session.choose(|choices, _, options| choices.only_ranges(&only_player, options));
    let all_ranges = move || session.choose(|choices, drawn, options| choices.all_ranges(drawn, options));
    let every_label = move || {
        if all_on.get() { t!("ui.renderer.context.disable_all") } else { t!("ui.renderer.context.enable_all") }
            .into_owned()
    };
    let switch = |test_id: &'static str,
                  label_key: &'static str,
                  read: fn(&ShipConfigFilter) -> bool,
                  write: fn(&mut ShipConfigFilter, bool)| {
        let read_player = player.clone();
        let write_player = player.clone();
        view! {
            CheckRow(
                label = t!(label_key).into_owned(),
                test_id = test_id.to_owned(),
                checked = Signal::derive_local(move || session.choices.with(|choices| read(&choices.ranges_for(&read_player)))),
                on_change = UnsyncCallback::new(move |on: bool| {
                    session.choose(|choices, _, options| choices.set_ranges_for(&write_player, |filter| write(filter, on), options));
                })
            )
        }
    };

    view! {
        row(class = "wt-renderer-ship-menu__header") {
            text(class = "wt-renderer-ship-menu__player") {{player.clone()}}
            Button(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::IconXs,
                attr:data-testid = "replay-renderer-ship-menu-close",
                on:click = move |_| session.menu.set(None)
            ) {
                AppIcon(glyph = Glyph::X)
            }
        }
        CheckRow(
            label = t!("ui.renderer.context.show_trail").into_owned(),
            test_id = "replay-renderer-ship-trail".to_owned(),
            checked = trail_shown,
            on_change = set_trail
        )
        Show(when = move || armor_offered(session, entity_id)) {
            Button(
                variant = ButtonVariant::Outline,
                size = ButtonSize::Sm,
                attr:data-testid = "replay-renderer-ship-armor",
                on:click = move |_| session.show_armor(entity_id)
            ) {
                {t!("ui.renderer.context.realtime_armor").into_owned()}
            }
        }
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Sm,
            attr:data-testid = "replay-renderer-ship-only-trail",
            on:click = move |_| only_trail()
        ) {
            {t!("ui.renderer.context.disable_other_trails").into_owned()}
        }
        text(class = "wt-renderer-ship-menu__ranges") {{t!("ui.renderer.context.ranges").into_owned()}}
        {switch("replay-renderer-range-detection", "ui.renderer.context.detection", |filter| filter.detection, |filter, on| filter.detection = on)}
        {switch("replay-renderer-range-main", "ui.renderer.context.main_battery", |filter| filter.main_battery, |filter, on| filter.main_battery = on)}
        {switch("replay-renderer-range-secondary", "ui.renderer.context.secondary", |filter| filter.secondary_battery, |filter, on| filter.secondary_battery = on)}
        {switch("replay-renderer-range-torpedo", "ui.renderer.context.torpedo", |filter| filter.torpedo, |filter, on| filter.torpedo = on)}
        {switch("replay-renderer-range-radar", "ui.renderer.context.radar", |filter| filter.radar, |filter, on| filter.radar = on)}
        {switch("replay-renderer-range-hydro", "ui.renderer.context.hydro", |filter| filter.hydro, |filter, on| filter.hydro = on)}
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Sm,
            attr:data-testid = "replay-renderer-every-range",
            on:click = move |_| every_range()
        ) {
            {every_label}
        }
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Sm,
            attr:data-testid = "replay-renderer-only-ranges",
            on:click = move |_| only_ranges()
        ) {
            {t!("ui.renderer.context.disable_other_ranges").into_owned()}
        }
        Button(
            variant = ButtonVariant::Outline,
            size = ButtonSize::Sm,
            attr:data-testid = "replay-renderer-all-ranges",
            on:click = move |_| all_ranges()
        ) {
            {t!("ui.renderer.context.enable_all_ranges").into_owned()}
        }
    }
}

/// Whether the armor viewer has anything to show for `entity_id`: hits were read for it, and the
/// frame on screen names its ship. Reactive.
fn armor_offered(session: RendererSession, entity_id: EntityId) -> bool {
    let Some(track) = session.track() else { return false };
    let Some(game_data) = session.game_data() else { return false };
    let at = session.playback.with(Playback::at);
    let Some(commands) = track.commands(at) else { return false };

    session.events.with(|events| has_armor_to_show(events.shots(), commands, entity_id, &game_data))
}
