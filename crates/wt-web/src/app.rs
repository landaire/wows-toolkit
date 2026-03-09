//! The main eframe::App implementation for the web client.

use egui::Color32;
use egui_dock::DockArea;
use egui_dock::DockState;
use egui_dock::TabViewer;
use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;

use wt_collab_egui::interaction::handle_annotation_select_move;
use wt_collab_egui::interaction::handle_tool_interaction;
use wt_collab_egui::rendering;
use wt_collab_egui::toolbar::draw_annotation_toolbar;
use wt_collab_egui::transforms::MapTransform;
use wt_collab_egui::transforms::ViewportZoomPan;
use wt_collab_egui::types::GridStyle;

use crate::assets::AssetStore;
use crate::connection::CommandQueue;
use crate::connection::ConnectionCommand;
use crate::connection::ConnectionEvent;
use crate::connection::EventQueue;
use crate::state::ActiveView;
use crate::state::ConnectedUser;
use crate::state::FrameState;
use crate::state::MapPing;
use crate::state::Permissions;
use crate::state::ReplayView;
use crate::state::SessionState;
use crate::state::TacticsBoard;
use crate::state::UserCursor;
use crate::types::Annotation;
use crate::types::AnnotationState;
use crate::types::PaintTool;

use wt_collab_protocol::protocol::PeerMessage;

/// Tab type for the dock layout.
#[derive(Clone, Debug, PartialEq)]
pub enum WebTab {
    Lobby,
    Replay(u64),
    TacticsBoard(u64),
}

impl WebTab {
    fn title(&self, session: &SessionState) -> String {
        match self {
            WebTab::Lobby => "Lobby".to_string(),
            WebTab::Replay(id) => session
                .replay_views
                .get(id)
                .map(|v| {
                    if v.display_name.is_empty() {
                        format!("Replay \u{2014} {}", v.replay_name)
                    } else {
                        format!("Replay \u{2014} {}", v.display_name)
                    }
                })
                .unwrap_or_else(|| format!("Replay {id}")),
            WebTab::TacticsBoard(id) => session
                .tactics_boards
                .get(id)
                .map(|b| {
                    if b.display_name.is_empty() {
                        format!("Tactics Board \u{2014} {}", b.map_name)
                    } else {
                        format!("Tactics Board \u{2014} {}", b.display_name)
                    }
                })
                .unwrap_or_else(|| format!("Tactics {id}")),
        }
    }

    fn to_active_view(&self) -> ActiveView {
        match self {
            WebTab::Lobby => ActiveView::Lobby,
            WebTab::Replay(id) => ActiveView::Replay(*id),
            WebTab::TacticsBoard(id) => ActiveView::TacticsBoard(*id),
        }
    }
}

/// The web application state.
pub struct WebApp {
    /// Connection state.
    connection_status: ConnectionStatus,
    /// Event queue from connection task.
    events: Option<EventQueue>,
    /// Command queue to connection task.
    commands: Option<CommandQueue>,

    /// Session state (populated after connection).
    session: SessionState,
    /// Asset store (populated after receiving AssetBundle).
    assets: AssetStore,
    /// Whether fonts have been registered.
    fonts_registered: bool,

    /// Annotation state for the currently active view.
    annotation_state: AnnotationState,
    /// Viewport zoom/pan.
    viewport: ViewportZoomPan,

    /// Display name for this client.
    display_name: String,
    /// Session token from URL hash or manual entry.
    session_token: Option<String>,
    /// Text field for manual token entry.
    manual_token: String,

    /// How many times we've requested assets in this connection. Resets on reconnect.
    asset_request_count: u32,
    /// When we last sent a RequestAssets message.
    last_asset_request: Option<web_time::Instant>,

    /// Dock state for tabbed views.
    dock_state: DockState<WebTab>,
    /// Tabs queued to be added after the current frame's dock rendering completes.
    /// Needed because `dock_state` is temporarily swapped out during `DockArea::show`.
    pending_tabs: Vec<WebTab>,
    /// Tabs the user has explicitly closed. Used to show re-open buttons in the lobby.
    closed_tabs: Vec<WebTab>,
}

enum ConnectionStatus {
    /// Waiting for user to enter name / auto-connecting.
    NotConnected,
    /// Connection in progress.
    Connecting,
    /// Connected and receiving data.
    Connected,
    /// Auto-reconnecting after a drop.
    Reconnecting { attempt: u32, max_attempts: u32 },
    /// Connection failed or was rejected.
    Error(String),
}

