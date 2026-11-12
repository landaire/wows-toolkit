//! What was fired at the ship on screen, salvo by salvo, under the penetration checker.
//!
//! The egui app's Incoming Fire panel (`replay/realtime_armor_viewer.rs`). Empty for a ship opened
//! from the catalogue, which nobody was shooting at.

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use rust_i18n::t;
use wows_replay_insights::timeline::PreExtractedHit;
use wows_replays::analyzer::decoder::HitType;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_toolkit_replay_data::replay_render::armor::IncomingContext;
use wows_toolkit_viewmodel::armor::health_strip::HealthStrip;
use wows_toolkit_viewmodel::armor::incoming::IncomingFilter;
use wows_toolkit_viewmodel::armor::incoming::IncomingSalvo;
use wows_toolkit_viewmodel::armor::incoming::ServerOutcome;
use wows_toolkit_viewmodel::armor::incoming::group_incoming;
use wowsunpack::game_params::types::ShellInfo;
use zgui::canvas::zgui_color::Color;
use zgui::component;
use zgui::css;
use zgui::custom::CustomElement;
use zgui::custom::CustomHandle;
use zgui::custom::CustomLayoutCx;
use zgui::custom::CustomMeasured;
use zgui::custom::ScenePainter;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::geom::Rect;
use zgui::geom::Size;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::CheckboxProps;
use zgui_ui::Checked;
use zgui_ui::ToggleProps;
use zgui_ui::ToggleSize;
use zgui_ui::TooltipContentProps;
use zgui_ui::TooltipProps;
use zgui_ui::TooltipTriggerProps;
use zgui_ui_primitives::Binding;

use crate::services::Services;
use crate::tabs::armor_viewer::viewport::Agreement;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;

const SHEET: &str = "wt-armor-incoming";

const CSS: &str = css!(
    ".wt-armor-incoming { display: flex; flex-direction: column; gap: 4px; padding-top: 8px;
        border-top: 1px solid var(--lui-color-border); }
    .wt-armor-incoming__title { font-size: 14px; font-weight: 700; }
    .wt-armor-incoming__dim { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-incoming__hint { padding: 4px 0; font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-incoming__attackers { display: flex; flex-wrap: wrap; gap: 4px; }
    .wt-armor-incoming__check { display: flex; align-items: center; gap: 6px; font-size: 12px; }
    .wt-armor-incoming__log { display: flex; flex-direction: column; gap: 4px; max-height: 220px;
        overflow-y: auto; }
    .wt-armor-incoming__salvo { display: flex; flex-direction: column; gap: 2px; }
    .wt-armor-incoming__salvo-head { display: flex; align-items: center; gap: 8px; }
    .wt-armor-incoming__who { flex: 1; font-size: 12px; }
    .wt-armor-incoming__shell { display: flex; gap: 4px; padding-left: 12px; font-size: 12px; }
    .wt-armor-incoming__said { font-size: 12px; color: var(--wt-color-text-dim); }
    .wt-armor-incoming__sim[data-standing=agrees] { color: var(--wt-color-ok); }
    .wt-armor-incoming__sim[data-standing=deferred] { color: var(--wt-color-warn); }
    .wt-armor-incoming__sim[data-standing=disagrees] { color: var(--wt-color-error); }
    .wt-armor-incoming__strip { width: 100%; height: 60px; cursor: pointer;
        --wt-strip-background: var(--wt-color-text-faint); --wt-strip-hit: var(--wt-color-armor-pen);
        --wt-strip-line: var(--wt-color-ok); --wt-strip-at: var(--wt-color-text-strong); }"
);

/// What was fired at the ship on screen, and which of it the log shows.
#[derive(Clone, Default)]
pub struct IncomingFire {
    pub context: IncomingContext,
    pub filter: IncomingFilter,
    /// The ship the log is about. A comparison run against any other hull would be about plating
    /// those shells never met, which a compare split would otherwise do.
    pub victim: Option<String>,
}

impl IncomingFire {
    /// The fire `context` describes, landing on `victim_param_index`'s ship.
    pub fn about(context: IncomingContext, victim_param_index: String) -> Self {
        Self { context, filter: IncomingFilter::default(), victim: Some(victim_param_index) }
    }

    /// Whether the log is about `param_index`'s ship.
    pub fn is_about(&self, param_index: &str) -> bool {
        self.victim.as_deref() == Some(param_index)
    }

    /// What a viewer showing `param_index`'s ship should compare, where it is the ship the log is
    /// about.
    pub fn to_compare(
        &self,
        param_index: Option<&str>,
    ) -> Option<(Vec<PreExtractedHit>, HashMap<GameParamId, ShellInfo>)> {
        self.is_about(param_index?).then(|| (self.context.taken.clone(), self.context.shells.clone()))
    }

    fn is_empty(&self) -> bool {
        self.context.taken.is_empty() && self.context.attackers.is_empty()
    }

    fn salvos(&self) -> Vec<IncomingSalvo> {
        let enemies: HashSet<EntityId> = self.context.attackers.keys().copied().collect();
        group_incoming(&self.context.taken, &self.filter, &enemies, &self.context.main_battery)
    }
}

/// A game clock as the log prints it: minutes and seconds into the battle.
fn clock_label(clock: GameClock) -> String {
    let seconds = clock.seconds().max(0.0);
    format!("{}:{:02}", (seconds / 60.0).floor() as i32, (seconds % 60.0) as i32)
}

/// What the server said a shell did.
fn hit_label(hit: &HitType) -> String {
    let outcome = ServerOutcome::from_shell_hit_type(&hit.shell_hit);
    match &outcome {
        ServerOutcome::Unknown(raw) => t!(outcome.label_key(), raw = raw.clone()).into_owned(),
        _ => t!(outcome.label_key()).into_owned(),
    }
}

fn standing_name(standing: Agreement) -> &'static str {
    match standing {
        Agreement::Agrees => "agrees",
        Agreement::Deferred => "deferred",
        Agreement::Disagrees => "disagrees",
    }
}

