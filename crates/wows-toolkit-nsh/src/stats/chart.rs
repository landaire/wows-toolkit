//! One chart of the Stats tab: its model, the frame surface it is drawn on,
//! and the thread that draws it and turns the surface's pointer input into
//! panning and zooming.

use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::mpsc;

use base64::Engine as _;
use nsh_core::prelude::*;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::chart_model::ChartId;
use wows_toolkit_viewmodel::stats::chart_model::ChartModel;
use wows_toolkit_viewmodel::stats::match_group_display_name;
use wows_toolkit_viewmodel::stats::plot::Colors;
use wows_toolkit_viewmodel::stats::plot::Plot;
use wows_toolkit_viewmodel::stats::plot::PlotColor;
use wows_toolkit_viewmodel::stats::plot::PlotPoint;
use wows_toolkit_viewmodel::stats::plot_image;
use wows_toolkit_viewmodel::stats::plot_image::PlotImage;

use super::error::ChartImageError;
use super::ids::MatchGroup;
use super::ids::ParamIdText;
use super::ids::ScopeName;
use super::ids::ViewChartId;
use super::snapshot::DivisionChoice;
use super::snapshot::ModeOption;
use crate::frames;
use crate::frames::AlphaMode;
use crate::frames::Frame;
use crate::frames::PixelSize;
use crate::frames::PointerButton;
use crate::frames::SurfaceId;
use crate::frames::SurfaceInput;
use crate::frames::ViewSize;
use crate::text::text_with;

/// How much one wheel step zooms, as the GPUI chart zooms per scroll event.
const WHEEL_ZOOM: f32 = 1.1;

/// The colours charts are drawn in, from the view's theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChartPalette {
    pub(crate) background: PlotColor,
    pub(crate) colors: Colors,
}

static PALETTE: LazyLock<Mutex<ChartPalette>> = LazyLock::new(|| {
    let grey = PlotColor::new(0.6, 0.6, 0.6, 1.0);
    Mutex::new(ChartPalette {
        background: plot_image::COPY_BACKGROUND,
        colors: Colors { axis: grey, grid: grey.opacity(0.4), text: grey },
    })
});

/// A statistic as the view names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PropEnum)]
pub enum StatChoice {
    BaseXp,
    Damage,
    Frags,
    PersonalRating,
    RawXp,
    SpottingDamage,
    WinRate,
}

impl StatChoice {
    pub(crate) fn from_stat(stat: ChartableStat) -> Self {
        match stat {
            ChartableStat::BaseXp => Self::BaseXp,
            ChartableStat::Damage => Self::Damage,
            ChartableStat::Frags => Self::Frags,
            ChartableStat::PersonalRating => Self::PersonalRating,
            ChartableStat::RawXp => Self::RawXp,
            ChartableStat::SpottingDamage => Self::SpottingDamage,
            ChartableStat::WinRate => Self::WinRate,
        }
    }

    pub(crate) fn stat(self) -> ChartableStat {
        match self {
            Self::BaseXp => ChartableStat::BaseXp,
            Self::Damage => ChartableStat::Damage,
            Self::Frags => ChartableStat::Frags,
            Self::PersonalRating => ChartableStat::PersonalRating,
            Self::RawXp => ChartableStat::RawXp,
            Self::SpottingDamage => ChartableStat::SpottingDamage,
            Self::WinRate => ChartableStat::WinRate,
        }
    }
}

/// A statistic a chart offers.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct StatOption {
    pub stat: StatChoice,
    pub label: String,
    pub selected: bool,
}

/// A ship a chart's filter lists.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ShipOption {
    pub ship_id: ParamIdText,
    pub name: String,
    /// The name folded for search.
    pub search_key: String,
    pub selected: bool,
}

