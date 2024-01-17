use std::sync::{atomic::Ordering, Arc};

use egui::{Button, Color32, RichText, TextEdit, Widget};

use twitch_irc::{
    login::StaticLoginCredentials,
    message::{IRCMessage, ServerMessage},
    ClientConfig, SecureTCPTransport, TwitchIRCClient,
};

use tokio::time::{sleep, Duration};

use crate::{app::ToolkitTabViewer, icons, replay_parser::Replay, task::BackgroundTaskCompletion, util::build_short_ship_config_url, wows_data::WorldOfWarshipsData};

const MAX_PARSE_ATTEMPTS: u8 = 3;
const PARSE_RETRY_DELAY: u64 = 5000;

fn parse_url(wows_data: &Arc<WorldOfWarshipsData>) -> Option<String> {
    let task = wows_data.parse_live_replay()?;
    let completion = task.receiver.recv().ok()?.ok()?;

    match completion {
        BackgroundTaskCompletion::ReplayLoaded { replay: r } => {
            let replay = r.read();
            Some(build_short_ship_config_url(
                &(replay).battle_report.as_ref()?.self_entity(),
                &wows_data.game_metadata.clone().unwrap(),
            ))
        }
        _ => None,
    }
}

impl ToolkitTabViewer<'_> {
    fn toggle_twitch_connection(&mut self) {
        let settings = &self.tab_state.settings.twitch;
        let login_name = settings.login.to_owned();
        let oauth_token = settings.token.to_owned();
        let command = settings.command.to_owned();
        let template = settings.template.to_owned();
        let unavailable = settings.unavailable.to_owned();
        let error = settings.error.to_owned();
        let new_match_loaded = settings.new_match_loaded.clone();

        if let Some(wows_data) = &self.tab_state.world_of_warships_data {
            let wows_data = wows_data.clone();

            self.runtime.spawn(async move {
                let credentials = StaticLoginCredentials::new(login_name.clone(), Some(oauth_token));
                let client_config = ClientConfig::new_simple(credentials);
                let (mut incoming_messages, client) = TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(client_config);
                client.join(login_name.clone()).unwrap();

                let join_handle = tokio::spawn(async move {
                    let mut cached_build_url = None;
                    while let Some(message) = incoming_messages.recv().await {
                        if let ServerMessage::Privmsg(msg) = message {
                            if msg.message_text != command {
                                continue;
                            };

                            // If a new match was loaded we need to invalidate our cached build
                            let mut use_cached_build = cached_build_url.is_some();
                            if let Ok(true) = new_match_loaded.compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed) {
                                use_cached_build = false;
                            };

                            if !use_cached_build {
                                for i in 0..MAX_PARSE_ATTEMPTS {
                                    cached_build_url = parse_url(&wows_data);
                                    if cached_build_url.is_none() && i < MAX_PARSE_ATTEMPTS - 1 {
                                        sleep(Duration::from_millis(PARSE_RETRY_DELAY)).await;
                                    };
                                }
                            }

                            if cached_build_url.is_some() {
                                client
                                    .say(login_name.clone(), template.replace("{url}", &cached_build_url.as_ref().unwrap()[..]))
                                    .await
                                    .unwrap();
                            } else {
                                client.say(login_name.clone(), error.clone()).await.unwrap();
                            }
                        }
                    }
                });

                join_handle.await.unwrap();
            });

            self.tab_state.twitch_connection = true;
        }
    }

    pub fn build_twitch_builds_tab(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label("Twitch Login");
            ui.group(|ui| {
                ui.add(TextEdit::singleline(&mut self.tab_state.settings.twitch.login).hint_text("Username"));
                ui.add(TextEdit::singleline(&mut self.tab_state.settings.twitch.token).hint_text("Token").password(true));

                let (text, text_color, fill);
                if self.tab_state.twitch_connection {
                    text = "Disconnect";
                    text_color = Color32::WHITE;
                    fill = Color32::from_rgb(173, 58, 58);
                } else {
                    text = "Connect";
                    text_color = Color32::BLACK;
                    fill = Color32::LIGHT_GRAY;
                }
                if Button::new(RichText::new(text).color(text_color)).fill(fill).ui(ui).clicked() {
                    self.toggle_twitch_connection();
                }
            });
            ui.add_space(15.0);

            ui.label("Message Templates");
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Command: ");
                    ui.add(TextEdit::singleline(&mut self.tab_state.settings.twitch.command).desired_width(321.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Build: ");
                    ui.add(TextEdit::singleline(&mut self.tab_state.settings.twitch.template).desired_width(350.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Unavailable: ");
                    ui.add(TextEdit::singleline(&mut self.tab_state.settings.twitch.unavailable).desired_width(316.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Error: ");
                    ui.add(TextEdit::singleline(&mut self.tab_state.settings.twitch.error).desired_width(349.0));
                });
            });
            ui.horizontal(|ui| {
                ui.label(format!("{} Build template must contain", icons::WARNING_CIRCLE));
                ui.code("{{url}}");
            });
            ui.add_space(15.0);

            ui.label("Information");
            ui.add(
                TextEdit::multiline(&mut concat!(
                    "You can make your active build available to a Twitch audience here.\n",
                    "Generate credentials using a service like https://twitchtokengenerator.com.\n",
                    "If the connection fails, you may need to generate a new token.\n",
                ))
                .interactive(false)
                .frame(false)
                .desired_width(500.0),
            );
        });
    }
}