/// The incoming fire section: the health strip, the counts, the attacker filter and the salvo log.
#[component]
pub fn IncomingLog(
    incoming: RwSignal<IncomingFire, LocalStorage>,
    playback_at: Signal<Option<GameClock>, LocalStorage>,
    viewport: ArmorViewportState,
    on_seek: Option<UnsyncCallback<GameClock>>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = Services::current().locale;
    let seek = StoredValue::new_local(on_seek);
    let seek_to = move |clock: GameClock| {
        seek.with_value(|seek| {
            if let Some(seek) = seek {
                seek.run(clock);
            }
        })
    };

    let body = move || {
        if incoming.with(IncomingFire::is_empty) {
            return AnyView::new(());
        }
        let salvos = incoming.with(IncomingFire::salvos);
        let shells: usize = salvos.iter().map(|salvo| salvo.shells.len()).sum();
        let unchecked = viewport.sim_unchecked();
        let (chosen, attackers, secondaries, strip) = incoming.with(|fire| {
            let attackers: Vec<(EntityId, String)> =
                fire.context.attackers.iter().map(|(id, named)| (*id, named.clone())).collect();
            (fire.filter.attacker, attackers, fire.filter.secondaries, fire.context.health.clone())
        });

        let mut buttons =
            vec![attacker_button(incoming, None, t!("ui.armor.realtime.all_enemies").into_owned(), chosen)];
        buttons.extend(attackers.iter().map(|(id, named)| attacker_button(incoming, Some(*id), named.clone(), chosen)));

        let strip = strip.map(|strip| {
            let on_seek: Rc<dyn Fn(GameClock)> = Rc::new(seek_to);
            AnyView::new(view! { HealthStripView(strip = strip, at = playback_at, on_seek = on_seek) })
        });
        let unchecked = (unchecked > 0).then(|| {
            AnyView::new(view! {
                text(class = "wt-armor-incoming__dim") {{t!("ui.armor.realtime.sim_unchecked", count = unchecked).into_owned()}}
            })
        });
        let secondaries_checked = Signal::stored_local(if secondaries { Checked::Yes } else { Checked::No });
        let log = if salvos.is_empty() {
            AnyView::new(view! {
                text(class = "wt-armor-incoming__hint") {{t!("ui.armor.realtime.nothing_landed").into_owned()}}
            })
        } else {
            let blocks: Vec<AnyView> = salvos
                .iter()
                .enumerate()
                .map(|(index, salvo)| salvo_block(incoming, viewport, index, salvo, seek_to))
                .collect();
            AnyView::new(view! {
                column(class = "wt-armor-incoming__log", attr:data-testid = "armor-incoming-log") {
                    {blocks}
                }
            })
        };

        AnyView::new(view! {
            column(class = "wt-armor-incoming", attr:data-testid = "armor-incoming") {
                text(class = "wt-armor-incoming__title") {{locale.text("ui.armor.realtime.incoming_fire")}}
                {strip}
                text(class = "wt-armor-incoming__dim", attr:data-testid = "armor-incoming-tracked") {
                    {t!("ui.armor.realtime.tracked", salvos = salvos.len(), shells = shells).into_owned()}
                }
                {unchecked}
                text(class = "wt-armor-incoming__dim") {{locale.text("ui.armor.realtime.attacker_filter")}}
                row(class = "wt-armor-incoming__attackers") {
                    {buttons}
                }
                row(class = "wt-armor-incoming__check") {
                    Checkbox(
                        checked = Binding::controlled(secondaries_checked, move |checked: Checked| {
                            incoming.update(|fire| fire.filter.secondaries = checked == Checked::Yes)
                        }),
                        attr:data-testid = "armor-incoming-secondaries",
                        a11y:label = locale.text("ui.armor.realtime.show_secondaries")
                    )
                    text {{locale.text("ui.armor.realtime.show_secondaries")}}
                }
                {log}
            }
        })
    };

    view! {
        {body}
    }
}

