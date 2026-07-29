use egui::Color32;
use egui::RichText;
use rust_i18n::t;
use wowsunpack::game_types::TeamId;

use crate::replay::minimap_view::ENEMY_COLOR;
use crate::replay::minimap_view::FRIENDLY_COLOR;
use crate::replay::minimap_view::NEUTRAL_COLOR;
use crate::replay::timeline::TimelineEvent;
use crate::replay::timeline::TimelineEventKind;

pub(crate) fn event_color(team: TeamId, viewer_team: Option<TeamId>) -> Color32 {
    // Without a known viewer team every ship reads as an opponent, which is the
    // safer default: it never claims an enemy is an ally.
    match viewer_team {
        Some(viewer) if viewer == team => FRIENDLY_COLOR,
        _ => ENEMY_COLOR,
    }
}

/// Advantage events are viewer-relative and carry no absolute team.
pub(crate) fn advantage_color(is_friendly: bool) -> Color32 {
    if is_friendly { FRIENDLY_COLOR } else { ENEMY_COLOR }
}

pub(crate) const KIND_COUNT: usize = 8;

/// Stable index per event kind, used to key the filter's checkbox array.
pub(crate) fn kind_index(kind: &TimelineEventKind) -> usize {
    match kind {
        TimelineEventKind::HealthLost { .. } => 0,
        TimelineEventKind::Death { .. } => 1,
        TimelineEventKind::CapContested { .. } => 2,
        TimelineEventKind::CapFlipped { .. } => 3,
        TimelineEventKind::CapBeingCaptured { .. } => 4,
        TimelineEventKind::RadarUsed { .. } => 5,
        TimelineEventKind::AdvantageChanged { .. } => 6,
        TimelineEventKind::Disconnected { .. } => 7,
    }
}

fn kind_label_key(index: usize) -> &'static str {
    match index {
        0 => "ui.replay.timeline_kind_health_lost",
        1 => "ui.replay.timeline_kind_death",
        2 => "ui.replay.timeline_kind_cap_contested",
        3 => "ui.replay.timeline_kind_cap_flipped",
        4 => "ui.replay.timeline_kind_cap_being_captured",
        5 => "ui.replay.timeline_kind_radar",
        6 => "ui.replay.timeline_kind_advantage",
        _ => "ui.replay.timeline_kind_disconnected",
    }
}