/// Everything a chart panel shows besides the plot itself.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct ChartSnapshot {
    pub id: ViewChartId,
    /// The frame surface the plot is drawn on.
    pub surface: String,
    pub title: String,
    pub value_label: String,
    pub line: bool,
    pub stats: Vec<StatOption>,
    pub running: bool,
    pub combined: bool,
    pub show_values: bool,
    pub overrides: bool,
    pub division: DivisionChoice,
    /// The session's modes, chosen as this chart's own filter chooses them.
    pub modes: Vec<ModeOption>,
    pub ships: Vec<ShipOption>,
    /// `3 of 5 selected`.
    pub selection: String,
    /// Whether every listed ship is plotted.
    pub all_selected: bool,
    /// Nothing would be drawn.
    pub empty: bool,
    /// The view has been dragged or zoomed.
    pub view_moved: bool,
}

/// A chart and the thread drawing it.
pub(crate) struct Chart {
    model: Arc<Mutex<ChartModel>>,
    surface: SurfaceId,
    messages: mpsc::Sender<Message>,
    view_moved: tokio::sync::watch::Receiver<bool>,
}

impl Chart {
    /// Starts drawing `model` on the surface named `surface`.
    pub(crate) fn start(model: ChartModel, surface: SurfaceId) -> Self {
        let model = Arc::new(Mutex::new(model));
        let (messages, receiver) = mpsc::channel();
        let (moved_sender, view_moved) = tokio::sync::watch::channel(false);

        let shown = frames::surface(&surface);
        if let Some(mut input) = shown.take_input() {
            let forward = messages.clone();
            // Ends when the surface is released and its last view drops.
            let spawned = std::thread::Builder::new().name(format!("{}-input", surface.as_str())).spawn(move || {
                while let Some(event) = input.blocking_recv() {
                    if forward.send(Message::Input(event)).is_err() {
                        return;
                    }
                }
            });
            if let Err(error) = spawned {
                log::warn!("stats: chart input is not forwarded: {error}");
            }
        }

        let producer = Producer { model: Arc::clone(&model), surface: shown, size: None, drag: None, pinch: None, moved: moved_sender };
        let spawned = std::thread::Builder::new().name(surface.as_str().to_owned()).spawn(move || producer.run(&receiver));
        if let Err(error) = spawned {
            log::warn!("stats: chart {} is not drawn: {error}", surface.as_str());
        }

        Self { model, surface, messages, view_moved }
    }

    /// Runs `change` on the model and draws the result.
    pub(crate) fn update<T>(&self, change: impl FnOnce(&mut ChartModel) -> T) -> T {
        let result = {
            let mut model = self.model.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            change(&mut model)
        };
        self.redraw();
        result
    }

    pub(crate) fn read<T>(&self, read: impl FnOnce(&ChartModel) -> T) -> T {
        let model = self.model.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        read(&model)
    }

    pub(crate) fn redraw(&self) {
        // The producer only stops once the chart is dropped.
        let _ = self.messages.send(Message::Redraw);
    }

    pub(crate) fn view_moved(&self) -> tokio::sync::watch::Receiver<bool> {
        self.view_moved.clone()
    }

    pub(crate) fn snapshot(&self) -> ChartSnapshot {
        self.read(|model| snapshot(model, self.surface.as_str(), *self.view_moved.borrow()))
    }

    /// The chart drawn at the size it shows at, as a PNG `data:` URL.
    pub(crate) fn image(&self) -> Result<String, ChartImageError> {
        let Some(ViewSize { size, scale }) = frames::surface(&self.surface).view_size() else {
            return Err(ChartImageError::NotShown);
        };
        let palette = *PALETTE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

        let snapshot = self.read(PlotSnapshot::of).ok_or(ChartImageError::Empty)?;
        let PlotImage { width, height, mut rgba } = snapshot
            .render(palette.colors, plot_image::COPY_BACKGROUND, size, scale.get())
            .ok_or(ChartImageError::Empty)?;
        plot_image::unpremultiply(&mut rgba);

        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|source| ChartImageError::Encode { source })?;
            writer.write_image_data(&rgba).map_err(|source| ChartImageError::Encode { source })?;
        }

        Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(encoded)))
    }
}

