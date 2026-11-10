//! What the renderer's controls read of a viewport.

use nsh_core::prelude::*;
use rust_i18n::t;
use wows_minimap_renderer::VideoCodec;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_replay_insights::timeline::EventTone;
use wows_replay_insights::timeline::KIND_COUNT;
use wows_replay_insights::timeline::format_timeline_event;
use wows_replay_insights::timeline::kind_label_key;
use wows_replay_insights::timeline::row_text;
use wows_replay_insights::timeline::row_tone;
use wows_toolkit_replay_data::playback;
use wows_toolkit_replay_data::playback::ALL_RANGES;
use wows_toolkit_replay_data::playback::INKS;
use wows_toolkit_replay_data::playback::LAYER_TOGGLES;
use wows_toolkit_replay_data::playback::MAX_NIB;
use wows_toolkit_replay_data::playback::MIN_NIB;
use wows_toolkit_replay_data::playback::SPEEDS;
use wows_toolkit_replay_data::playback::ShipPick;
use wows_toolkit_viewmodel::ship_catalog::tier_roman;
use wt_collab_client::drawing::Tool;

use crate::renderer::viewer::Phase;
use crate::renderer::viewer::Viewer;

/// Where a viewport is in reading its replay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum RendererStatus {
    #[default]
    Baking,
    Ready,
    Failed,
}

/// A drawing tool, as the renderer's toolbar and shortcuts name it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum RendererTool {
    #[default]
    Arrow,
    Freehand,
    Eraser,
    Line,
    Circle,
    Rectangle,
    Triangle,
    Measurement,
    /// A ship of the class and side `placeShip` names.
    Ship,
}

/// One range circle a ship can show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum RendererRange {
    #[default]
    Detection,
    MainBattery,
    Secondary,
    Torpedo,
    Radar,
    Hydro,
}

/// Which side an event row is drawn as.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PropEnum)]
pub enum EventSide {
    Friendly,
    Enemy,
    #[default]
    Neutral,
}

/// How far an export has got.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct ExportView {
    /// `None` while the encoder starts and there is nothing to measure.
    pub fraction: Option<f64>,
    /// What the progress line says.
    pub label: String,
}

/// What a hover over the frame reads, placed in points from its top left.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct ReadingView {
    pub x: f64,
    pub y: f64,
    pub title: String,
    pub lines: Vec<String>,
    pub detail: Option<String>,
}

/// A peer's pointer, in points from the frame's top left.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct CursorView {
    pub x: f64,
    pub y: f64,
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
}

/// One ship found by the ship search.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct ShipMatchView {
    /// What `nameShip` takes.
    pub index: u32,
    pub label: String,
}

/// A message about something the viewport did.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct NoticeView {
    /// Counts up, so the same text said twice is shown twice.
    pub id: u32,
    pub text: String,
}

/// Everything the transport, the overlays and the drawing tools read.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct RendererView {
    pub id: u32,
    /// The frame surface the viewport draws on.
    pub surface: String,
    /// Changes whenever anything here does.
    pub revision: u32,
    pub title: String,
    pub status: RendererStatus,
    pub failure: Option<String>,
    pub playing: bool,
    pub frame: u32,
    pub last_frame: u32,
    /// Where playback is and how long the battle runs, as the game's clock.
    pub clock: String,
    pub speed_index: u32,
    pub zoom: f64,
    pub whole_map: bool,
    /// Where the battle began and ended along the seek bar, as fractions.
    pub marks: Vec<f64>,
    pub has_previous_event: bool,
    pub has_next_event: bool,
    pub export: Option<ExportView>,
    pub export_failure: Option<String>,
    /// Whether an export would be encoded in software without being asked
    /// to, which the reader is told before the wait.
    pub software_encode: bool,
    /// Whether ctrl is held over the frame, which puts the shortcut sheet up.
    pub ctrl_held: bool,
    pub reading: Option<ReadingView>,
    pub cursors: Vec<CursorView>,
    /// Whether a session is running, which the drawing tools need.
    pub in_session: bool,
    pub tool: Option<RendererTool>,
    /// The species and side of a ship in hand.
    pub placing: Option<PlacingView>,
    /// Which of the offered inks is in use.
    pub ink: Option<u32>,
    pub nib: f64,
    pub can_thin: bool,
    pub can_thicken: bool,
    pub can_undo: bool,
    /// Whether the picked shape is a placed ship, and what it is named.
    pub picked_ship: Option<PickedShipView>,
    pub ship_query: String,
    pub ship_matches: Vec<ShipMatchView>,
    pub notice: Option<NoticeView>,
}

