//! The demo: a baked clip drawn by the desktop viewer's egui renderer, with
//! its transport and layer switches.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use egui::Color32;
use egui::FontFamily;
use egui::Pos2;
use egui::Rect;
use egui::Sense;
use egui::Shape;
use egui::Stroke;
use egui::StrokeKind;
use egui::Vec2;
use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::config::hide_command_parts;
use wows_minimap_renderer::toggles::TOGGLES;
use wows_minimap_renderer::toggles::Toggle;
use wows_minimap_renderer::track_file::TrackFile;
use wows_minimap_renderer::trail::trails_through;
use wows_replays::types::GameClock;
use wt_collab_egui::player::MinimapView;
use wt_collab_egui::player::NoCommandFilter;
use wt_collab_egui::rendering::game_font;
use wt_collab_egui::types::GridStyle;
use wt_translations::DefaultTextResolver;

use crate::error::LoadError;
use crate::page::PageOptions;
use crate::page::Theme;
use crate::playback::FrameIndex;
use crate::playback::Playback;
use crate::playback::Speed;
use crate::playback::elapsed_label;
use crate::textures::Textures;
use crate::tour::Tour;
use crate::tour::TourAction;
use crate::tour::TourStep;

/// The fetched and decoded clip, handed over by the task that fetched it.
pub type Inbox = Rc<RefCell<Option<Result<TrackFile, LoadError>>>>;

/// Behind the map, as the desktop viewer paints it. The HUD's text is drawn
/// for a dark canvas, so this stays dark in the light theme too.
const MAP_BACKGROUND: Color32 = Color32::from_rgb(20, 25, 35);

/// Width of the layer list when it sits beside the map.
const LAYERS_WIDTH: f32 = 190.0;

/// How long a switch the tour changed stays marked, in seconds.
const HIGHLIGHT_SECONDS: f64 = 2.5;

pub struct DemoApp {
    inbox: Inbox,
    state: State,
}

enum State {
    Loading,
    Failed(LoadError),
    Ready(Box<Demo>),
}

impl DemoApp {
    pub fn new(cc: &eframe::CreationContext<'_>, page: &PageOptions, inbox: Inbox) -> Self {
        cc.egui_ctx.set_theme(match page.theme {
            Some(Theme::Light) => egui::ThemePreference::Light,
            Some(Theme::Dark) => egui::ThemePreference::Dark,
            None => egui::ThemePreference::System,
        });

        // The renderer draws its labels in the game's font family; without the
        // game's fonts that family is egui's own proportional font.
        let mut fonts = egui::FontDefinitions::default();
        if let Some(proportional) = fonts.families.get(&FontFamily::Proportional).cloned() {
            fonts.families.insert(FontFamily::Name("GameFont".into()), proportional);
        }
        cc.egui_ctx.set_fonts(fonts);

        Self { inbox, state: State::Loading }
    }
}

impl eframe::App for DemoApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let Some(result) = self.inbox.borrow_mut().take() else {
            return;
        };

        let now = ctx.input(|input| input.time);
        self.state = match result.and_then(|track| Demo::new(ctx, track, now)) {
            Ok(demo) => State::Ready(Box::new(demo)),
            Err(error) => State::Failed(error),
        };
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        match &mut self.state {
            State::Loading => {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.horizontal_centered(|ui| {
                            ui.spinner();
                            ui.label("Loading replay");
                        });
                    });
                });
            }
            State::Failed(error) => {
                let mut reasons = vec![error.to_string()];
                let mut source = std::error::Error::source(error);
                while let Some(cause) = source {
                    reasons.push(cause.to_string());
                    source = cause.source();
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.heading("The replay could not be loaded");
                    for reason in reasons {
                        ui.label(reason);
                    }
                });
            }
            State::Ready(demo) => demo.show(ui),
        }
    }
}

/// A layer switch the clip offers, with the label it shows.
struct Switch {
    label: String,
    toggle: &'static Toggle,
}

/// What the tour puts back when it ends.
struct Settings {
    options: RenderOptions,
    show_dead_ships: bool,
    speed: Speed,
    playing: bool,
}

struct Demo {
    frames: Vec<Vec<DrawCommand>>,
    battle_start: GameClock,
    playback: Playback,
    textures: Textures,