/// Names an event can be searched by.
fn searchable_text(kind: &TimelineEventKind) -> (&str, &str) {
    match kind {
        TimelineEventKind::HealthLost { ship_name, player_name, .. }
        | TimelineEventKind::Death { ship_name, player_name, .. }
        | TimelineEventKind::RadarUsed { ship_name, player_name, .. }
        | TimelineEventKind::Disconnected { ship_name, player_name, .. } => (ship_name, player_name),
        TimelineEventKind::CapContested { cap_label, .. }
        | TimelineEventKind::CapFlipped { cap_label, .. }
        | TimelineEventKind::CapBeingCaptured { cap_label, .. } => (cap_label, ""),
        TimelineEventKind::AdvantageChanged { label, .. } => (label, ""),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TimelineFilter {
    pub kinds: [bool; KIND_COUNT],
    pub search: String,
}

impl Default for TimelineFilter {
    fn default() -> Self {
        Self { kinds: [true; KIND_COUNT], search: String::new() }
    }
}

impl TimelineFilter {
    pub fn matches(&self, event: &TimelineEvent) -> bool {
        if !self.kinds[kind_index(&event.kind)] {
            return false;
        }
        if self.search.is_empty() {
            return true;
        }
        let needle = self.search.to_lowercase();
        let (a, b) = searchable_text(&event.kind);
        a.to_lowercase().contains(&needle) || b.to_lowercase().contains(&needle)
    }
}

/// Color, label text, and hover text for one timeline row.
fn row_content(kind: &TimelineEventKind, viewer_team: Option<TeamId>) -> (Color32, String, String) {
    match kind {
        TimelineEventKind::HealthLost { ship_name, player_name, team, percent_lost, old_hp, new_hp, max_hp } => {
            let pct = (percent_lost * 100.0) as u32;
            (
                event_color(*team, viewer_team),
                format!("{} -{}% HP", ship_name, pct),
                format!(
                    "{} ({})\n{:.0}/{:.0} -> {:.0}/{:.0} HP",
                    ship_name, player_name, old_hp, max_hp, new_hp, max_hp
                ),
            )
        }
        TimelineEventKind::Death { ship_name, player_name, team, killer_ship, killer_player } => {
            let hover = if killer_ship.is_empty() {
                format!("{} ({})", ship_name, player_name)
            } else {
                format!("{} ({})\nKilled by {} ({})", ship_name, player_name, killer_ship, killer_player)
            };
            (event_color(*team, viewer_team), format!("{} destroyed", ship_name), hover)
        }
        TimelineEventKind::CapContested { cap_label, owner_team } => (
            owner_team.map(|team| event_color(team, viewer_team)).unwrap_or(NEUTRAL_COLOR),
            format!("{} contested", cap_label),
            String::new(),
        ),
        TimelineEventKind::CapFlipped { cap_label, capturer_team } => {
            (event_color(*capturer_team, viewer_team), format!("{} captured", cap_label), String::new())
        }
        TimelineEventKind::CapBeingCaptured { cap_label, capturer_team } => {
            (event_color(*capturer_team, viewer_team), format!("{} being captured", cap_label), String::new())
        }
        TimelineEventKind::RadarUsed { ship_name, player_name, team } => (
            event_color(*team, viewer_team),
            format!("{} used radar", ship_name),
            format!("{} ({})", ship_name, player_name),
        ),
        TimelineEventKind::AdvantageChanged { label, is_friendly } => {
            (advantage_color(*is_friendly), label.clone(), String::new())
        }
        TimelineEventKind::Disconnected { ship_name, player_name, team } => (
            event_color(*team, viewer_team),
            format!("{} disconnected", ship_name),
            format!("{} ({})", ship_name, player_name),
        ),
    }
}

/// Kind toggles live inside a menu rather than an inline row so this bar fits
/// both the inspector window and the renderer's narrow popup.
pub(crate) fn timeline_filter_bar(ui: &mut egui::Ui, filter: &mut TimelineFilter) {
    ui.horizontal(|ui| {
        ui.menu_button(t!("ui.replay.timeline_filter"), |ui| {
            if ui.button(t!("ui.replay.timeline_filter_all")).clicked() {
                filter.kinds = [true; KIND_COUNT];
            }
            if ui.button(t!("ui.replay.timeline_filter_none")).clicked() {
                filter.kinds = [false; KIND_COUNT];
            }
            ui.separator();
            for index in 0..KIND_COUNT {
                ui.checkbox(&mut filter.kinds[index], t!(kind_label_key(index)));
            }
        });
        ui.add(
            egui::TextEdit::singleline(&mut filter.search)
                .desired_width(140.0)
                .hint_text(t!("ui.replay.timeline_search_hint")),
        );
    });
}

/// Draws the filtered event list. `on_click` receives whichever event the user
/// clicked, letting each surface decide what a click means.
pub(crate) fn timeline_list(
    ui: &mut egui::Ui,
    events: &[TimelineEvent],
    filter: &TimelineFilter,
    viewer_team: Option<TeamId>,
    mut on_click: impl FnMut(&TimelineEvent),
) {
    let visible: Vec<&TimelineEvent> = events.iter().filter(|e| filter.matches(e)).collect();
    if visible.is_empty() {
        ui.label(t!("ui.replay.timeline_no_events"));
        return;
    }

    for event in visible {
        let secs = event.clock.seconds() as u32;
        let timestamp = format!("{:02}:{:02}", secs / 60, secs % 60);

        let clicked = ui
            .horizontal(|ui| {
                let mut clicked = ui.small_button(&timestamp).clicked();
                let (color, text, hover) = row_content(&event.kind, viewer_team);
                let label = ui.add(
                    egui::Label::new(RichText::new(text).color(color)).selectable(false).sense(egui::Sense::click()),
                );
                if !hover.is_empty() {
                    label.clone().on_hover_text(&hover);
                }
                if label.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                clicked |= label.clicked();
                clicked
            })
            .inner;

        if clicked {
            on_click(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::timeline::TimelineEvent;
    use crate::replay::timeline::TimelineEventKind;
    use wows_replays::types::ElapsedClock;
    use wowsunpack::game_types::TeamId;

    fn death(ship: &str, player: &str) -> TimelineEvent {
        TimelineEvent {
            clock: ElapsedClock(10.0),
            kind: TimelineEventKind::Death {
                ship_name: ship.to_owned(),
                player_name: player.to_owned(),
                team: TeamId::new(0),
                killer_ship: String::new(),
                killer_player: String::new(),
            },
        }
    }

    fn radar(ship: &str) -> TimelineEvent {
        TimelineEvent {
            clock: ElapsedClock(20.0),
            kind: TimelineEventKind::RadarUsed {
                ship_name: ship.to_owned(),
                player_name: "someone".to_owned(),
                team: TeamId::new(1),
            },
        }
    }

    #[test]
    fn default_filter_admits_every_kind() {
        let filter = TimelineFilter::default();
        assert!(filter.matches(&death("Yamato", "a")));
        assert!(filter.matches(&radar("Gearing")));
    }

    #[test]
    fn kind_indices_are_distinct_across_all_eight_kinds() {
        // A collision would make one checkbox silently toggle two kinds.
        let mut seen = [false; 8];
        for idx in [kind_index(&death("x", "y").kind), kind_index(&radar("x").kind)] {
            assert!(!seen[idx], "duplicate kind index {idx}");
            seen[idx] = true;
        }
    }

    #[test]
    fn disabling_a_kind_excludes_only_that_kind() {
        let mut filter = TimelineFilter::default();
        filter.kinds[kind_index(&radar("Gearing").kind)] = false;
        assert!(filter.matches(&death("Yamato", "a")));
        assert!(!filter.matches(&radar("Gearing")));
    }

    #[test]
    fn search_matches_ship_name_case_insensitively() {
        let filter = TimelineFilter { search: "yam".to_owned(), ..Default::default() };
        assert!(filter.matches(&death("Yamato", "someone")));
        assert!(!filter.matches(&death("Gearing", "someone")));
    }

    #[test]
    fn search_matches_player_name() {
        let filter = TimelineFilter { search: "bob".to_owned(), ..Default::default() };
        assert!(filter.matches(&death("Yamato", "Bobby")));
        assert!(!filter.matches(&death("Yamato", "alice")));
    }

    #[test]
    fn search_matches_cap_label_on_cap_events() {
        let filter = TimelineFilter { search: "a".to_owned(), ..Default::default() };
        let event = TimelineEvent {
            clock: ElapsedClock(5.0),
            kind: TimelineEventKind::CapFlipped { cap_label: "A".to_owned(), capturer_team: TeamId::new(0) },
        };
        assert!(filter.matches(&event));
    }
}
