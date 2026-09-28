//! What the app asks the reader once, and the confirmations that stand between
//! a command and work that cannot be taken back.
//!
//! The egui app asks the same questions from its own windows (`app.rs`'s
//! `build_consent_window_open`, `replay_migration_window_open`,
//! `language_selection_open` and `refresh_persisted_data_window_open`) and
//! records the answers in the rows both apps read, so a reader who answered in
//! either is not asked again in the other.

use gpui_kit::App;
use gpui_kit::Entity;
use gpui_kit::InteractiveElement as _;
use gpui_kit::ParentElement as _;
use gpui_kit::Styled as _;
use gpui_kit::Window;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::h_flex;
use rust_i18n::t;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::keys;

use crate::app::App as Toolkit;
use crate::settings_store;

/// Asks what battle data may be shared, on a first run.
///
/// Three answers rather than a yes or no, which is why this is a dialog of its
/// own rather than an alert: the reader chooses between sending replays, sending
/// only their builds, and sending nothing.
pub fn ask_build_consent(app: &Entity<Toolkit>, window: &mut Window, cx: &mut App) {
    let asked = app.downgrade();
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let asked = asked.clone();
        dialog
            .title(t!("ui.windows.build_consent").into_owned())
            .close_button(false)
            .child(
                gpui_kit::div()
                    .id("first-run-build-consent")
                    .max_w(gpui_kit::px(520.))
                    .text_sm()
                    .child(t!("ui.dialogs.build_consent_message").into_owned()),
            )
            .footer(chosen_row([
                (
                    "first-run-consent-replays",
                    t!("ui.buttons.send_replays").into_owned(),
                    DataSharingMode::Replays,
                    asked.clone(),
                ),
                (
                    "first-run-consent-builds",
                    t!("ui.buttons.send_build_data").into_owned(),
                    DataSharingMode::BuildData,
                    asked.clone(),
                ),
                ("first-run-consent-nothing", t!("ui.buttons.send_nothing").into_owned(), DataSharingMode::Off, asked),
            ]))
    });
}

/// Offers sharing whole replays to a reader who consented before that existed.
pub fn ask_replay_migration(app: &Entity<Toolkit>, window: &mut Window, cx: &mut App) {
    let asked = app.downgrade();
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let switch = asked.clone();
        dialog
            .title(t!("ui.windows.replay_migration").into_owned())
            .close_button(false)
            .child(
                gpui_kit::div()
                    .id("first-run-replay-migration")
                    .max_w(gpui_kit::px(520.))
                    .text_sm()
                    .child(t!("ui.dialogs.replay_migration_message").into_owned()),
            )
            .footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child({
                        let switch = switch.clone();
                        Button::new("first-run-migration-switch")
                            .primary()
                            .label(t!("ui.buttons.switch_to_replays").into_owned())
                            .small()
                            .on_click(move |_event, window, cx: &mut App| {
                                if let Some(app) = switch.upgrade() {
                                    app.update(cx, |this, cx| {
                                        this.adopt_data_sharing(DataSharingMode::Replays, cx);
                                    });
                                }
                                settings_store::save(keys::REPLAY_CONSENT_SHOWN, &true, cx);
                                window.close_dialog(cx);
                            })
                    })
                    .child({
                        Button::new("first-run-migration-keep")
                            .label(t!("ui.buttons.keep_current").into_owned())
                            .small()
                            .on_click(move |_event, window, cx: &mut App| {
                                // The setting stands; only the asking is recorded.
                                settings_store::save(keys::REPLAY_CONSENT_SHOWN, &true, cx);
                                window.close_dialog(cx);
                            })
                    }),
            )
    });
}

/// Asks which language to read in, when the machine's own is not English.
///
/// The warning is the point of the question: every language but English is
/// machine-translated, and a reader who would rather read the original is told so
/// before they meet one.
pub fn ask_language(app: &Entity<Toolkit>, detected: String, window: &mut Window, cx: &mut App) {
    let native = wt_translations::language_name(&detected).unwrap_or("English").to_owned();
    let asked = app.downgrade();

    window.open_dialog(cx, move |dialog, _window, _cx| {
        let english = asked.clone();
        let keep = asked.clone();
        let detected = detected.clone();
        // The catalogue's own label, which reads in the detected language once
        // that catalogue is loaded; in English it names the language instead.
        let keep_label = {
            let offered = t!("dialog.continue_in_language").into_owned();
            if offered == "Continue in English" { format!("Continue in {native}") } else { offered }
        };

        dialog
            .title(t!("dialog.select_language").into_owned())
            .close_button(false)
            .child(
                gpui_kit::div()
                    .id("first-run-language")
                    .max_w(gpui_kit::px(480.))
                    .text_sm()
                    .child(t!("dialog.machine_translation_warning").into_owned()),
            )
            .footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child({
                        let english = english.clone();
                        Button::new("first-run-language-english")
                            .label(t!("dialog.continue_in_english").into_owned())
                            .small()
                            .on_click(move |_event, window, cx: &mut App| {
                                if let Some(app) = english.upgrade() {
                                    app.update(cx, |this, cx| this.set_locale("en".to_owned(), window, cx));
                                }
                                settings_store::save(keys::LANGUAGE_SELECTION_SHOWN, &true, cx);
                                window.close_dialog(cx);
                            })
                    })
                    .child({
                        let keep = keep.clone();
                        let kept = detected.clone();
                        Button::new("first-run-language-keep").primary().label(keep_label.clone()).small().on_click(
                            move |_event, window, cx: &mut App| {
                                // Written rather than assumed: the row may never
                                // have held a language, and this question is what
                                // settles it.
                                if let Some(app) = keep.upgrade() {
                                    let kept = kept.clone();
                                    app.update(cx, |this, cx| this.set_locale(kept, window, cx));
                                }
                                settings_store::save(keys::LANGUAGE_SELECTION_SHOWN, &true, cx);
                                window.close_dialog(cx);
                            },
                        )
                    }),
            )
    });
}

