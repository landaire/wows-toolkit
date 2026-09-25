use egui::Color32;
use egui::RichText;
use rust_i18n::t;
use wowsunpack::game_types::TeamId;

use crate::replay::minimap_view::ENEMY_COLOR;
use crate::replay::minimap_view::FRIENDLY_COLOR;
use crate::replay::minimap_view::NEUTRAL_COLOR;
use crate::replay::timeline::EventTone;
use crate::replay::timeline::KIND_COUNT;
use crate::replay::timeline::TimelineEvent;
use crate::replay::timeline::TimelineEventKind;
pub(crate) use crate::replay::timeline::TimelineFilter;
use crate::replay::timeline::kind_label_key;
use crate::replay::timeline::row_text;
use crate::replay::timeline::row_tone;

/// The colour a tone is painted in this app.
fn tone_color(tone: EventTone) -> Color32 {
    match tone {
        EventTone::Friendly => FRIENDLY_COLOR,
        EventTone::Enemy => ENEMY_COLOR,
        EventTone::Neutral => NEUTRAL_COLOR,
    }
}

/// Color, label text, and hover text for one timeline row.
fn row_content(kind: &TimelineEventKind, viewer_team: Option<TeamId>) -> (Color32, String, String) {
    let (label, hover) = row_text(kind);
    (tone_color(row_tone(kind, viewer_team)), label, hover)
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
    use crate::replay::timeline::kind_index;
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
        let kinds = [
            TimelineEventKind::HealthLost {
                ship_name: "Yamato".to_owned(),
                player_name: "a".to_owned(),
                team: TeamId::new(0),
                percent_lost: 0.1,
                old_hp: 100.0,
                new_hp: 90.0,
                max_hp: 100.0,
            },
            TimelineEventKind::Death {
                ship_name: "Yamato".to_owned(),
                player_name: "a".to_owned(),
                team: TeamId::new(0),
                killer_ship: String::new(),
                killer_player: String::new(),
            },
            TimelineEventKind::CapContested { cap_label: "A".to_owned(), cap_index: 0, owner_team: None },
            TimelineEventKind::CapFlipped { cap_label: "A".to_owned(), cap_index: 0, capturer_team: TeamId::new(0) },
            TimelineEventKind::CapBeingCaptured {
                cap_label: "A".to_owned(),
                cap_index: 0,
                capturer_team: TeamId::new(0),
            },
            TimelineEventKind::RadarUsed {
                ship_name: "Gearing".to_owned(),
                player_name: "a".to_owned(),
                team: TeamId::new(0),
            },
            TimelineEventKind::AdvantageChanged { label: "adv".to_owned(), is_friendly: true },
            TimelineEventKind::Disconnected {
                ship_name: "Yamato".to_owned(),
                player_name: "a".to_owned(),
                team: TeamId::new(0),
            },
        ];

        let mut seen = [false; KIND_COUNT];
        for kind in &kinds {
            let idx = kind_index(kind);
            assert!(idx < KIND_COUNT, "index {idx} out of range");
            assert!(!seen[idx], "duplicate kind index {idx}");
            seen[idx] = true;
        }
        assert!(seen.iter().all(|&s| s), "not all eight kind indices were covered");
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
            kind: TimelineEventKind::CapFlipped {
                cap_label: "A".to_owned(),
                cap_index: 0,
                capturer_team: TeamId::new(0),
            },
        };
        assert!(filter.matches(&event));
    }
}
