//! What the app asks the reader once.
//!
//! The egui and GPUI apps ask the same questions and record the answers in the rows all of them
//! read, so a reader who answered in one is not asked again in another.

use rust_i18n::t;
use wows_toolkit_services::settings::StartupSettings;
use wows_toolkit_viewmodel::settings::DataSharingMode;
use wows_toolkit_viewmodel::settings::keys;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetValue;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DialogContentProps;
use zgui_ui::DialogDescriptionProps;
use zgui_ui::DialogFooterProps;
use zgui_ui::DialogHeaderProps;
use zgui_ui::DialogProps;
use zgui_ui::DialogTitleProps;

use crate::i18n::LanguageCode;
use crate::services::Services;
use crate::shell::DialogHandle;
use crate::shell::Shell;

const SHEET: &str = "wt-first-run";

const CSS: &str = css!(".wt-first-run { max-width: 520px; }");

/// What the reader has already been asked, as the shared rows record it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answered {
    pub build_consent: bool,
    pub replay_consent: bool,
    pub language: bool,
    /// The language the settings hold, which is what the question offers. `None` reads as
    /// English.
    pub locale: Option<String>,
}

impl Answered {
    pub fn of(settings: &StartupSettings) -> Self {
        Self {
            build_consent: settings.build_consent_shown,
            replay_consent: settings.replay_consent_shown,
            language: settings.language_selection_shown,
            locale: settings.locale.clone(),
        }
    }
}

/// What a fresh run still has to ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Question {
    BuildConsent,
    ReplayMigration,
    Language {
        detected: LanguageCode,
    },
    /// Nothing to ask, but the language question is answered by the machine being English and is
    /// recorded as asked.
    NoLanguageToAsk,
}

/// Which question a fresh run has to ask, if any, in the egui app's order: what may be shared
/// comes before the offer to share more, and the language question stands on its own.
pub fn pending(answered: &Answered) -> Option<Question> {
    if !answered.build_consent {
        return Some(Question::BuildConsent);
    }
    if !answered.replay_consent {
        return Some(Question::ReplayMigration);
    }
    if answered.language {
        return None;
    }

    // English needs no warning about machine translation, so answering it is the same as never
    // asking.
    match answered.locale.as_deref() {
        None | Some("en") => Some(Question::NoLanguageToAsk),
        Some(locale) => Some(Question::Language { detected: LanguageCode::new(locale) }),
    }
}

/// Puts the pending question, if there is one.
pub fn ask(services: Services, shell: Shell, answered: Answered) {
    match pending(&answered) {
        Some(Question::BuildConsent) => ask_build_consent(shell),
        Some(Question::ReplayMigration) => ask_replay_migration(shell),
        Some(Question::Language { detected }) => ask_language(shell, detected),
        Some(Question::NoLanguageToAsk) => services.settings.save(keys::LANGUAGE_SELECTION_SHOWN, &true),
        None => {}
    }
}

/// A question that has to be answered: no close button, and a press beside it does not dismiss it.
#[derive(Clone, Copy)]
struct Asked {
    handle: DialogHandle,
}

impl Asked {
    fn answered(self) {
        self.handle.close();
    }
}