/// The ship in hand.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct PlacingView {
    /// The species' short name, `DD` to `SS`.
    pub short: String,
    pub friendly: bool,
}

/// The placed ship picked out.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct PickedShipView {
    pub name: Option<String>,
}

/// One layer switch.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct LayerView {
    pub id: String,
    pub label: String,
    pub on: bool,
}

/// One codec the encoder offers here.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct CodecView {
    pub value: String,
    pub label: String,
}

/// The settings popover: the layers and what an export is encoded with.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct SettingsView {
    pub layers: Vec<LayerView>,
    pub prefer_cpu: bool,
    pub include_pre_battle: bool,
    /// The codec chosen; `None` lets the encoder pick.
    pub codec: Option<String>,
    /// The automatic choice, naming the codec it would pick.
    pub auto_label: String,
    pub codecs: Vec<CodecView>,
}

/// One event row of the timeline.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct EventRowView {
    /// Seconds into the battle.
    pub at: f64,
    pub time: String,
    pub label: String,
    pub hover: String,
    pub side: EventSide,
}

/// One kind of event the timeline can show.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct EventKindView {
    pub index: u32,
    pub label: String,
    pub on: bool,
}

/// The event timeline popover.
#[derive(Debug, Clone, PartialEq, JsObject)]
pub struct TimelineView {
    /// Whether the second walk of the battle has finished.
    pub read: bool,
    pub any_events: bool,
    pub search: String,
    pub kinds: Vec<EventKindView>,
    pub rows: Vec<EventRowView>,
    /// The rows as text, for the copy button.
    pub copy_text: String,
}

/// One range switch of the ship menu.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct RangeSwitchView {
    pub range: RendererRange,
    pub label: String,
    pub on: bool,
}

/// The menu a right-click on a ship opens.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct ShipMenuView {
    pub player: String,
    pub trail: bool,
    pub ranges: Vec<RangeSwitchView>,
    pub all_ranges_on: bool,
}

/// The labels the renderer's controls show, in the reader's language.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct RendererText {
    pub speeds: Vec<String>,
    pub jump_to_start: String,
    pub previous_event: String,
    pub back_10s: String,
    pub play: String,
    pub pause: String,
    pub forward_10s: String,
    pub next_event: String,
    pub jump_to_end: String,
    pub speed: String,
    pub zoom_reset: String,
    pub export_video: String,
    pub export_clipboard: String,
    pub settings: String,
    pub export_settings: String,
    pub prefer_cpu: String,
    pub prefer_cpu_hint: String,
    pub include_pre_battle: String,
    pub include_pre_battle_hint: String,
    pub codec: String,
    pub save_defaults: String,
    pub timeline: String,
    pub timeline_all: String,
    pub timeline_none: String,
    pub timeline_search: String,
    pub timeline_copy: String,
    pub timeline_copy_hint: String,
    pub timeline_copied: String,
    pub timeline_reading: String,
    pub timeline_no_matches: String,
    pub timeline_no_events: String,
    pub baking: String,
    pub failed: String,
    pub preparing_export: String,
    pub software_encode_title: String,
    pub software_encode_body: String,
    pub software_encode_go: String,
    pub suppress_warning: String,
    pub cancel: String,
    pub annotations: String,
    pub undo: String,
    pub nib: String,
    pub thinner: String,
    pub thicker: String,
    pub no_ship: String,
    pub ship_hint: String,
    pub shortcuts_title: String,
    pub shortcuts_tools: String,
    pub shortcuts_actions: String,
    pub tools: Vec<ToolText>,
    pub actions: Vec<ChordText>,
    pub show_trail: String,
    pub only_trail: String,
    pub ranges: String,
    pub enable_all: String,
    pub disable_all: String,
    pub only_ranges: String,
    pub all_ranges: String,
    pub pop_out: String,
    pub pop_in: String,
    pub watching: String,
    pub waiting_for_frames: String,
}