impl WebApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Register phosphor icon font so icons render before AssetBundle arrives
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        cc.egui_ctx.set_fonts(fonts);

        // Extract session token from URL hash
        let session_token = extract_session_token();

        // Load persisted display name (empty until the user sets one).
        let display_name = local_storage_get("wt_display_name").unwrap_or_default();

        Self {
            connection_status: ConnectionStatus::NotConnected,
            events: None,
            commands: None,
            session: SessionState::default(),
            assets: AssetStore::default(),
            fonts_registered: false,
            annotation_state: AnnotationState::default(),
            viewport: ViewportZoomPan::default(),
            display_name,
            session_token,
            manual_token: String::new(),
            asset_request_count: 0,
            last_asset_request: None,
            dock_state: DockState::new(vec![WebTab::Lobby]),
            pending_tabs: Vec::new(),
            closed_tabs: Vec::new(),
        }
    }

    fn connect(&mut self, ctx: &egui::Context) {
        let Some(ref token) = self.session_token else {
            self.connection_status = ConnectionStatus::Error("No session token in URL".to_string());
            return;
        };

        self.connection_status = ConnectionStatus::Connecting;
        local_storage_set("wt_display_name", &self.display_name);
        let (events, commands) =
            crate::connection::start_connection(token.clone(), self.display_name.clone(), ctx.clone());
        self.events = Some(events);
        self.commands = Some(commands);
    }

    fn disconnect(&mut self) {
        if let Some(ref commands) = self.commands {
            commands.borrow_mut().push_back(ConnectionCommand::Disconnect);
        }
        self.events = None;
        self.commands = None;
        self.connection_status = ConnectionStatus::NotConnected;
        self.session = Default::default();
        self.assets = Default::default();
        self.asset_request_count = 0;
        self.last_asset_request = None;
    }

    fn send_message(&self, msg: PeerMessage) {
        if let Some(ref commands) = self.commands {
            commands.borrow_mut().push_back(ConnectionCommand::Send(Box::new(msg)));
        }
    }

    fn poll_events(&mut self, ctx: &egui::Context) {
        let Some(ref events) = self.events else { return };

        let mut events_to_process = Vec::new();
        {
            let mut queue = events.borrow_mut();
            while let Some(event) = queue.pop_front() {
                events_to_process.push(event);
            }
        }

        for event in events_to_process {
            match event {
                ConnectionEvent::Connected { my_user_id, my_name, my_color, host_user_id, frame_source_id } => {
                    self.connection_status = ConnectionStatus::Connected;
                    self.session.my_user_id = my_user_id;
                    self.session.host_user_id = host_user_id;
                    self.session.frame_source_id = frame_source_id;

                    // Reset asset request counter on (re)connect.
                    self.asset_request_count = 0;
                    self.last_asset_request = None;

                    // Clear old session state (handles reconnect case).
                    self.session.connected_users.clear();
                    self.session.cursors.clear();
                    self.session.tactics_boards.clear();
                    self.session.replay_views.clear();
                    self.closed_tabs.clear();

                    // Add self cursor.
                    self.session.cursors.push(UserCursor {
                        user_id: my_user_id,
                        name: my_name,
                        color: my_color,
                        pos: None,
                        last_update: web_time::Instant::now(),
                    });
                }
                ConnectionEvent::Message(msg) => {
                    self.handle_message(msg, ctx);
                }
                ConnectionEvent::Rejected(reason) => {
                    self.connection_status = ConnectionStatus::Error(format!("Rejected: {reason}"));
                }
                ConnectionEvent::Error(msg) => {
                    tracing::warn!("Connection error: {msg}");
                    self.connection_status = ConnectionStatus::Error(msg);
                }
                ConnectionEvent::Reconnecting { attempt, max_attempts } => {
                    tracing::info!("Reconnecting ({attempt}/{max_attempts})...");
                    self.connection_status = ConnectionStatus::Reconnecting { attempt, max_attempts };
                }
                ConnectionEvent::Disconnected => {
                    self.connection_status = ConnectionStatus::Error("Disconnected".to_string());
                }
            }
        }
    }

    fn handle_message(&mut self, msg: PeerMessage, ctx: &egui::Context) {
        match msg {
            PeerMessage::AssetBundle {
                ship_icons,
                plane_icons,
                consumable_icons,
                death_cause_icons,
                powerup_icons,
                game_fonts,
            } => {
                tracing::info!(
                    "Received AssetBundle: {} ship icons, {} plane icons",
                    ship_icons.len(),
                    plane_icons.len()
                );
                self.assets = AssetStore::load_from_bundle(
                    ctx,
                    ship_icons,
                    plane_icons,
                    consumable_icons,
                    death_cause_icons,
                    powerup_icons,
                    game_fonts,
                );
                self.assets.register_fonts(ctx);
                self.fonts_registered = true;
            }
            PeerMessage::Permissions { annotations_locked, settings_locked } => {
                self.session.permissions = Permissions { annotations_locked, settings_locked };
            }
            PeerMessage::RenderOptions(opts) => {
                self.session.render_options = Some(opts);
            }
            PeerMessage::UserJoined { user_id, name, color } => {
                if !self.session.connected_users.iter().any(|u| u.id == user_id) {
                    self.session.connected_users.push(ConnectedUser { id: user_id, name: name.clone(), color });
                    self.session.cursors.push(UserCursor {
                        user_id,
                        name,
                        color,
                        pos: None,
                        last_update: web_time::Instant::now(),
                    });
                }
            }
            PeerMessage::UserLeft { user_id } => {
                self.session.connected_users.retain(|u| u.id != user_id);
                self.session.cursors.retain(|c| c.user_id != user_id);
            }
            PeerMessage::CursorPosition { user_id, pos } => {
                if let Some(c) = self.session.cursors.iter_mut().find(|c| c.user_id == user_id) {
                    c.pos = pos;
                    c.last_update = web_time::Instant::now();
                } else {
                    // Create a cursor entry for this user.
                    let (name, color) = self
                        .session
                        .connected_users
                        .iter()
                        .find(|u| u.id == user_id)
                        .map(|u| (u.name.clone(), u.color))
                        .unwrap_or_else(|| (format!("Peer {user_id}"), [200, 200, 200]));
                    self.session.cursors.push(UserCursor {
                        user_id,
                        name,
                        color,
                        pos,
                        last_update: web_time::Instant::now(),
                    });
                }
            }
            PeerMessage::SetAnnotation { board_id, id, annotation, owner } => {
                let local = crate::types::wire_to_local(annotation);
                self.apply_set_annotation(board_id, id, local, owner);
            }
            PeerMessage::RemoveAnnotation { board_id, id } => {
                self.apply_remove_annotation(board_id, id);
            }
            PeerMessage::ClearAnnotations { board_id } => {
                self.apply_clear_annotations(board_id);
            }
            PeerMessage::AnnotationSync { board_id, annotations, owners, ids } => {
                let local_anns: Vec<Annotation> = annotations.into_iter().map(crate::types::wire_to_local).collect();
                self.apply_annotation_sync(board_id, local_anns, owners, ids);
            }
            PeerMessage::Ping { pos, color, .. } => {
                self.session.pings.push(MapPing { pos, color, time: web_time::Instant::now() });
            }
            PeerMessage::ReplayOpened { replay_id, replay_name, map_image_png, display_name, .. } => {
                self.session.replay_views.entry(replay_id).or_insert_with(|| ReplayView {
                    replay_id,
                    replay_name,
                    display_name,
                    map_image_png: Some(map_image_png),
                    map_texture: None,
                    annotations: Vec::new(),
                    annotation_ids: Vec::new(),
                    annotation_owners: Vec::new(),
                    current_frame: None,
                });
                let tab = WebTab::Replay(replay_id);
                if !self.dock_has_tab(&tab) {
                    self.dock_push_tab(tab);
                }
            }
            PeerMessage::ReplayClosed { replay_id } => {
                self.session.replay_views.remove(&replay_id);
                self.dock_remove_tab(&WebTab::Replay(replay_id));
                self.closed_tabs.retain(|t| t != &WebTab::Replay(replay_id));
            }
            PeerMessage::Frame { replay_id, clock, frame_index, total_frames, game_duration, commands } => {
                if let Some(view) = self.session.replay_views.get_mut(&replay_id) {
                    view.current_frame = Some(FrameState { clock, frame_index, total_frames, game_duration, commands });
                }
            }
            PeerMessage::TacticsMapOpened {
                board_id, map_name, display_name, map_id, map_image_png, map_info, ..
            } => {
                self.session.tactics_boards.entry(board_id).or_insert_with(|| TacticsBoard {
                    board_id,
                    map_name,
                    display_name,
                    map_id,
                    map_image_png: Some(map_image_png),
                    map_texture: None,
                    map_info,
                    annotations: Vec::new(),
                    annotation_ids: Vec::new(),
                    annotation_owners: Vec::new(),
                    cap_points: Vec::new(),
                });
                let tab = WebTab::TacticsBoard(board_id);
                if !self.dock_has_tab(&tab) {
                    self.dock_push_tab(tab);
                }
            }
            PeerMessage::TacticsMapClosed { board_id } => {
                self.session.tactics_boards.remove(&board_id);
                self.dock_remove_tab(&WebTab::TacticsBoard(board_id));
                self.closed_tabs.retain(|t| t != &WebTab::TacticsBoard(board_id));
            }
            PeerMessage::SetCapPoint { board_id, cap_point } => {
                if let Some(board) = self.session.tactics_boards.get_mut(&board_id) {
                    if let Some(existing) = board.cap_points.iter_mut().find(|c| c.id == cap_point.id) {
                        *existing = cap_point;
                    } else {
                        board.cap_points.push(cap_point);
                    }
                }
            }
            PeerMessage::RemoveCapPoint { board_id, id } => {
                if let Some(board) = self.session.tactics_boards.get_mut(&board_id) {
                    board.cap_points.retain(|c| c.id != id);
                }
            }
            PeerMessage::CapPointSync { board_id, cap_points } => {
                if let Some(board) = self.session.tactics_boards.get_mut(&board_id) {
                    board.cap_points = cap_points;
                }
            }
            PeerMessage::SelfSilhouette { data, width, height } => {
                let image = egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &data);
                self.assets.silhouette_texture =
                    Some(ctx.load_texture("stats_silhouette", image, egui::TextureOptions::LINEAR));
            }
            _ => {
                // Ignore unhandled messages (Heartbeat, MeshHello, PeerAnnounce, etc.)
            }
        }

        // Repaint after every message so cursor moves, pings, annotations,
        // user joins/leaves, etc. are rendered immediately.
        ctx.request_repaint();
    }

    fn apply_set_annotation(&mut self, board_id: Option<u64>, id: u64, annotation: Annotation, owner: u64) {
        let (anns, ids, owners) = self.get_annotation_lists_mut(board_id);
        if let Some(idx) = ids.iter().position(|&existing_id| existing_id == id) {
            anns[idx] = annotation;
            owners[idx] = owner;
        } else {
            anns.push(annotation);
            ids.push(id);
            owners.push(owner);
        }
    }

    fn apply_remove_annotation(&mut self, board_id: Option<u64>, id: u64) {
        let (anns, ids, owners) = self.get_annotation_lists_mut(board_id);
        if let Some(idx) = ids.iter().position(|&existing_id| existing_id == id) {
            anns.remove(idx);
            ids.remove(idx);
            owners.remove(idx);
        }
    }

    fn apply_clear_annotations(&mut self, board_id: Option<u64>) {
        let (anns, ids, owners) = self.get_annotation_lists_mut(board_id);
        anns.clear();
        ids.clear();
        owners.clear();
    }

    fn sync_annotations_after_undo(&mut self, board_id: Option<u64>) {
        // After undo, send the full annotation state to sync peers.
        let wire_anns: Vec<_> = self.annotation_state.annotations.iter().map(crate::types::local_to_wire).collect();
        self.send_message(PeerMessage::AnnotationSync {
            board_id,
            annotations: wire_anns,
            owners: self.annotation_state.annotation_owners.clone(),
            ids: self.annotation_state.annotation_ids.clone(),
        });
    }

    fn apply_annotation_sync(
        &mut self,
        board_id: Option<u64>,
        annotations: Vec<Annotation>,
        new_owners: Vec<u64>,
        new_ids: Vec<u64>,
    ) {
        let (anns, ids, owners) = self.get_annotation_lists_mut(board_id);
        *anns = annotations;
        *ids = new_ids;
        *owners = new_owners;
    }

    /// Get mutable references to the annotation lists for a given board_id.
    /// board_id=None means the current replay view.
    fn get_annotation_lists_mut(
        &mut self,
        board_id: Option<u64>,
    ) -> (&mut Vec<Annotation>, &mut Vec<u64>, &mut Vec<u64>) {
        if let Some(bid) = board_id
            && let Some(board) = self.session.tactics_boards.get_mut(&bid)
        {
            return (&mut board.annotations, &mut board.annotation_ids, &mut board.annotation_owners);
        }
        // Fallback to current replay view
        if let ActiveView::Replay(id) = self.session.active_view
            && let Some(view) = self.session.replay_views.get_mut(&id)
        {
            return (&mut view.annotations, &mut view.annotation_ids, &mut view.annotation_owners);
        }
        // Fallback: use annotation_state
        (
            &mut self.annotation_state.annotations,
            &mut self.annotation_state.annotation_ids,
            &mut self.annotation_state.annotation_owners,
        )
    }

    fn render_lobby(&mut self, ui: &mut egui::Ui) {
        let mut disconnect = false;
        let mut reopen_tab = None;
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading("WoWs Toolkit - Tactics Board");
            ui.add_space(20.0);

            match &self.connection_status {
                ConnectionStatus::NotConnected => {
                    ui.label("Display name:");
                    ui.text_edit_singleline(&mut self.display_name);
                    ui.add_space(10.0);

                    let name_valid = !self.display_name.trim().is_empty();

                    if self.session_token.is_some() {
                        if ui.add_enabled(name_valid, egui::Button::new("Connect")).clicked() {
                            self.connect(ui.ctx());
                        }
                    } else {
                        ui.label("Enter a session token to connect:");
                        ui.text_edit_singleline(&mut self.manual_token);
                        ui.add_space(6.0);
                        let token_valid = !self.manual_token.trim().is_empty();
                        if ui.add_enabled(name_valid && token_valid, egui::Button::new("Connect")).clicked() {
                            self.session_token = Some(self.manual_token.trim().to_string());
                            self.connect(ui.ctx());
                        }
                    }

                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(
                            "Collaborative sessions use peer-to-peer networking. \
                             Other users in the session may be able to see your IP address.",
                        )
                        .small()
                        .weak(),
                    );
                    ui.hyperlink_to("More info", "https://landaire.github.io/wows-toolkit/networking");
                }
                ConnectionStatus::Connecting => {
                    ui.spinner();
                    ui.label("Connecting to host...");
                }
                ConnectionStatus::Connected => {
                    let other_count =
                        self.session.connected_users.iter().filter(|u| u.id != self.session.my_user_id).count();
                    ui.label(format!("Connected \u{2014} {other_count} other user(s)"));
                    if ui.small_button("Disconnect").clicked() {
                        disconnect = true;
                    }

                    // User list sorted by role (host first, then peers).
                    if !self.session.connected_users.is_empty() {
                        ui.add_space(10.0);
                        let host_id = self.session.host_user_id;
                        let my_id = self.session.my_user_id;
                        let mut users: Vec<_> = self.session.connected_users.iter().collect();
                        users.sort_by_key(|u| if u.id == host_id { 0 } else { 1 });

                        for user in &users {
                            let role = if user.id == host_id { "Host" } else { "Peer" };
                            let [r, g, b] = user.color;
                            let color = egui::Color32::from_rgb(r, g, b);
                            let suffix = if user.id == my_id { " (you)" } else { "" };
                            ui.colored_label(color, format!("{} \u{2014} {role}{suffix}", user.name));
                        }
                    }

                    // Show tabs the user explicitly closed so they can re-open them.
                    let closed: Vec<_> = self
                        .closed_tabs
                        .iter()
                        .filter_map(|tab| match tab {
                            WebTab::TacticsBoard(id) => {
                                let b = self.session.tactics_boards.get(id)?;
                                let name = if b.display_name.is_empty() { &b.map_name } else { &b.display_name };
                                Some((tab.clone(), format!("Tactics Board \u{2014} {name}")))
                            }
                            WebTab::Replay(id) => {
                                let v = self.session.replay_views.get(id)?;
                                let name = if v.display_name.is_empty() { &v.replay_name } else { &v.display_name };
                                Some((tab.clone(), format!("Replay \u{2014} {name}")))
                            }
                            _ => None,
                        })
                        .collect();

                    if !closed.is_empty() {
                        ui.add_space(16.0);
                        ui.label("Closed windows:");
                        for (tab, label) in &closed {
                            if ui.button(label).clicked() {
                                reopen_tab = Some(tab.clone());
                            }
                        }
                    }
                }
                ConnectionStatus::Reconnecting { attempt, max_attempts } => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Connection lost. Reconnecting ({attempt}/{max_attempts})..."));
                    });
                }
                ConnectionStatus::Error(msg) => {
                    ui.colored_label(Color32::RED, msg);
                    ui.add_space(10.0);
                    if ui.button("Retry").clicked() {
                        self.connection_status = ConnectionStatus::NotConnected;
                    }
                }
            }
        });
        if disconnect {
            self.disconnect();
        }
        if let Some(tab) = reopen_tab {
            self.closed_tabs.retain(|t| t != &tab);
            self.dock_push_tab(tab);
        }
    }

    fn render_map_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use wt_collab_egui::interaction::ZoomPanConfig;
        use wt_collab_egui::interaction::handle_viewport_zoom_pan;
        use wt_collab_egui::transforms::compute_canvas_layout;
        use wt_collab_egui::transforms::compute_map_clip_rect;

        let is_replay = matches!(self.session.active_view, ActiveView::Replay(_));
        let hud_height = if is_replay { wows_minimap_renderer::HUD_HEIGHT as f32 } else { 0.0 };

        // Canvas layout
        let logical_canvas = egui::Vec2::new(MINIMAP_SIZE as f32, CANVAS_HEIGHT as f32);
        let available = ui.available_size();
        let (response, painter) = ui.allocate_painter(available, egui::Sense::click_and_drag());
        let layout = compute_canvas_layout(available, logical_canvas, self.viewport.zoom, response.rect.min, None);
        let window_scale = layout.window_scale;

        // Zoom/pan input handling — pass annotation state so left-drag panning
        // is suppressed when a drawing tool is active.
        handle_viewport_zoom_pan(
            ctx,
            &response,
            &mut self.viewport,
            &layout,
            logical_canvas,
            &ZoomPanConfig { allow_left_drag_pan: true, hud_height, handle_tool_yaw: true, map_width: None },
            Some(&mut self.annotation_state),
            false,
        );

        let transform = MapTransform {
            origin: layout.origin,
            window_scale,
            zoom: self.viewport.zoom,
            pan: self.viewport.pan,
            hud_height,
            canvas_width: MINIMAP_SIZE as f32,
            hud_width: MINIMAP_SIZE as f32,
        };

        // Clip rect for map elements (excludes HUD area above)
        let map_clip = compute_map_clip_rect(&layout, hud_height, None);
        let map_painter = painter.with_clip_rect(map_clip);

        // Draw map background
        let map_texture = self.get_active_map_texture(ctx);
        rendering::draw_map_background(&map_painter, &transform, map_texture.map(|t| t.id()));

        // Draw grid
        rendering::draw_grid(&map_painter, &transform, &GridStyle::default());

        // Draw replay frame (DrawCommands) — shared code from wt-collab-egui
        if is_replay && let Some(commands) = self.get_active_frame_commands() {
            use wt_collab_egui::draw_commands::DrawCommandLabelOptions;
            use wt_collab_egui::draw_commands::DrawCommandTextures;
            use wt_collab_egui::draw_commands::draw_command_to_shapes;
            use wt_translations::DefaultTextResolver;

            let textures = DrawCommandTextures {
                ship_icons: &self.assets.ship_icons,
                ship_icon_outlines: None,
                plane_icons: &self.assets.plane_icons,
                building_icons: None,
                consumable_icons: Some(&self.assets.consumable_icons),
                death_cause_icons: Some(&self.assets.death_cause_icons),
                powerup_icons: Some(&self.assets.powerup_icons),
                silhouette_texture: self.assets.silhouette_texture.as_ref(),
            };
            let label_opts = DrawCommandLabelOptions::default();
            let text_resolver = DefaultTextResolver;

            for cmd in &commands {
                let is_hud = cmd.is_hud();
                let cmd_shapes =
                    draw_command_to_shapes(cmd, &transform, &textures, ctx, &label_opts, None, &text_resolver);
                let target_painter = if is_hud { &painter } else { &map_painter };
                for shape in cmd_shapes {
                    target_painter.add(shape);
                }
            }
        }

        // Draw cap points (tactics boards only)
        if let ActiveView::TacticsBoard(bid) = self.session.active_view
            && let Some(board) = self.session.tactics_boards.get(&bid)
            && let Some(ref map_info) = board.map_info
        {
            for cap in &board.cap_points {
                rendering::render_cap_point(&map_painter, &transform, map_info, &cap.into());
            }
        }

        // Draw annotations + selection highlights
        let map_space_size = match self.session.active_view {
            ActiveView::TacticsBoard(bid) => {
                self.session.tactics_boards.get(&bid).and_then(|b| b.map_info.as_ref()).map(|m| m.space_size as f32)
            }
            _ => None,
        };
        let annotations = self.get_active_annotations();
        for ann in &annotations {
            rendering::render_annotation(ann, &transform, Some(&self.assets.ship_icons), &map_painter, map_space_size);
        }
        for &sel in &self.annotation_state.selected_indices {
            if sel < annotations.len() {
                rendering::render_selection_highlight(&annotations[sel], &transform, &map_painter);
            }
        }

        // Draw pings
        self.session.prune_pings();
        if rendering::draw_pings(&self.session.pings, &map_painter, &transform) {
            ctx.request_repaint();
        }

        // Draw remote cursors
        rendering::draw_remote_cursors(&self.session.cursors, self.session.my_user_id, &map_painter, &transform);

        // Handle cursor position updates
        if response.hovered() {
            if let Some(pointer) = response.hover_pos() {
                let minimap_pos = transform.screen_to_minimap(pointer);
                self.send_message(PeerMessage::CursorPosition {
                    user_id: self.session.my_user_id,
                    pos: Some([minimap_pos.x, minimap_pos.y]),
                });
            }
        } else {
            self.send_message(PeerMessage::CursorPosition { user_id: self.session.my_user_id, pos: None });
        }

        // Handle tool interaction (tactics boards only)
        let is_tactics = matches!(self.session.active_view, ActiveView::TacticsBoard(_));
        let can_annotate = is_tactics && !self.session.permissions.annotations_locked;
        let board_id = if let ActiveView::TacticsBoard(bid) = self.session.active_view { Some(bid) } else { None };

        if can_annotate {
            // Render tool preview
            if let Some(pos) = response.hover_pos().map(|p| transform.screen_to_minimap(p))
                && !matches!(self.annotation_state.active_tool, PaintTool::None | PaintTool::Eraser)
            {
                rendering::render_tool_preview(
                    &self.annotation_state.active_tool,
                    pos,
                    self.annotation_state.paint_color,
                    self.annotation_state.stroke_width,
                    &transform,
                    Some(&self.assets.ship_icons),
                    &map_painter,
                    map_space_size,
                );
            }

            // Swap board annotations into annotation_state for shared interaction
            if let Some(bid) = board_id
                && let Some(board) = self.session.tactics_boards.get_mut(&bid)
            {
                std::mem::swap(&mut self.annotation_state.annotations, &mut board.annotations);
                std::mem::swap(&mut self.annotation_state.annotation_ids, &mut board.annotation_ids);
                std::mem::swap(&mut self.annotation_state.annotation_owners, &mut board.annotation_owners);
            }

            let mut ping_on_empty_click = false;

            if !matches!(self.annotation_state.active_tool, PaintTool::None) {
                // Active drawing tool
                let result = handle_tool_interaction(&mut self.annotation_state, &response, &transform);

                // Swap back before applying results
                if let Some(bid) = board_id
                    && let Some(board) = self.session.tactics_boards.get_mut(&bid)
                {
                    std::mem::swap(&mut self.annotation_state.annotations, &mut board.annotations);
                    std::mem::swap(&mut self.annotation_state.annotation_ids, &mut board.annotation_ids);
                    std::mem::swap(&mut self.annotation_state.annotation_owners, &mut board.annotation_owners);
                }

                // Apply new annotation
                if let Some(ann) = result.new_annotation {
                    let id = rand::random::<u64>();
                    let wire = crate::types::local_to_wire(&ann);
                    self.send_message(PeerMessage::SetAnnotation {
                        board_id,
                        id,
                        annotation: wire,
                        owner: self.session.my_user_id,
                    });
                    self.apply_set_annotation(board_id, id, ann, self.session.my_user_id);
                }

                // Apply erase
                if let Some(idx) = result.erase_index {
                    let (_, ids, _) = self.get_annotation_lists_mut(board_id);
                    if let Some(&ann_id) = ids.get(idx) {
                        self.send_message(PeerMessage::RemoveAnnotation { board_id, id: ann_id });
                        self.apply_remove_annotation(board_id, ann_id);
                    }
                }
            } else {
                // No tool active — handle annotation select/move/rotate
                let result = handle_annotation_select_move(&mut self.annotation_state, &response, &transform);

                // Sync moved/rotated annotations to collab
                if let Some(idx) = result.rotation_stopped_index
                    && idx < self.annotation_state.annotations.len()
                {
                    let wire = crate::types::local_to_wire(&self.annotation_state.annotations[idx]);
                    let id = self.annotation_state.annotation_ids[idx];
                    let owner = self.annotation_state.annotation_owners.get(idx).copied().unwrap_or(0);
                    self.send_message(PeerMessage::SetAnnotation { board_id, id, annotation: wire, owner });
                }
                for &idx in &result.moved_indices {
                    if idx < self.annotation_state.annotations.len() {
                        let wire = crate::types::local_to_wire(&self.annotation_state.annotations[idx]);
                        let id = self.annotation_state.annotation_ids[idx];
                        let owner = self.annotation_state.annotation_owners.get(idx).copied().unwrap_or(0);
                        self.send_message(PeerMessage::SetAnnotation { board_id, id, annotation: wire, owner });
                    }
                }

                // Swap back
                if let Some(bid) = board_id
                    && let Some(board) = self.session.tactics_boards.get_mut(&bid)
                {
                    std::mem::swap(&mut self.annotation_state.annotations, &mut board.annotations);
                    std::mem::swap(&mut self.annotation_state.annotation_ids, &mut board.annotation_ids);
                    std::mem::swap(&mut self.annotation_state.annotation_owners, &mut board.annotation_owners);
                }

                // Click on empty space → ping (only if click didn't select anything)
                if result.selected_by_click && !self.annotation_state.has_selection() {
                    ping_on_empty_click = true;
                }
            }

            // Right-click: open context menu
            if response.secondary_clicked()
                && let Some(click_pos) = response.interact_pointer_pos()
            {
                self.annotation_state.active_tool = PaintTool::None;
                self.annotation_state.show_context_menu = true;
                self.annotation_state.context_menu_pos = click_pos;
            }

            // Ping on empty click
            if ping_on_empty_click && let Some(pointer) = response.interact_pointer_pos() {
                let minimap_pos = transform.screen_to_minimap(pointer);
                let pos = [minimap_pos.x, minimap_pos.y];
                let my_id = self.session.my_user_id;
                let color = self
                    .session
                    .cursors
                    .iter()
                    .find(|c| c.user_id == my_id)
                    .map(|c| c.color)
                    .unwrap_or([255, 255, 255]);
                self.session.pings.push(MapPing { pos, color, time: web_time::Instant::now() });
                self.send_message(PeerMessage::Ping { user_id: self.session.my_user_id, pos, color });
            }
        } else if is_tactics {
            // Annotations locked — still allow pings on click
            if response.clicked()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let minimap_pos = transform.screen_to_minimap(pointer);
                let pos = [minimap_pos.x, minimap_pos.y];
                let my_id = self.session.my_user_id;
                let color = self
                    .session
                    .cursors
                    .iter()
                    .find(|c| c.user_id == my_id)
                    .map(|c| c.color)
                    .unwrap_or([255, 255, 255]);
                self.session.pings.push(MapPing { pos, color, time: web_time::Instant::now() });
                self.send_message(PeerMessage::Ping { user_id: self.session.my_user_id, pos, color });
            }
        }

        // Context menu (right-click)
        if self.annotation_state.show_context_menu {
            let menu_pos = self.annotation_state.context_menu_pos;
            let mut menu_did_undo = false;
            let mut menu_did_clear = false;
            let menu_resp = egui::Area::new(ui.id().with("web_paint_menu"))
                .order(egui::Order::Foreground)
                .fixed_pos(menu_pos)
                .interactable(true)
                .show(ctx, |ui| {
                    let frame = egui::Frame::NONE
                        .fill(Color32::from_gray(30))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(8))
                        .stroke(egui::Stroke::new(1.0, Color32::from_gray(80)));
                    frame.show(ui, |ui| {
                        ui.set_min_width(160.0);
                        let result = wt_collab_egui::toolbar::draw_annotation_menu_common(
                            ui,
                            &mut self.annotation_state,
                            Some(&self.assets.ship_icons),
                        );
                        menu_did_undo = result.did_undo;
                        menu_did_clear = result.did_clear;
                    });
                });

            let board_id = match self.session.active_view {
                ActiveView::TacticsBoard(id) => Some(id),
                _ => None,
            };
            if menu_did_clear {
                self.send_message(PeerMessage::ClearAnnotations { board_id });
                if let Some(bid) = board_id {
                    self.apply_clear_annotations(Some(bid));
                }
            }
            if menu_did_undo {
                self.sync_annotations_after_undo(board_id);
            }

            if menu_resp.response.clicked_elsewhere() {
                self.annotation_state.show_context_menu = false;
            }
        }
    }

    fn render_annotation_toolbar(&mut self, ui: &mut egui::Ui) {
        let locked = self.session.permissions.annotations_locked;
        let result = draw_annotation_toolbar(ui, &mut self.annotation_state, Some(&self.assets.ship_icons), locked);

        let board_id = match self.session.active_view {
            ActiveView::TacticsBoard(id) => Some(id),
            _ => None,
        };

        if result.did_clear {
            self.send_message(PeerMessage::ClearAnnotations { board_id });
            if let Some(bid) = board_id {
                self.apply_clear_annotations(Some(bid));
            }
        }
        if result.did_undo {
            // Sync full annotation state after undo
            self.sync_annotations_after_undo(board_id);
        }
    }

    fn render_info_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // View name
            match &self.session.active_view {
                ActiveView::TacticsBoard(id) => {
                    let name = self
                        .session
                        .tactics_boards
                        .get(id)
                        .map(|b| if b.display_name.is_empty() { b.map_name.as_str() } else { b.display_name.as_str() })
                        .unwrap_or("Tactics Board");
                    ui.label(format!("Tactics: {name}"));
                }
                ActiveView::Replay(id) => {
                    let name =
                        self.session
                            .replay_views
                            .get(id)
                            .map(|v| {
                                if v.display_name.is_empty() { v.replay_name.as_str() } else { v.display_name.as_str() }
                            })
                            .unwrap_or("Replay");
                    ui.label(format!("Replay: {name}"));
                }
                ActiveView::Lobby => {
                    ui.label("Lobby");
                }
            }

            ui.separator();

            // Connected users
            for user in &self.session.connected_users {
                let c = Color32::from_rgb(user.color[0], user.color[1], user.color[2]);
                ui.colored_label(c, &user.name);
            }
        });
    }

    fn get_active_map_texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        match self.session.active_view {
            ActiveView::TacticsBoard(id) => {
                if let Some(board) = self.session.tactics_boards.get_mut(&id) {
                    if board.map_texture.is_none()
                        && let Some(ref png_data) = board.map_image_png
                        && let Ok(img) = image_from_png(png_data)
                    {
                        let tex = ctx.load_texture(format!("map_{id}"), img, egui::TextureOptions::LINEAR);
                        board.map_texture = Some(tex);
                    }
                    board.map_texture.clone()
                } else {
                    None
                }
            }
            ActiveView::Replay(id) => {
                if let Some(view) = self.session.replay_views.get_mut(&id) {
                    if view.map_texture.is_none()
                        && let Some(ref png_data) = view.map_image_png
                        && let Ok(img) = image_from_png(png_data)
                    {
                        let tex = ctx.load_texture(format!("replay_map_{id}"), img, egui::TextureOptions::LINEAR);
                        view.map_texture = Some(tex);
                    }
                    view.map_texture.clone()
                } else {
                    None
                }
            }
            ActiveView::Lobby => None,
        }
    }

    fn get_active_frame_commands(&self) -> Option<Vec<wows_minimap_renderer::DrawCommand>> {
        if let ActiveView::Replay(id) = self.session.active_view
            && let Some(view) = self.session.replay_views.get(&id)
            && let Some(ref frame) = view.current_frame
        {
            return Some(frame.commands.clone());
        }
        None
    }

    fn get_active_annotations(&self) -> Vec<Annotation> {
        match self.session.active_view {
            ActiveView::TacticsBoard(id) => {
                self.session.tactics_boards.get(&id).map(|b| b.annotations.clone()).unwrap_or_default()
            }
            ActiveView::Replay(id) => {
                self.session.replay_views.get(&id).map(|v| v.annotations.clone()).unwrap_or_default()
            }
            ActiveView::Lobby => Vec::new(),
        }
    }

    /// Check if a tab already exists in the dock.
    fn dock_has_tab(&self, tab: &WebTab) -> bool {
        self.dock_state.find_tab(tab).is_some()
    }

    /// Queue a tab to be added after the current dock render pass completes.
    fn dock_push_tab(&mut self, tab: WebTab) {
        self.pending_tabs.push(tab);
    }

    /// Remove a tab from the dock.
    fn dock_remove_tab(&mut self, tab: &WebTab) {
        if let Some(location) = self.dock_state.find_tab(tab) {
            self.dock_state.remove_tab(location);
        }
    }
}