    options: RenderOptions,
    show_dead_ships: bool,
    switches: Vec<Switch>,
    /// The trails, and what they were derived for.
    trails: Option<(TrailKey, Vec<DrawCommand>)>,

    tour: Tour,
    before_tour: Option<Settings>,
    /// The switch the tour last changed, and until when it is marked.
    highlight: Option<(&'static str, f64)>,
}

impl Demo {
    fn new(ctx: &egui::Context, track: TrackFile, now: f64) -> Result<Self, LoadError> {
        let playback = Playback::new(track.clocks).ok_or(LoadError::NoFrames)?;
        let textures = Textures::upload(ctx, &track.map_png, track.icons)?;

        // The file decides which switches are offered and what they say; one
        // this build does not know is left out.
        let switches = track
            .toggles
            .into_iter()
            .filter_map(|offered| {
                let toggle = TOGGLES.iter().find(|toggle| toggle.id == offered.id)?;
                Some(Switch { label: offered.label, toggle })
            })
            .collect();

        Ok(Self {
            frames: track.frames,
            battle_start: track.battle_start,
            playback,
            textures,
            options: initial_options(),
            show_dead_ships: true,
            switches,
            trails: None,
            tour: Tour::new(now),
            before_tour: None,
            highlight: None,
        })
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let (now, wall, visitor_input) =
            ctx.input(|input| (input.time, input.stable_dt, input.events.iter().any(is_visitor_input)));

        if visitor_input {
            self.tour.interrupt(now);
            self.before_tour = None;
        }
        self.keyboard(&ctx);

        let was_touring = self.tour.is_running();
        let due = self.tour.due(now);
        if !was_touring && !due.is_empty() {
            self.before_tour = Some(self.settings());
            self.playback.set_playing(true);
        }
        for step in due {
            self.perform(step, now);
        }

        self.playback.advance(Duration::from_secs_f32(wall));

        let wide = ui.available_width() >= ui.available_height() * 0.9 + LAYERS_WIDTH;
        egui::Panel::bottom("transport").show(ui, |ui| self.transport(ui, !wide, now));
        if wide {
            egui::Panel::right("layers").resizable(false).exact_size(LAYERS_WIDTH).show(ui, |ui| {
                ui.add_space(4.0);
                ui.strong("Layers");
                egui::ScrollArea::vertical().show(ui, |ui| self.layer_list(ui, now));
            });
        }
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));