impl Drop for Chart {
    fn drop(&mut self) {
        // The producer may already have stopped.
        let _ = self.messages.send(Message::Stop);
        frames::release(&self.surface);
    }
}

/// Replaces the colours every chart is drawn in and draws them again.
pub(crate) fn set_palette(palette: ChartPalette) {
    *PALETTE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = palette;
}

/// Parses a CSS colour as the theme's tokens write them.
pub(crate) fn parse_color(css: &str) -> Option<PlotColor> {
    let rgba = nsh_core::Rgba::from(nsh_core::Color::parse(css)?);
    Some(PlotColor::new(rgba.r, rgba.g, rgba.b, rgba.a))
}

/// The surface a chart of `scope` is drawn on.
pub(crate) fn surface_id(scope: &ScopeName, id: ChartId) -> SurfaceId {
    SurfaceId::new(format!("stats-{scope}-chart-{}", id.0))
}

fn snapshot(model: &ChartModel, surface: &str, view_moved: bool) -> ChartSnapshot {
    let selected = model.selected_ships();
    let played = model.played();
    let selected_count = played.iter().filter(|(id, _)| selected.contains(id)).count();
    let filters = model.effective_filters();

    ChartSnapshot {
        id: ViewChartId::from_chart(model.id()).expect("chart ids are checked to fit when a chart is restored"),
        surface: surface.to_owned(),
        title: model.tab_title(),
        value_label: model.value_label(),
        line: model.mode() == ChartMode::Line,
        stats: model
            .selectable_stats()
            .into_iter()
            .map(|stat| StatOption { stat: StatChoice::from_stat(stat), label: stat.label(), selected: stat == model.stat() })
            .collect(),
        running: model.running() || model.combined(),
        combined: model.combined(),
        show_values: model.show_values(),
        overrides: model.overrides_tab(),
        division: DivisionChoice::from_filter(filters.division),
        modes: model
            .offered_modes()
            .into_iter()
            .map(|mode| ModeOption {
                label: match_group_display_name(&mode).to_owned(),
                selected: filters.game_modes.contains(&mode),
                id: MatchGroup::new(mode),
            })
            .collect(),
        ships: played
            .iter()
            .map(|(id, name)| ShipOption {
                ship_id: ParamIdText::from_id(*id),
                name: name.clone(),
                search_key: wows_toolkit_viewmodel::search::normalize_ship_search(name),
                selected: selected.contains(id),
            })
            .collect(),
        selection: text_with(
            "ui.stats.ship_selection_count",
            &[("selected", &selected_count.to_string()), ("total", &played.len().to_string())],
        ),
        all_selected: selected_count == played.len(),
        empty: model.series().is_empty() && model.bars().is_empty(),
        view_moved,
    }
}

/// What one frame of a chart plots, copied out of its model so the frame is
/// drawn without holding it.
struct PlotSnapshot {
    series: Vec<wows_toolkit_viewmodel::stats::chart::ChartSeries>,
    bars: Vec<wows_toolkit_viewmodel::stats::chart::ChartBar>,
    x_label: String,
    y_label: String,
    show_values: bool,
    view: wows_toolkit_viewmodel::stats::plot::PlotView,
}

impl PlotSnapshot {
    /// `None` when the chart has nothing to plot.
    fn of(model: &ChartModel) -> Option<Self> {
        let series = model.series();
        let bars = model.bars();
        if series.is_empty() && bars.is_empty() {
            return None;
        }

        Some(Self {
            series,
            bars,
            x_label: model.x_label(),
            y_label: model.value_label(),
            show_values: model.show_values(),
            view: model.view(),
        })
    }