/// Confirms re-reading every replay and rewriting what the index holds.
///
/// Long and not worth starting twice, so it is asked for rather than run from a
/// menu item, as the egui app asks.
pub fn confirm_refresh_persisted_data(app: &Entity<Toolkit>, window: &mut Window, cx: &mut App) {
    let asked = app.downgrade();
    window.open_alert_dialog(cx, move |alert, _window, _cx| {
        let asked = asked.clone();
        alert
            .title(t!("ui.windows.refresh_persisted_data").into_owned())
            .description(t!("ui.dialogs.refresh_persisted_data_message").into_owned())
            .ok_text(t!("ui.buttons.refresh_persisted_data_confirm").into_owned())
            .show_cancel(true)
            .on_ok(move |_event, _window, cx| {
                if let Some(app) = asked.upgrade() {
                    app.update(cx, |this, cx| {
                        this.build_replay_index(crate::replay_index::IndexMode::RefreshAll, cx);
                    });
                }
                true
            })
    });
}

/// The consent dialog's row of answers: one button per choice.
fn chosen_row(
    choices: [(&'static str, String, DataSharingMode, gpui_kit::WeakEntity<Toolkit>); 3],
) -> impl gpui_kit::IntoElement {
    let mut row = h_flex().gap_2().justify_end();
    for (id, label, mode, asked) in choices {
        row = row.child(Button::new(id).label(label).small().on_click(move |_event, window, cx: &mut App| {
            if let Some(app) = asked.upgrade() {
                app.update(cx, |this, cx| this.adopt_data_sharing(mode, cx));
            }
            // Answering the first question answers the second: the reader has
            // been told what sharing replays means and chosen.
            settings_store::save(keys::BUILD_CONSENT_SHOWN, &true, cx);
            settings_store::save(keys::REPLAY_CONSENT_SHOWN, &true, cx);
            window.close_dialog(cx);
        }));
    }
    row
}

/// Which question a fresh run has to ask, if any.
///
/// The order is the egui app's: what may be shared comes before the offer to
/// share more, and the language question stands on its own.
pub fn pending(answered: &Answered) -> Option<Question> {
    if !answered.build_consent {
        return Some(Question::BuildConsent);
    }
    if !answered.replay_consent {
        return Some(Question::ReplayMigration);
    }
    if !answered.language {
        // English needs no warning about machine translation, so answering it is
        // the same as never asking.
        let locale = answered.locale.as_deref().unwrap_or("en");
        if locale == "en" {
            return Some(Question::NoLanguageToAsk);
        }
        return Some(Question::Language { detected: locale.to_owned() });
    }
    None
}

/// What the reader has already been asked, as the shared rows record it.
pub struct Answered {
    pub build_consent: bool,
    pub replay_consent: bool,
    pub language: bool,
    /// The language the machine reports, which is what the question offers.
    pub locale: Option<String>,
}

impl From<&crate::settings::GpuiSettings> for Answered {
    fn from(settings: &crate::settings::GpuiSettings) -> Self {
        Self {
            build_consent: settings.build_consent_shown,
            replay_consent: settings.replay_consent_shown,
            language: settings.language_selection_shown,
            locale: settings.locale.clone(),
        }
    }
}

/// What a fresh run still has to ask.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    BuildConsent,
    ReplayMigration,
    Language {
        detected: String,
    },
    /// Nothing to ask, but the language question is answered by the machine
    /// being English and is recorded as asked.
    NoLanguageToAsk,
}

/// Puts the pending question, if there is one.
pub fn ask(app: &Entity<Toolkit>, settings: &crate::settings::GpuiSettings, window: &mut Window, cx: &mut App) {
    match pending(&Answered::from(settings)) {
        Some(Question::BuildConsent) => ask_build_consent(app, window, cx),
        Some(Question::ReplayMigration) => ask_replay_migration(app, window, cx),
        Some(Question::Language { detected }) => ask_language(app, detected, window, cx),
        Some(Question::NoLanguageToAsk) => settings_store::save(keys::LANGUAGE_SELECTION_SHOWN, &true, cx),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::Answered;
    use super::Question;
    use super::pending;

    /// A fresh run asks what may be shared first, then whether to share more,
    /// then which language to read in.
    #[test]
    fn the_questions_come_in_the_egui_apps_order() {
        let mut answered =
            Answered { build_consent: false, replay_consent: false, language: false, locale: Some("de".to_owned()) };
        assert_eq!(pending(&answered), Some(Question::BuildConsent));

        answered.build_consent = true;
        assert_eq!(pending(&answered), Some(Question::ReplayMigration));

        answered.replay_consent = true;
        assert_eq!(pending(&answered), Some(Question::Language { detected: "de".to_owned() }));

        answered.language = true;
        assert_eq!(pending(&answered), None, "a reader who has answered is not asked again");
    }

    /// An English machine is not warned about machine translation; the question
    /// is recorded as asked instead.
    #[test]
    fn an_english_machine_is_not_asked_about_language() {
        let answered = Answered { build_consent: true, replay_consent: true, language: false, locale: None };

        assert_eq!(pending(&answered), Some(Question::NoLanguageToAsk));
    }
}