/// Asks what battle data may be shared, on a first run: three answers rather than a yes or no.
fn ask_build_consent(shell: Shell) {
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let asked = Asked { handle };
        let open = RwSignal::new_local(true);
        let choose = move |mode: DataSharingMode| {
            move |_: &mut zgui::view::EventCx<'_, zgui::view::events::Click>| {
                shell.adopt_data_sharing(mode);
                // Answering the first question answers the second: the reader has been told
                // what sharing replays means and chosen.
                shell.services().settings.save(keys::BUILD_CONSENT_SHOWN, &true);
                shell.services().settings.save(keys::REPLAY_CONSENT_SHOWN, &true);
                asked.answered();
            }
        };

        AnyView::new(view! {
            Dialog(open = open) {
                DialogContent(dismiss_on_outside_press = false, dismiss_on_escape = false, dismiss_control = false, attr:data-testid = "first-run-build-consent") {
                    DialogHeader {
                        DialogTitle {{t!("ui.windows.build_consent").into_owned()}}
                        DialogDescription(class = "wt-first-run") {{t!("ui.dialogs.build_consent_message").into_owned()}}
                    }
                    DialogFooter {
                        Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline,
                            attr:data-testid = "first-run-consent-replays",
                            on:click = choose(DataSharingMode::Replays)) {
                            {t!("ui.buttons.send_replays").into_owned()}
                        }
                        Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline,
                            attr:data-testid = "first-run-consent-builds",
                            on:click = choose(DataSharingMode::BuildData)) {
                            {t!("ui.buttons.send_build_data").into_owned()}
                        }
                        Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline,
                            attr:data-testid = "first-run-consent-nothing",
                            on:click = choose(DataSharingMode::Off)) {
                            {t!("ui.buttons.send_nothing").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

/// Offers sharing whole replays to a reader who consented before that existed.
fn ask_replay_migration(shell: Shell) {
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let asked = Asked { handle };
        let open = RwSignal::new_local(true);

        AnyView::new(view! {
            Dialog(open = open) {
                DialogContent(dismiss_on_outside_press = false, dismiss_on_escape = false, dismiss_control = false, attr:data-testid = "first-run-replay-migration") {
                    DialogHeader {
                        DialogTitle {{t!("ui.windows.replay_migration").into_owned()}}
                        DialogDescription(class = "wt-first-run") {{t!("ui.dialogs.replay_migration_message").into_owned()}}
                    }
                    DialogFooter {
                        Button(size = ButtonSize::Sm,
                            attr:data-testid = "first-run-migration-switch",
                            on:click = move |_| {
                                shell.adopt_data_sharing(DataSharingMode::Replays);
                                shell.services().settings.save(keys::REPLAY_CONSENT_SHOWN, &true);
                                asked.answered();
                            }) {
                            {t!("ui.buttons.switch_to_replays").into_owned()}
                        }
                        Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline,
                            attr:data-testid = "first-run-migration-keep",
                            on:click = move |_| {
                                // The setting stands; only the asking is recorded.
                                shell.services().settings.save(keys::REPLAY_CONSENT_SHOWN, &true);
                                asked.answered();
                            }) {
                            {t!("ui.buttons.keep_current").into_owned()}
                        }
                    }
                }
            }
        })
    });
}

/// Asks which language to read in, when the saved one is not English. The warning is the point of
/// the question: every language but English is machine-translated.
fn ask_language(shell: Shell, detected: LanguageCode) {
    shell.dialogs.open(move |handle| {
        install_stylesheet(SHEET, CSS);
        let asked = Asked { handle };
        let open = RwSignal::new_local(true);
        let native = wt_translations::language_name(detected.as_str()).unwrap_or("English").to_owned();
        // The catalogue's own label, which reads in the detected language once that catalogue is
        // loaded; in English it names the language instead.
        let keep_label = StoredValue::new({
            let offered = t!("dialog.continue_in_language").into_owned();
            if offered == "Continue in English" { format!("Continue in {native}") } else { offered }
        });
        let kept = StoredValue::new(detected.clone());

        AnyView::new(view! {
            Dialog(open = open) {
                DialogContent(dismiss_on_outside_press = false, dismiss_on_escape = false, dismiss_control = false, attr:data-testid = "first-run-language") {
                    DialogHeader {
                        DialogTitle {{t!("dialog.select_language").into_owned()}}
                        DialogDescription(class = "wt-first-run") {{t!("dialog.machine_translation_warning").into_owned()}}
                    }
                    DialogFooter {
                        Button(size = ButtonSize::Sm, variant = ButtonVariant::Outline,
                            attr:data-testid = "first-run-language-english",
                            on:click = move |_| {
                                shell.set_locale(LanguageCode::new("en"));
                                shell.services().settings.save(keys::LANGUAGE_SELECTION_SHOWN, &true);
                                asked.answered();
                            }) {
                            {t!("dialog.continue_in_english").into_owned()}
                        }
                        Button(size = ButtonSize::Sm,
                            attr:data-testid = "first-run-language-keep",
                            on:click = move |_| {
                                // Written rather than assumed: the row may never have held a
                                // language, and this question is what settles it.
                                shell.set_locale(kept.get_value());
                                shell.services().settings.save(keys::LANGUAGE_SELECTION_SHOWN, &true);
                                asked.answered();
                            }) {
                            {keep_label.get_value()}
                        }
                    }
                }
            }
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_questions_come_in_the_egui_apps_order() {
        let mut answered =
            Answered { build_consent: false, replay_consent: false, language: false, locale: Some("de".to_owned()) };
        assert_eq!(pending(&answered), Some(Question::BuildConsent));

        answered.build_consent = true;
        assert_eq!(pending(&answered), Some(Question::ReplayMigration));

        answered.replay_consent = true;
        assert_eq!(pending(&answered), Some(Question::Language { detected: LanguageCode::new("de") }));

        answered.language = true;
        assert_eq!(pending(&answered), None, "a reader who has answered is not asked again");
    }

    #[test]
    fn an_english_machine_is_not_asked_about_language() {
        let answered = Answered { build_consent: true, replay_consent: true, language: false, locale: None };
        assert_eq!(pending(&answered), Some(Question::NoLanguageToAsk));
    }
}