/// TabViewer implementation for the web client dock.
struct WebTabViewer<'a> {
    app: &'a mut WebApp,
    ctx: &'a egui::Context,
}

impl TabViewer for WebTabViewer<'_> {
    type Tab = WebTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title(&self.app.session).into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        // Set active_view to match the tab being rendered.
        self.app.session.active_view = tab.to_active_view();

        match tab {
            WebTab::Lobby => {
                self.app.render_lobby(ui);
            }
            WebTab::TacticsBoard(_) | WebTab::Replay(_) => {
                if self.app.assets.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 3.0);
                        ui.spinner();
                        if self.app.asset_request_count >= 5 {
                            ui.label("Failed to receive map data from host.");
                        } else {
                            ui.label("Waiting for map data from host...");
                        }
                    });
                } else {
                    self.app.render_annotation_toolbar(ui);
                    ui.separator();
                    self.app.render_map_view(ui, self.ctx);
                }
            }
        }
    }

    fn closeable(&mut self, tab: &mut Self::Tab) -> bool {
        // Lobby tab can't be closed.
        !matches!(tab, WebTab::Lobby)
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> egui_dock::tab_viewer::OnCloseResponse {
        // Close the dock tab but keep session data so the window can be re-opened
        // from the lobby. Host-side ReplayClosed / TacticsMapClosed messages
        // handle actual data cleanup.
        match tab {
            WebTab::Lobby => egui_dock::tab_viewer::OnCloseResponse::Ignore,
            WebTab::Replay(_) | WebTab::TacticsBoard(_) => {
                self.app.closed_tabs.push(tab.clone());
                egui_dock::tab_viewer::OnCloseResponse::Close
            }
        }
    }
}