        let marking = self.highlight.is_some_and(|(_, until)| now < until);
        if self.playback.is_playing() || self.tour.is_running() || marking {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_secs_f64(self.tour.until_next(now)));
        }
    }

    fn settings(&self) -> Settings {
        Settings {
            options: self.options.clone(),
            show_dead_ships: self.show_dead_ships,
            speed: self.playback.speed(),
            playing: self.playback.is_playing(),
        }
    }

    fn perform(&mut self, step: &TourStep, now: f64) {
        match step.action {
            TourAction::Toggle { id, on } => {
                if let Some(switch) = self.switches.iter().find(|switch| switch.toggle.id == id) {
                    (switch.toggle.write)(&mut self.options, &mut self.show_dead_ships, on);
                    self.highlight = Some((switch.toggle.id, now + HIGHLIGHT_SECONDS));
                }
            }
            TourAction::SeekBack { game_seconds } => {
                self.playback.seek(GameClock(self.playback.position().seconds() - game_seconds));
            }
            TourAction::SetSpeed(speed) => self.playback.set_speed(speed),
            TourAction::Restore => {
                if let Some(settings) = self.before_tour.take() {
                    self.options = settings.options;
                    self.show_dead_ships = settings.show_dead_ships;
                    self.playback.set_speed(settings.speed);
                    self.playback.set_playing(settings.playing);
                }
            }
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }

        ctx.input(|input| {
            if input.key_pressed(egui::Key::Space) {
                self.playback.set_playing(!self.playback.is_playing());
            }
            if input.key_pressed(egui::Key::ArrowLeft) {
                self.playback.step(-1);
            }
            if input.key_pressed(egui::Key::ArrowRight) {
                self.playback.step(1);
            }
        });
    }

    fn transport(&mut self, ui: &mut egui::Ui, layers_in_menu: bool, now: f64) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Rewind, "Back to the start").clicked() {
                self.playback.rewind();
            }
            if icon_button(ui, Icon::StepBack, "Previous frame").clicked() {
                self.playback.step(-1);
            }
            let (icon, hint) = if self.playback.is_playing() { (Icon::Pause, "Pause") } else { (Icon::Play, "Play") };
            if icon_button(ui, icon, hint).clicked() {
                self.playback.set_playing(!self.playback.is_playing());
            }
            if icon_button(ui, Icon::StepForward, "Next frame").clicked() {
                self.playback.step(1);
            }

            let mut speed = self.playback.speed();
            egui::ComboBox::from_id_salt("speed").selected_text(speed.label()).width(56.0).show_ui(ui, |ui| {
                for choice in Speed::CHOICES {
                    ui.selectable_value(&mut speed, choice, choice.label());
                }
            });
            self.playback.set_speed(speed);

            if layers_in_menu {
                ui.menu_button("Layers", |ui| self.layer_list(ui, now));
            }

            let elapsed = elapsed_label(self.playback.position().seconds() - self.battle_start.seconds());
            let total = elapsed_label(self.playback.end().seconds() - self.battle_start.seconds());
            let label = format!("{elapsed} / {total}");
            let label_width = ui.fonts_mut(|fonts| {
                fonts.layout_no_wrap(label.clone(), egui::TextStyle::Monospace.resolve(ui.style()), Color32::WHITE)
            });
            let reserved = label_width.size().x + ui.spacing().item_spacing.x * 2.0;

            let start = self.playback.start().seconds();
            let mut offset = self.playback.position().seconds() - start;
            ui.spacing_mut().slider_width = (ui.available_width() - reserved).max(40.0);
            let slider = egui::Slider::new(&mut offset, 0.0..=self.playback.end().seconds() - start).show_value(false);
            if ui.add(slider).changed() {
                self.playback.seek(GameClock(start + offset));
            }
            ui.monospace(label);
        });
        ui.add_space(2.0);
    }

    fn layer_list(&mut self, ui: &mut egui::Ui, now: f64) {
        let marked = self.highlight.filter(|(_, until)| now < *until).map(|(id, _)| id);
        for switch in &self.switches {
            let mut on = (switch.toggle.read)(&self.options, self.show_dead_ships);
            let behind = ui.painter().add(Shape::Noop);
            let response = ui.checkbox(&mut on, switch.label.as_str());
            if response.changed() {
                (switch.toggle.write)(&mut self.options, &mut self.show_dead_ships, on);
            }
            if marked == Some(switch.toggle.id) {
                let fill = ui.visuals().selection.bg_fill.gamma_multiply(0.6);
                ui.painter().set(behind, Shape::rect_filled(response.rect.expand(2.0), 3.0, fill));
            }
        }
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let frame = self.playback.frame();
        let commands = self.commands_for(frame);

        let area = ui.available_rect_before_wrap();
        let aspect = MINIMAP_SIZE as f32 / CANVAS_HEIGHT as f32;
        let size = if area.width() / area.height() > aspect {
            Vec2::new(area.height() * aspect, area.height())
        } else {
            Vec2::new(area.width(), area.width() / aspect)
        };
        let rect = Rect::from_center_size(area.center(), size);

        let textures = self.textures.for_drawing();
        let grid = |window_scale: f32| GridStyle {
            grid_color: Color32::from_rgba_unmultiplied(255, 255, 255, 64),
            label_color: Color32::from_rgba_unmultiplied(255, 255, 255, 180),
            line_width: 1.0,
            label_font: game_font(11.0 * window_scale),
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        let (response, _) = MinimapView {
            commands: &commands,
            textures: &textures,
            map_texture: Some(self.textures.map()),
            options: &self.options,
            show_dead_ships: self.show_dead_ships,
            zoom: 1.0,
            pan: Vec2::ZERO,
            filter: &NoCommandFilter,
            text_resolver: &DefaultTextResolver,
            grid: Some(&grid),
            background: MAP_BACKGROUND,
        }
        .show(&mut child, size, Sense::click());
        ui.advance_cursor_after_rect(area);

        if response.clicked() {
            self.playback.set_playing(!self.playback.is_playing());
        }

        if let Some(caption) = self.tour.caption() {
            let scale = size.x / MINIMAP_SIZE as f32;
            let at = rect.min + Vec2::new(8.0, HUD_HEIGHT as f32 + 8.0) * scale;
            caption_box(ui, at, &format!("Tour: {caption}. Move the pointer to take over."));
        }
    }

    /// Frame `frame`'s commands, behind them the trails so far when trails
    /// are on, as the desktop viewer draws them.
    fn commands_for(&mut self, frame: FrameIndex) -> Vec<DrawCommand> {
        let options = &self.options;
        let trails: &[DrawCommand] = if options.show_trails || options.show_speed_trails {
            let wanted = TrailKey { frame, by_speed: options.show_speed_trails, with_dead: options.show_dead_trails };
            if self.trails.as_ref().map(|(key, _)| *key) != Some(wanted) {
                self.trails = Some((wanted, trails_through(&self.frames[..=frame.get()], options)));
            }
            self.trails.as_ref().map_or(&[][..], |(_, trails)| trails.as_slice())
        } else {
            &[]
        };

        trails
            .iter()
            .chain(&self.frames[frame.get()])
            .cloned()
            .map(|mut command| {
                hide_command_parts(&mut command, options);
                command
            })
            .collect()
    }
}