    /// Premultiplied RGBA, `None` for a size no image can be made at.
    fn render(&self, colors: Colors, background: PlotColor, size: PixelSize, scale: f32) -> Option<PlotImage> {
        let plot = Plot {
            series: &self.series,
            bars: &self.bars,
            x_label: &self.x_label,
            y_label: &self.y_label,
            show_values: self.show_values,
            view: self.view,
        };
        plot_image::render(&plot, colors, background, size.width, size.height, scale)
    }
}

enum Message {
    Input(SurfaceInput),
    Redraw,
    Stop,
}

/// Draws one chart whenever it, its size or its view changes.
struct Producer {
    model: Arc<Mutex<ChartModel>>,
    surface: Arc<frames::Surface>,
    /// The surface's size in device pixels and its scale.
    size: Option<(PixelSize, f32)>,
    /// Where the pointer last was while a drag pans the plot, in logical
    /// pixels.
    drag: Option<PlotPoint>,
    /// The pinch scale last applied, while a pinch is in progress.
    pinch: Option<f32>,
    moved: tokio::sync::watch::Sender<bool>,
}

impl Producer {
    fn run(mut self, messages: &mpsc::Receiver<Message>) {
        let mut last_pointer: Option<PlotPoint> = None;
        while let Ok(first) = messages.recv() {
            // Everything already queued is applied before one draw: every
            // redraw reads the same latest model, and a stop is honored at
            // once rather than behind a backlog of obsolete frames.
            let mut redraw = false;
            for message in std::iter::once(first).chain(std::iter::from_fn(|| messages.try_recv().ok())) {
                redraw |= match message {
                    Message::Stop => return,
                    Message::Redraw => true,
                    Message::Input(input) => self.input(input, &mut last_pointer),
                };
            }
            if redraw {
                self.draw();
            }
        }
    }

    /// Applies one input event. Returns whether the chart needs drawing.
    fn input(&mut self, input: SurfaceInput, last_pointer: &mut Option<PlotPoint>) -> bool {
        let scale = self.size.map_or(1.0, |(_, scale)| scale);
        let logical = |x: f32, y: f32| PlotPoint::new(x / scale, y / scale);

        match input {
            SurfaceInput::Resized { size, scale } => {
                self.size = Some((size, scale.get()));
                true
            }
            SurfaceInput::PointerDown { at, button: PointerButton::Primary, clicks, .. } => {
                if !self.pannable() {
                    return false;
                }
                if clicks >= 2 {
                    self.drag = None;
                    return self.change_view(|view| view.reset());
                }
                self.drag = Some(logical(at.x, at.y));
                false
            }
            SurfaceInput::PointerMoved { at, .. } => {
                let position = logical(at.x, at.y);
                *last_pointer = Some(position);
                let Some(last) = self.drag else { return false };

                self.drag = Some(position);
                self.change_view(|view| view.drag(position.x - last.x, position.y - last.y))
            }
            SurfaceInput::PointerUp { button: PointerButton::Primary, .. } | SurfaceInput::PointerLeft => {
                self.drag = None;
                false
            }
            SurfaceInput::Wheel { at, dy, .. } => {
                let dy = dy.get();
                if dy == 0.0 || !self.pannable() {
                    return false;
                }
                let cursor = at.map(|at| logical(at.x, at.y)).or(*last_pointer).unwrap_or_default();
                let factor = if dy < 0.0 { WHEEL_ZOOM } else { 1.0 / WHEEL_ZOOM };
                self.change_view(|view| view.zoom_about(factor, cursor))
            }
            SurfaceInput::Pinch { scale: gesture } => {
                let gesture = gesture.get();
                if !self.pannable() {
                    return false;
                }
                let previous = self.pinch.replace(gesture).unwrap_or(1.0);
                if previous <= 0.0 || gesture <= 0.0 {
                    return false;
                }
                let cursor = last_pointer.unwrap_or_else(|| self.centre());
                self.change_view(|view| view.zoom_about(gesture / previous, cursor))
            }
            SurfaceInput::Focus { focused: false } => {
                self.pinch = None;
                false
            }
            SurfaceInput::PointerDown { .. } | SurfaceInput::PointerUp { .. } | SurfaceInput::Key { .. } | SurfaceInput::Focus { .. } => false,
        }
    }