impl eframe::App for WebApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Register default GameFont family on first frame (can't do in new() — fonts not ready)
        if !self.fonts_registered {
            self.fonts_registered = true;
            let mut font_defs = ctx.fonts(|r| r.definitions().clone());
            if !font_defs.families.contains_key(&egui::FontFamily::Name("GameFont".into())) {
                let proportional = font_defs.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
                font_defs.families.insert(egui::FontFamily::Name("GameFont".into()), proportional);
                ctx.set_fonts(font_defs);
            }
        }

        // Poll connection events
        self.poll_events(ctx);

        // Request assets if connected, a board/replay is open, but none received yet.
        let needs_assets = !self.session.tactics_boards.is_empty() || !self.session.replay_views.is_empty();
        if matches!(self.connection_status, ConnectionStatus::Connected)
            && needs_assets
            && self.assets.is_empty()
            && self.asset_request_count < 5
        {
            let should_request = match self.last_asset_request {
                None => true,
                Some(t) => t.elapsed().as_secs() >= 3,
            };
            if should_request {
                self.asset_request_count += 1;
                self.last_asset_request = Some(web_time::Instant::now());
                tracing::info!("Requesting assets from host ({}/5)", self.asset_request_count);
                self.send_message(PeerMessage::RequestAssets);
            }
        }

        // Info bar at bottom
        egui::TopBottomPanel::bottom("info_bar").show(ctx, |ui| {
            self.render_info_bar(ui);
        });

        // Dock area for tabbed views
        egui::CentralPanel::default().show(ctx, |ui| {
            // Temporarily take the dock state out of self so we can pass
            // &mut self to the TabViewer without double-borrow.
            let mut dock_state = std::mem::replace(&mut self.dock_state, DockState::new(vec![WebTab::Lobby]));

            DockArea::new(&mut dock_state)
                .style(egui_dock::Style::from_egui(ui.style().as_ref()))
                .allowed_splits(egui_dock::AllowedSplits::None)
                .show_leaf_collapse_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_inside(ui, &mut WebTabViewer { app: self, ctx });

            self.dock_state = dock_state;

            // Apply any tabs queued during rendering (e.g. from lobby re-open buttons).
            for tab in self.pending_tabs.drain(..) {
                if self.dock_state.find_tab(&tab).is_none() {
                    self.dock_state.push_to_focused_leaf(tab);
                }
            }
        });
    }
}