/// One drawing tool: what it is called and the chord that takes it up.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct ToolText {
    /// `None` for the selector, which escape goes back to.
    pub tool: Option<RendererTool>,
    pub label: String,
    pub chord: Option<String>,
}

/// One chord of the shortcut sheet's actions.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
pub struct ChordText {
    pub chord: String,
    pub label: String,
}

/// The drawing tool `tool` names, for a ship of the species and side given.
pub fn tool_of(tool: RendererTool) -> Tool {
    match tool {
        RendererTool::Arrow => Tool::Arrow,
        RendererTool::Freehand => Tool::Freehand,
        RendererTool::Eraser => Tool::Eraser,
        RendererTool::Line => Tool::Line,
        RendererTool::Circle => Tool::Circle { filled: false },
        RendererTool::Rectangle => Tool::Rectangle { filled: false },
        RendererTool::Triangle => Tool::Triangle { filled: false },
        RendererTool::Measurement => Tool::Measurement,
        RendererTool::Ship => Tool::Ship { species: playback::SPECIES[0].1.to_owned(), friendly: true, yaw: 0.0 },
    }
}

fn name_of(tool: &Tool) -> Option<RendererTool> {
    Some(match tool {
        Tool::None => return None,
        Tool::Ship { .. } => RendererTool::Ship,
        Tool::Freehand => RendererTool::Freehand,
        Tool::Eraser => RendererTool::Eraser,
        Tool::Line => RendererTool::Line,
        Tool::Circle { .. } => RendererTool::Circle,
        Tool::Rectangle { .. } => RendererTool::Rectangle,
        Tool::Triangle { .. } => RendererTool::Triangle,
        Tool::Arrow => RendererTool::Arrow,
        Tool::Measurement => RendererTool::Measurement,
    })
}