/// One attacker the log can be narrowed to, or every enemy.
fn attacker_button(
    incoming: RwSignal<IncomingFire, LocalStorage>,
    attacker: Option<EntityId>,
    label: String,
    chosen: Option<EntityId>,
) -> AnyView {
    let test_id = match attacker {
        Some(attacker) => format!("armor-incoming-attacker-{}", attacker.raw()),
        None => "armor-incoming-attacker-all".to_owned(),
    };

    AnyView::new(view! {
        Toggle(
            size = ToggleSize::Sm,
            pressed = Binding::controlled(Signal::stored_local(attacker == chosen), move |_: bool| {
                incoming.update(|fire| fire.filter.attacker = attacker)
            }),
            label = label.clone(),
            attr:data-testid = test_id
        ) {
            {label.clone()}
        }
    })
}

/// One salvo: who fired it, when its first shell landed, and what each shell did, with this app's
/// own reading of it beside the server's.
fn salvo_block(
    incoming: RwSignal<IncomingFire, LocalStorage>,
    viewport: ArmorViewportState,
    index: usize,
    salvo: &IncomingSalvo,
    seek_to: impl Fn(GameClock) + Copy + 'static,
) -> AnyView {
    // A ship the roster did not name is one the frame had no row for, which reads as its entity
    // rather than as a blank.
    let who = incoming
        .with_untracked(|fire| fire.context.attackers.get(&salvo.attacker).cloned())
        .unwrap_or_else(|| t!("ui.armor.realtime.unknown_attacker", id = salvo.attacker.raw()).into_owned());
    let clock = salvo.first_clock;
    let seek_label = t!("ui.armor.realtime.seek_to_salvo").into_owned();
    let seek_tip = seek_label.clone();
    let shells: Vec<AnyView> = salvo
        .shells
        .iter()
        .enumerate()
        .map(|(shell_index, shell)| {
            let said = format!("{}  {}", clock_label(shell.clock), hit_label(&shell.hit_type));
            let sim = viewport.sim_agreement(shell.shot_id).map(|agreed| {
                AnyView::new(view! {
                    text(class = "wt-armor-incoming__sim", attr:data-standing = standing_name(agreed.standing)) {
                        {agreed.label.clone()}
                    }
                })
            });
            AnyView::new(view! {
                row(class = "wt-armor-incoming__shell", attr:data-testid = format!("armor-incoming-shell-{index}-{shell_index}")) {
                    text(class = "wt-armor-incoming__said") {{said}}
                    {sim}
                }
            })
        })
        .collect();

    AnyView::new(view! {
        column(class = "wt-armor-incoming__salvo") {
            row(class = "wt-armor-incoming__salvo-head") {
                text(class = "wt-armor-incoming__who") {{who}}
                Tooltip {
                    TooltipTrigger {
                        Button(
                            variant = ButtonVariant::Outline,
                            size = ButtonSize::Sm,
                            a11y:label = seek_label,
                            attr:data-testid = format!("armor-incoming-seek-{index}"),
                            on:click = move |_| seek_to(clock)
                        ) {
                            {clock_label(clock)}
                        }
                    }
                    TooltipContent {{seek_tip.clone()}}
                }
                text(class = "wt-armor-incoming__dim") {{t!("ui.armor.realtime.shells", count = salvo.shells.len()).into_owned()}}
            }
            {shells}
        }
    })
}

/// The colours the strip paints with, read from its style.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StripColors {
    background: Color,
    hit: Color,
    line: Color,
    at: Color,
}