/// Decode a PNG image into an egui ColorImage.
fn image_from_png(png_data: &[u8]) -> Result<egui::ColorImage, String> {
    let cursor = std::io::Cursor::new(png_data);
    let decoder = png::Decoder::new(cursor);
    let mut reader = decoder.read_info().map_err(|e| format!("PNG decode error: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("PNG frame error: {e}"))?;
    let bytes = &buf[..info.buffer_size()];

    let size = [info.width as usize, info.height as usize];
    match info.color_type {
        png::ColorType::Rgba => Ok(egui::ColorImage::from_rgba_unmultiplied(size, bytes)),
        png::ColorType::Rgb => {
            let mut rgba = Vec::with_capacity(bytes.len() / 3 * 4);
            for chunk in bytes.chunks(3) {
                rgba.extend_from_slice(chunk);
                rgba.push(255);
            }
            Ok(egui::ColorImage::from_rgba_unmultiplied(size, &rgba))
        }
        _ => Err(format!("Unsupported PNG color type: {:?}", info.color_type)),
    }
}

/// Extract the session token from the URL hash fragment.
///
/// Expected format: `#toolkit-<base64>` or `#toolkit-<base64>`
fn extract_session_token() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window()?;
        let location = window.location();
        let hash = location.hash().ok()?;
        let hash = hash.strip_prefix('#')?;
        if hash.starts_with("toolkit-") {
            return Some(hash.to_string());
        }
        None
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// Read a value from localStorage.
fn local_storage_get(key: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let storage = web_sys::window()?.local_storage().ok()??;
        storage.get_item(key).ok()?
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = key;
        None
    }
}

/// Write a value to localStorage.
fn local_storage_set(key: &str, value: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok()).flatten() {
            let _ = storage.set_item(key, value);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (key, value);
    }
}