    /// A bar chart is a comparison of whole bars, not a curve to be read
    /// into: the egui bar plot refuses drag, zoom and scroll for the same
    /// reason (`ui/session_stats_chart.rs`).
    fn pannable(&self) -> bool {
        let model = self.model.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        model.mode() == ChartMode::Line
    }

    fn centre(&self) -> PlotPoint {
        let Some((size, scale)) = self.size else { return PlotPoint::default() };
        PlotPoint::new(size.width as f32 / scale / 2.0, size.height as f32 / scale / 2.0)
    }

    fn change_view(&mut self, change: impl FnOnce(&mut wows_toolkit_viewmodel::stats::plot::PlotView)) -> bool {
        let moved = {
            let mut model = self.model.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            change(model.view_mut());
            !model.view().is_default()
        };
        self.moved.send_if_modified(|current| std::mem::replace(current, moved) != moved);
        true
    }

    fn draw(&mut self) {
        let Some((size, scale)) = self.size else { return };
        if size.is_empty() {
            return;
        }

        let palette = *PALETTE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let snapshot = {
            let model = self.model.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            // A view reset by a mode or statistic change says so.
            let moved = !model.view().is_default();
            self.moved.send_if_modified(|current| std::mem::replace(current, moved) != moved);
            PlotSnapshot::of(&model)
        };
        // Rasterized after the model is let go, so a slow frame never holds
        // up the calls that change the chart.
        let rendered = snapshot.and_then(|snapshot| snapshot.render(palette.colors, palette.background, size, scale));

        let Some(PlotImage { width, height, rgba }) = rendered else {
            self.surface.clear();
            return;
        };
        match Frame::new(PixelSize { width, height }, AlphaMode::Premultiplied, rgba) {
            Ok(frame) => self.surface.present(frame),
            Err(error) => log::warn!("stats: a chart frame was refused: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wows_replays::types::GameParamId;
    use wows_toolkit_viewmodel::stats::PerGameStat;
    use wows_toolkit_viewmodel::stats::StatsFilters;

    use crate::frames::Modifiers;
    use crate::frames::PixelPoint;

    fn games() -> Vec<PerGameStat> {
        (0..6)
            .map(|index| PerGameStat {
                ship_name: "Yamato".to_owned(),
                ship_id: GameParamId::from(1u64),
                game_time: format!("2026-01-0{index}"),
                sort_key: format!("2026-01-0{index}"),
                player_id: 7,
                damage: 50_000 + index * 10_000,
                spotting_damage: 0,
                frags: 1,
                raw_xp: 1_000,
                base_xp: 800,
                is_win: index % 2 == 0,
                is_loss: index % 2 == 1,
                is_draw: false,
                is_div: false,
                match_group: "pvp".to_owned(),
                achievements: Vec::new(),
            })
            .collect()
    }

    fn wait_for_frame(surface: &frames::Surface, after: Option<Arc<Frame>>) -> Arc<Frame> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(frame) = surface.latest()
                && after.as_ref().is_none_or(|after| !Arc::ptr_eq(after, &frame))
            {
                return frame;
            }
            assert!(std::time::Instant::now() < deadline, "the chart drew no frame");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_chart_draws_at_the_size_its_view_reports_and_pans_with_a_drag() {
        let mut model = ChartModel::new(ChartId(0));
        model.set_games(&games(), &StatsFilters::default());
        let id = SurfaceId::new("stats-test-chart-pan");
        let chart = Chart::start(model, id.clone());
        let surface = frames::surface(&id);

        surface.send_input(SurfaceInput::Resized { size: PixelSize { width: 640, height: 400 }, scale: frames::ScaleFactor::new(2.0) });
        let first = wait_for_frame(&surface, None);
        assert_eq!(first.size, PixelSize { width: 640, height: 400 });
        assert!(first.pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[..3] != first.pixels[..3]), "the plot marks the frame");

        let mut moved = chart.view_moved();
        let modifiers = Modifiers::default();
        surface.send_input(SurfaceInput::PointerDown { at: PixelPoint { x: 200.0, y: 200.0 }, button: PointerButton::Primary, clicks: 1, modifiers });
        surface.send_input(SurfaceInput::PointerMoved { at: PixelPoint { x: 260.0, y: 220.0 }, modifiers });
        surface.send_input(SurfaceInput::PointerUp { at: PixelPoint { x: 260.0, y: 220.0 }, button: PointerButton::Primary, modifiers });
        let panned = wait_for_frame(&surface, Some(Arc::clone(&first)));
        assert_ne!(panned.pixels, first.pixels, "the panned plot draws differently");
        tokio::runtime::Builder::new_current_thread().build().expect("a runtime").block_on(async {
            moved.wait_for(|moved| *moved).await.expect("the chart reports its view moved");
        });
        let view = chart.read(ChartModel::view);
        assert_eq!(view.pan, PlotPoint::new(30.0, 10.0), "device pixels pan by logical pixels");

        surface.send_input(SurfaceInput::PointerDown { at: PixelPoint { x: 10.0, y: 10.0 }, button: PointerButton::Primary, clicks: 2, modifiers });
        tokio::runtime::Builder::new_current_thread().build().expect("a runtime").block_on(async {
            moved.wait_for(|moved| !*moved).await.expect("a double click resets the view");
        });
    }

    #[test]
    fn a_bar_chart_ignores_the_wheel() {
        let mut model = ChartModel::new(ChartId(0));
        model.set_games(&games(), &StatsFilters::default());
        model.set_mode(ChartMode::Bar);
        let id = SurfaceId::new("stats-test-chart-bars");
        let chart = Chart::start(model, id.clone());
        let surface = frames::surface(&id);
        surface.send_input(SurfaceInput::Resized { size: PixelSize { width: 300, height: 200 }, scale: frames::ScaleFactor::new(1.0) });
        wait_for_frame(&surface, None);

        surface.send_input(SurfaceInput::Wheel {
            at: Some(PixelPoint { x: 100.0, y: 100.0 }),
            dx: frames::ScrollSteps::new(0.0),
            dy: frames::ScrollSteps::new(-1.0),
            modifiers: Modifiers::default(),
        });
        chart.redraw();
        wait_for_frame(&surface, None);

        assert!(chart.read(|model| model.view().is_default()));
    }

    #[test]
    fn a_copied_chart_is_a_png_of_its_shown_size() {
        let mut model = ChartModel::new(ChartId(0));
        model.set_games(&games(), &StatsFilters::default());
        let id = SurfaceId::new("stats-test-chart-copy");
        let chart = Chart::start(model, id.clone());
        assert!(matches!(chart.image(), Err(ChartImageError::NotShown)));

        frames::surface(&id).send_input(SurfaceInput::Resized { size: PixelSize { width: 320, height: 200 }, scale: frames::ScaleFactor::new(1.0) });
        let url = chart.image().expect("a shown chart copies");

        let encoded = url.strip_prefix("data:image/png;base64,").expect("a PNG data URL");
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).expect("valid base64");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder.read_info().expect("a readable PNG");
        assert_eq!((reader.info().width, reader.info().height), (320, 200));
    }

    #[test]
    fn theme_tokens_parse_as_plot_colours() {
        let white = parse_color("oklch(1 0 0)").expect("oklch parses");
        assert!((white.r - 1.0).abs() < 0.01 && (white.a - 1.0).abs() < 0.01);
        let faint = parse_color("oklch(1 0 0 / 10%)").expect("alpha parses");
        assert!((faint.a - 0.1).abs() < 0.01);
        assert!(parse_color("not a colour").is_none());
    }
}