/// The health strip: what landed along the bottom, the health line above it, and where playback
/// is.
struct HealthStripElement {
    strip: HealthStrip,
    at: Option<GameClock>,
    colors: Option<StripColors>,
}

impl CustomElement for HealthStripElement {
    fn layout(&mut self, cx: &mut CustomLayoutCx<'_>) -> CustomMeasured {
        let read = |name: &str| zgui_css::values::custom::color(cx.style, name);
        if let (Some(background), Some(hit), Some(line), Some(at)) =
            (read("wt-strip-background"), read("wt-strip-hit"), read("wt-strip-line"), read("wt-strip-at"))
        {
            self.colors = Some(StripColors { background, hit, line, at });
        }

        CustomMeasured {
            width: cx.known_width.unwrap_or(0.0),
            height: cx.known_height.unwrap_or(0.0),
            ..CustomMeasured::default()
        }
    }

    fn paint(&mut self, painter: &mut ScenePainter<'_>) {
        let Some(colors) = self.colors else { return };
        let scale = painter.scale();
        let size = painter.size();
        let (width, height) = (size.width.0, size.height.0);
        let rect = |x: f32, y: f32, w: f32, h: f32| {
            Rect::new(Point::new(DevicePx(x), DevicePx(y)), Size::new(DevicePx(w), DevicePx(h)))
        };
        // The ticks sit in a band along the bottom, and the line is drawn above it.
        let tick_height = height * 0.2;
        let line_height = height - tick_height;
        let (thin, thick) = (scale, 1.5 * scale);

        painter.fill(rect(0.0, 0.0, width, height), 0.0, colors.background.with_alpha(0.15));
        for along in &self.strip.hits {
            painter.fill(
                rect(along * width, height - tick_height, thin, tick_height),
                0.0,
                colors.hit.with_alpha(0.55),
            );
        }
        // A bar per step rather than a sloped path: what the strip says is where the health
        // stepped, so a shell that took a third of the ship reads as a drop.
        for pair in self.strip.line.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let (x0, x1) = (from.0 * width, to.0 * width);
            let y = line_height - from.1 * line_height;
            painter.fill(rect(x0, y, (x1 - x0).max(thin), thick), 0.0, colors.line);
            let next_y = line_height - to.1 * line_height;
            let (high, low) = if next_y < y { (next_y, y) } else { (y, next_y) };
            painter.fill(rect(x1, high, thick, (low - high).max(thin)), 0.0, colors.line);
        }
        if let Some(at) = self.at {
            painter.fill(rect(self.strip.along(at) * width, 0.0, thick, height), 0.0, colors.at);
        }
    }

    fn custom_properties_read(&self) -> Option<&'static [&'static str]> {
        Some(&["wt-strip-*"])
    }
}

/// This ship's health over the battle. A press along it moves playback there.
#[component]
fn HealthStripView(
    strip: HealthStrip,
    at: Signal<Option<GameClock>, LocalStorage>,
    on_seek: Rc<dyn Fn(GameClock)>,
) -> impl IntoView {
    let area = NodeRef::new();
    let seeking = strip.clone();
    let (element, handle): (_, CustomHandle<HealthStripElement>) =
        zgui::custom::custom(HealthStripElement { strip, at: at.get_untracked(), colors: None });
    let following = RenderEffect::new(move |_| {
        let at = at.get();
        handle.update(|element| element.at = at);
        handle.repaint();
    });
    on_cleanup_local(move || drop(following));

    view! {
        box(
            node_ref = area,
            class = "wt-armor-incoming__strip",
            attr:data-testid = "armor-incoming-health",
            on:pointer_down = move |ev| {
                // Where along the strip the press landed is the moment it names.
                let Some(bounds) = area.window_bounds() else { return };
                let scale = area.scale();
                let width = bounds.size.width.0 / scale;
                if width <= 0.0 {
                    return;
                }
                let along = (ev.position.x.0 - bounds.origin.x.0 / scale) / width;
                on_seek(seeking.clock_at(along));
            }
        ) {
            {element}
        }
    }
}

#[cfg(test)]
mod tests {
    use wows_replays::types::GameClock;

    use super::clock_label;

    #[test]
    fn the_log_prints_minutes_and_seconds() {
        assert_eq!(clock_label(GameClock(0.0)), "0:00");
        assert_eq!(clock_label(GameClock(65.4)), "1:05");
        assert_eq!(clock_label(GameClock(-3.0)), "0:00", "a clock before the battle reads as its start");
    }
}