/// What a set of derived trails was derived for: the frame and the trail switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TrailKey {
    frame: FrameIndex,
    by_speed: bool,
    with_dead: bool,
}

/// What the demo draws before the visitor changes anything: the desktop
/// viewer's defaults for a playback, without the gutter panels.
fn initial_options() -> RenderOptions {
    RenderOptions {
        show_player_names: false,
        show_kill_feed: true,
        show_armament: true,
        show_stats_panel: false,
        show_team_rosters: false,
        show_ship_config: false,
        show_trails: false,
        show_speed_trails: false,
        ..RenderOptions::default()
    }
}

fn is_visitor_input(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::PointerMoved(_)
            | egui::Event::PointerButton { .. }
            | egui::Event::MouseWheel { .. }
            | egui::Event::Key { .. }
            | egui::Event::Touch { .. }
    )
}

fn caption_box(ui: &egui::Ui, at: Pos2, text: &str) {
    let painter = ui.painter();
    let galley =
        painter.layout_no_wrap(text.to_owned(), egui::FontId::proportional(13.0), Color32::from_rgb(240, 240, 240));
    let padding = Vec2::new(8.0, 5.0);
    let rect = Rect::from_min_size(at, galley.size() + padding * 2.0);

    painter.rect_filled(rect, 4.0, Color32::from_black_alpha(190));
    painter.galley(rect.min + padding, galley, Color32::WHITE);
}

#[derive(Clone, Copy)]
enum Icon {
    Rewind,
    StepBack,
    Play,
    Pause,
    StepForward,
}

/// A transport button with its symbol drawn as shapes, so the demo carries no
/// icon font.
fn icon_button(ui: &mut egui::Ui, icon: Icon, hint: &str) -> egui::Response {
    let side = ui.spacing().interact_size.y + 6.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let visuals = ui.style().interact(&response);
    let painter = ui.painter();
    painter.rect(
        rect.expand(visuals.expansion),
        visuals.corner_radius,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        StrokeKind::Inside,
    );

    let color = visuals.fg_stroke.color;
    let center = rect.center();
    let s = side * 0.22;
    let triangle = |tip_x: f32, base_x: f32| {
        Shape::convex_polygon(
            vec![Pos2::new(base_x, center.y - s), Pos2::new(tip_x, center.y), Pos2::new(base_x, center.y + s)],
            color,
            Stroke::NONE,
        )
    };
    let bar = |x: f32| {
        Shape::rect_filled(
            Rect::from_min_max(Pos2::new(x, center.y - s), Pos2::new(x + s * 0.4, center.y + s)),
            0.0,
            color,
        )
    };

    let shapes = match icon {
        Icon::Play => vec![triangle(center.x + s, center.x - s * 0.8)],
        Icon::Pause => vec![bar(center.x - s * 0.7), bar(center.x + s * 0.3)],
        Icon::StepBack => vec![bar(center.x - s), triangle(center.x - s * 0.5, center.x + s)],
        Icon::StepForward => vec![triangle(center.x + s * 0.5, center.x - s), bar(center.x + s * 0.6)],
        Icon::Rewind => vec![
            bar(center.x - s * 1.2),
            triangle(center.x - s * 0.7, center.x + s * 0.1),
            triangle(center.x + s * 0.1, center.x + s * 0.9),
        ],
    };
    painter.extend(shapes);

    response.on_hover_text(hint)
}