fn hex(color: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

/// What `viewer` shows, at `revision`.
pub fn of(id: u32, surface: &str, viewer: &Viewer, revision: u32) -> RendererView {
    let track = viewer.track();
    let (status, failure) = match viewer.phase() {
        Phase::Baking => (RendererStatus::Baking, None),
        Phase::Ready(_) => (RendererStatus::Ready, None),
        Phase::Failed(reason) => (RendererStatus::Failed, Some(reason.clone())),
    };
    let clock = track.map_or_else(String::new, |track| {
        format!("{} / {}", playback::mmss(track.elapsed_at(viewer.at())), playback::mmss(track.elapsed_at(track.last())))
    });
    let scale = viewer.viewport().map_or(1.0, |viewport| viewport.scale);
    let export = viewer.export().map(|progress| {
        if progress.total == 0 {
            return ExportView { fraction: None, label: t!("ui.renderer.preparing_export").into_owned() };
        }
        ExportView {
            fraction: Some(progress.done.min(progress.total) as f64 / progress.total as f64),
            label: format!("{} ({}/{})", t!(progress.stage.label()), progress.done, progress.total),
        }
    });
    let settings = viewer.export_settings();
    let drawing = viewer.drawing();

    RendererView {
        id,
        surface: surface.to_owned(),
        revision,
        title: viewer.title().to_owned(),
        status,
        failure,
        playing: viewer.is_playing(),
        frame: viewer.at() as u32,
        last_frame: track.map_or(0, |track| track.last() as u32),
        clock,
        speed_index: playback::speed_index(viewer.speed()) as u32,
        zoom: f64::from(viewer.map_view().zoom()),
        whole_map: viewer.map_view().is_whole_map(),
        marks: track.map(|track| track.battle_marks().into_iter().map(f64::from).collect()).unwrap_or_default(),
        has_previous_event: viewer.previous_event().is_some(),
        has_next_event: viewer.next_event().is_some(),
        export,
        export_failure: viewer.export_failure().map(str::to_owned),
        software_encode: !settings.prefer_cpu
            && playback::must_use_cpu(&settings, playback::encoder_status()),
        ctrl_held: viewer.ctrl_held(),
        reading: viewer.reading().map(|reading| ReadingView {
            x: f64::from(reading.at.0 as f32 / scale),
            y: f64::from(reading.at.1 as f32 / scale),
            title: reading.title.clone(),
            lines: reading.lines.clone(),
            detail: reading.detail.clone(),
        }),
        cursors: viewer
            .peer_cursors()
            .into_iter()
            .map(|cursor| CursorView {
                x: f64::from(cursor.at.0),
                y: f64::from(cursor.at.1),
                name: cursor.name,
                color: hex(cursor.color),
            })
            .collect(),
        in_session: viewer.collab().is_active(),
        tool: name_of(drawing.tool()),
        placing: match drawing.tool() {
            Tool::Ship { species, friendly, .. } => playback::SPECIES
                .iter()
                .find(|(_, name)| name == species)
                .map(|(short, _)| PlacingView { short: (*short).to_owned(), friendly: *friendly }),
            _ => None,
        },
        ink: INKS.iter().position(|(_, ink)| *ink == drawing.color()).map(|index| index as u32),
        nib: f64::from(drawing.width()),
        can_thin: drawing.width() > MIN_NIB,
        can_thicken: drawing.width() < MAX_NIB,
        can_undo: viewer.can_undo(),
        picked_ship: viewer.picked_ship().map(|name| PickedShipView { name }),
        ship_query: viewer.ship_query().to_owned(),
        ship_matches: viewer
            .matched_ships()
            .iter()
            .enumerate()
            .map(|(index, (_, ship))| ShipMatchView {
                index: index as u32,
                label: format!("{} {}", tier_roman(ship.tier), ship.display_name),
            })
            .collect(),
        notice: viewer.notice().map(|notice| NoticeView { id: notice.id, text: notice.text.clone() }),
    }
}

/// The settings popover's contents.
pub fn settings(viewer: &Viewer) -> SettingsView {
    let options = viewer.options();
    let dead = viewer.show_dead_ships();
    let export = viewer.export_settings();
    let status = playback::encoder_status();

    SettingsView {
        layers: LAYER_TOGGLES
            .iter()
            .map(|toggle| LayerView {
                id: toggle.id.to_owned(),
                label: t!(toggle.label).into_owned(),
                on: (toggle.read)(options, dead),
            })
            .collect(),
        prefer_cpu: export.prefer_cpu,
        include_pre_battle: export.include_pre_battle,
        codec: export.codec.map(|codec| codec.as_str().to_owned()),
        auto_label: format!(
            "{} ({})",
            t!("ui.renderer.settings.codec_auto"),
            status.best_codec(export.prefer_cpu).display_name()
        ),
        codecs: status
            .supported_codecs()
            .map(|codec| CodecView { value: codec.as_str().to_owned(), label: codec.display_name().to_owned() })
            .collect(),
    }
}

/// The codec `value` names.
pub fn codec_of(value: &str) -> Option<VideoCodec> {
    VideoCodec::ALL.into_iter().find(|codec| codec.as_str() == value)
}

/// The event timeline's contents.
pub fn timeline(viewer: &Viewer) -> TimelineView {
    let filter = viewer.event_filter();
    let team = viewer.viewer_team();
    let visible = viewer.visible_events();

    TimelineView {
        read: viewer.events_read(),
        any_events: !viewer.events().is_empty(),
        search: filter.search.clone(),
        kinds: (0..KIND_COUNT)
            .map(|index| EventKindView {
                index: index as u32,
                label: t!(kind_label_key(index)).into_owned(),
                on: filter.kinds[index],
            })
            .collect(),
        rows: visible
            .iter()
            .map(|event| {
                let (label, hover) = row_text(&event.kind);
                EventRowView {
                    at: f64::from(event.clock.seconds()),
                    time: playback::mmss(event.clock.seconds()),
                    label,
                    hover,
                    side: match row_tone(&event.kind, team) {
                        EventTone::Friendly => EventSide::Friendly,
                        EventTone::Enemy => EventSide::Enemy,
                        EventTone::Neutral => EventSide::Neutral,
                    },
                }
            })
            .collect(),
        copy_text: visible.iter().map(|event| format_timeline_event(event)).collect::<Vec<_>>().join("\n"),
    }
}

/// Which switch of a filter `range` is.
pub fn range_switch(filter: &mut ShipConfigFilter, range: RendererRange) -> &mut bool {
    match range {
        RendererRange::Detection => &mut filter.detection,
        RendererRange::MainBattery => &mut filter.main_battery,
        RendererRange::Secondary => &mut filter.secondary_battery,
        RendererRange::Torpedo => &mut filter.torpedo,
        RendererRange::Radar => &mut filter.radar,
        RendererRange::Hydro => &mut filter.hydro,
    }
}

/// The ship menu for `pick`.
pub fn ship_menu(viewer: &Viewer, pick: &ShipPick) -> ShipMenuView {
    let mut filter = viewer.ranges_for(&pick.player_name);
    let ranges = [
        (RendererRange::Detection, "ui.renderer.context.detection"),
        (RendererRange::MainBattery, "ui.renderer.context.main_battery"),
        (RendererRange::Secondary, "ui.renderer.context.secondary"),
        (RendererRange::Torpedo, "ui.renderer.context.torpedo"),
        (RendererRange::Radar, "ui.renderer.context.radar"),
        (RendererRange::Hydro, "ui.renderer.context.hydro"),
    ]
    .into_iter()
    .map(|(range, label)| RangeSwitchView { range, label: t!(label).into_owned(), on: *range_switch(&mut filter, range) })
    .collect();

    ShipMenuView {
        player: pick.player_name.clone(),
        trail: viewer.trail_shown(&pick.player_name),
        ranges,
        all_ranges_on: filter == ALL_RANGES,
    }
}

/// The controls' labels.
pub fn text() -> RendererText {
    let tr = |key: &str| t!(key).into_owned();
    let tool = |tool: Option<RendererTool>, key: &str, chord: Option<&str>| ToolText {
        tool,
        label: tr(key),
        chord: chord.map(str::to_owned),
    };

    RendererText {
        speeds: SPEEDS.iter().map(|speed| playback::speed_label(*speed)).collect(),
        jump_to_start: tr("ui.renderer.controls.jump_to_start"),
        previous_event: tr("ui.renderer.controls.previous_event"),
        back_10s: tr("ui.renderer.controls.back_10s"),
        play: tr("ui.replay.renderer.play"),
        pause: tr("ui.replay.renderer.pause"),
        forward_10s: tr("ui.renderer.controls.forward_10s"),
        next_event: tr("ui.renderer.controls.next_event"),
        jump_to_end: tr("ui.renderer.controls.jump_to_end"),
        speed: tr("ui.renderer.controls.speed"),
        zoom_reset: tr("ui.buttons.reset"),
        export_video: tr("ui.replay.renderer.export_video"),
        export_clipboard: tr("ui.replay.renderer.export_clipboard"),
        settings: tr("ui.renderer.settings.title"),
        export_settings: tr("ui.renderer.settings.export_settings"),
        prefer_cpu: tr("ui.renderer.settings.prefer_cpu"),
        prefer_cpu_hint: tr("ui.renderer.settings.prefer_cpu_tooltip"),
        include_pre_battle: tr("ui.renderer.settings.include_pre_battle"),
        include_pre_battle_hint: tr("ui.renderer.settings.include_pre_battle_tooltip"),
        codec: tr("ui.renderer.settings.codec"),
        save_defaults: tr("ui.renderer.settings.save_defaults"),
        timeline: tr("ui.renderer.settings.event_timeline"),
        timeline_all: tr("ui.replay.timeline_filter_all"),
        timeline_none: tr("ui.replay.timeline_filter_none"),
        timeline_search: tr("ui.replay.timeline_search_hint"),
        timeline_copy: tr("ui.buttons.copy"),
        timeline_copy_hint: tr("ui.replay.timeline_export_tooltip"),
        timeline_copied: tr("ui.replay.timeline_copied"),
        timeline_reading: tr("ui.replay.timeline_parsing"),
        timeline_no_matches: tr("ui.replay.timeline_no_matches"),
        timeline_no_events: tr("ui.replay.timeline_no_events"),
        baking: tr("ui.replay.renderer.baking"),
        failed: tr("ui.replay.renderer.failed"),
        preparing_export: tr("ui.renderer.preparing_export"),
        software_encode_title: tr("ui.windows.software_encode"),
        software_encode_body: tr("ui.dialogs.software_encode_message"),
        software_encode_go: tr("ui.buttons.continue_"),
        suppress_warning: tr("ui.labels.suppress_warning"),
        cancel: tr("ui.buttons.cancel"),
        annotations: tr("ui.renderer.annotations.title"),
        undo: tr("ui.renderer.annotations.undo"),
        nib: tr("ui.renderer.annotations.nib"),
        thinner: tr("ui.renderer.annotations.thinner"),
        thicker: tr("ui.renderer.annotations.thicker"),
        no_ship: tr("ui.renderer.annotations.no_ship"),
        ship_hint: tr("ui.renderer.annotations.ship_hint"),
        shortcuts_title: tr("ui.renderer.annotations.shortcuts_title"),
        shortcuts_tools: tr("ui.renderer.annotations.shortcuts_tools"),
        shortcuts_actions: tr("ui.renderer.annotations.shortcuts_actions"),
        tools: vec![
            tool(None, "ui.renderer.annotations.tool_select", None),
            tool(Some(RendererTool::Arrow), "ui.renderer.annotations.tool_arrow", Some("Ctrl+1")),
            tool(Some(RendererTool::Freehand), "ui.renderer.annotations.tool_freehand", Some("Ctrl+2")),
            tool(Some(RendererTool::Eraser), "ui.renderer.annotations.tool_eraser", Some("Ctrl+3")),
            tool(Some(RendererTool::Line), "ui.renderer.annotations.tool_line", Some("Ctrl+4")),
            tool(Some(RendererTool::Circle), "ui.renderer.annotations.tool_circle", Some("Ctrl+5")),
            tool(Some(RendererTool::Rectangle), "ui.renderer.annotations.tool_rectangle", Some("Ctrl+6")),
            tool(Some(RendererTool::Triangle), "ui.renderer.annotations.tool_triangle", Some("Ctrl+7")),
            tool(Some(RendererTool::Measurement), "ui.renderer.annotations.tool_measurement", Some("Ctrl+M")),
        ],
        actions: [
            ("Ctrl+Z", "ui.renderer.annotations.action_undo"),
            ("Ctrl+Click", "ui.renderer.annotations.action_multi_select"),
            ("[ / ]", "ui.renderer.annotations.action_stroke_width"),
            ("Del", "ui.renderer.annotations.action_delete"),
            ("Esc", "ui.renderer.annotations.action_cancel"),
        ]
        .into_iter()
        .map(|(chord, key)| ChordText { chord: chord.to_owned(), label: tr(key) })
        .collect(),
        show_trail: tr("ui.renderer.context.show_trail"),
        only_trail: tr("ui.renderer.context.disable_other_trails"),
        ranges: tr("ui.renderer.context.ranges"),
        enable_all: tr("ui.renderer.context.enable_all"),
        disable_all: tr("ui.renderer.context.disable_all"),
        only_ranges: tr("ui.renderer.context.disable_other_ranges"),
        all_ranges: tr("ui.renderer.context.enable_all_ranges"),
        pop_out: tr("ui.replay.renderer.pop_out"),
        pop_in: tr("ui.replay.renderer.pop_in"),
        watching: tr("ui.collab.watching"),
        waiting_for_frames: tr("ui.collab.waiting_for_frames"),
    }
}
