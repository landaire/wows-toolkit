//! The toolkit's own colours on top of the landaire-ui palettes: what a colour means in a battle
//! (a win, a division mate, a penetration), held against the surfaces it is drawn on.
//!
//! The palettes themselves are `landaire_ui::Scheme`'s; the saved choice is the shared settings
//! row's `ThemeChoice`, which names the same schemes.

use landaire_ui::Appearance;
use landaire_ui::Rgb;
use landaire_ui::Scheme;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::view::IntoView;
use zgui::view::Stylesheet;

/// What the `--wt-*` properties are installed under.
const SHEET: &str = "wows-toolkit-semantic";

/// What a colour means, rather than what it looks like.
///
/// The two sets are not each other's inverse: a light panel caps how light a tone may be, so the
/// light set is tuned apart along lightness instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Semantic {
    pub win: Rgb,
    pub loss: Rgb,
    pub draw: Rgb,
    pub warn: Rgb,
    pub error: Rgb,
    pub ok: Rgb,
    pub text_strong: Rgb,
    pub text_dim: Rgb,
    /// Below dim: grammar punctuation and hints that are there to be found, not read.
    pub text_faint: Rgb,

    /// Division mates.
    pub division: Rgb,
    /// Players flagged by the abuse list.
    pub abuser: Rgb,
    /// A value worth noticing that is not a warning.
    pub notice: Rgb,

    pub chat_division: Rgb,
    pub chat_team: Rgb,
    pub chat_other: Rgb,

    /// What a shell did to the armor it met.
    pub armor_pen: Rgb,
    pub armor_overpen: Rgb,
    pub armor_ricochet: Rgb,
    pub armor_shatter: Rgb,
    /// How square a strike was: head-on, angled, glancing.
    pub armor_angle_good: Rgb,
    pub armor_angle_mid: Rgb,
    pub armor_angle_bad: Rgb,
}

impl Semantic {
    pub const DARK: Self = Self {
        win: Rgb::new(0x6fd98a),
        loss: Rgb::new(0xf28389),
        draw: Rgb::new(0xe9e5dd),
        warn: Rgb::new(0xe8a54a),
        error: Rgb::new(0xe8737b),
        ok: Rgb::new(0x6fd98a),
        text_strong: Rgb::new(0xe8e5dc),
        text_dim: Rgb::new(0x7c7c6e),
        text_faint: Rgb::new(0x5c5c52),
        division: Rgb::new(0xe5c158),
        abuser: Rgb::new(0xf09bc0),
        notice: Rgb::new(0xe5c158),
        chat_division: Rgb::new(0xe5c158),
        chat_team: Rgb::new(0x6fd98a),
        chat_other: Rgb::new(0xe8a54a),
        armor_pen: Rgb::new(0x6fd98a),
        armor_overpen: Rgb::new(0xe0be64),
        armor_ricochet: Rgb::new(0x7fb4e8),
        armor_shatter: Rgb::new(0xa9a49a),
        armor_angle_good: Rgb::new(0x64d98a),
        armor_angle_mid: Rgb::new(0xe0be64),
        armor_angle_bad: Rgb::new(0xe8737b),
    };

    /// Mid tones rather than near-black ones, so the win and loss tones read as green and red at a
    /// glance and still clear 4.5:1 on the panel and the striped rows.
    pub const LIGHT: Self = Self {
        win: Rgb::new(0x16703a),
        loss: Rgb::new(0xb42318),
        draw: Rgb::new(0x5c5950),
        warn: Rgb::new(0x8a4b00),
        error: Rgb::new(0xae2230),
        ok: Rgb::new(0x106c34),
        text_strong: Rgb::new(0x0a0a08),
        text_dim: Rgb::new(0x78766f),
        text_faint: Rgb::new(0x9b9890),
        division: Rgb::new(0x775800),
        abuser: Rgb::new(0xa33270),
        notice: Rgb::new(0x775800),
        chat_division: Rgb::new(0x775800),
        chat_team: Rgb::new(0x106c34),
        chat_other: Rgb::new(0x8a4b00),
        armor_pen: Rgb::new(0x106c34),
        armor_overpen: Rgb::new(0x785808),
        armor_ricochet: Rgb::new(0x1b5fa8),
        armor_shatter: Rgb::new(0x5f5c52),
        armor_angle_good: Rgb::new(0x116b34),
        armor_angle_mid: Rgb::new(0x785808),
        armor_angle_bad: Rgb::new(0xae2230),
    };

    pub const fn for_appearance(appearance: Appearance) -> &'static Self {
        match appearance {
            Appearance::Dark => &Self::DARK,
            Appearance::Light => &Self::LIGHT,
        }
    }

    /// Each tone with the custom property a style sheet reads it from.
    pub const fn properties(&self) -> [(&'static str, Rgb); 22] {
        [
            ("--wt-color-win", self.win),
            ("--wt-color-loss", self.loss),
            ("--wt-color-draw", self.draw),
            ("--wt-color-warn", self.warn),
            ("--wt-color-error", self.error),
            ("--wt-color-ok", self.ok),
            ("--wt-color-text-strong", self.text_strong),
            ("--wt-color-text-dim", self.text_dim),
            ("--wt-color-text-faint", self.text_faint),
            ("--wt-color-division", self.division),
            ("--wt-color-abuser", self.abuser),
            ("--wt-color-notice", self.notice),
            ("--wt-color-chat-division", self.chat_division),
            ("--wt-color-chat-team", self.chat_team),
            ("--wt-color-chat-other", self.chat_other),
            ("--wt-color-armor-pen", self.armor_pen),
            ("--wt-color-armor-overpen", self.armor_overpen),
            ("--wt-color-armor-ricochet", self.armor_ricochet),
            ("--wt-color-armor-shatter", self.armor_shatter),
            ("--wt-color-armor-angle-good", self.armor_angle_good),
            ("--wt-color-armor-angle-mid", self.armor_angle_mid),
            ("--wt-color-armor-angle-bad", self.armor_angle_bad),
        ]
    }

    fn rule(&self) -> String {
        let declarations: String =
            self.properties().iter().map(|(property, color)| format!("{property}: {color}; ")).collect();

        format!(":root {{ {declarations}}}")
    }
}

/// The landaire-ui choice the saved `choice` names.
pub const fn palette_choice(choice: ThemeChoice) -> landaire_ui::ThemeChoice {
    match choice {
        ThemeChoice::System => landaire_ui::ThemeChoice::System,
        ThemeChoice::Dark => landaire_ui::ThemeChoice::Fixed(Scheme::Dark),
        ThemeChoice::Light => landaire_ui::ThemeChoice::Fixed(Scheme::Light),
        ThemeChoice::Dracula => landaire_ui::ThemeChoice::Fixed(Scheme::Dracula),
        ThemeChoice::Nord => landaire_ui::ThemeChoice::Fixed(Scheme::Nord),
        ThemeChoice::Monokai => landaire_ui::ThemeChoice::Fixed(Scheme::Monokai),
        ThemeChoice::SolarizedDark => landaire_ui::ThemeChoice::Fixed(Scheme::SolarizedDark),
        ThemeChoice::SolarizedLight => landaire_ui::ThemeChoice::Fixed(Scheme::SolarizedLight),
        ThemeChoice::GruvboxDark => landaire_ui::ThemeChoice::Fixed(Scheme::GruvboxDark),
        ThemeChoice::GruvboxLight => landaire_ui::ThemeChoice::Fixed(Scheme::GruvboxLight),
        ThemeChoice::CatppuccinMocha => landaire_ui::ThemeChoice::Fixed(Scheme::CatppuccinMocha),
        ThemeChoice::TokyoNight => landaire_ui::ThemeChoice::Fixed(Scheme::TokyoNight),
    }
}

/// The `--wt-*` properties for `choice`. Following the desktop puts the dark set behind
/// `prefers-color-scheme`, as the landaire-ui tokens are.
pub fn semantic_sheet(choice: ThemeChoice) -> String {
    let choice = palette_choice(choice);
    match choice {
        landaire_ui::ThemeChoice::System => {
            let light = Semantic::LIGHT.rule();
            let dark = Semantic::DARK.rule();
            format!("{light}\n@media (prefers-color-scheme: dark) {{ {dark} }}")
        }
        landaire_ui::ThemeChoice::Fixed(scheme) => Semantic::for_appearance(scheme.palette().appearance).rule(),
    }
}

/// Keeps the `--wt-*` properties of the window it is in on `choice`.
#[component]
pub fn SemanticTokens(#[prop(into)] choice: Signal<ThemeChoice, LocalStorage>) -> impl IntoView {
    let sheet = Stylesheet::install(SHEET, &semantic_sheet(choice.get_untracked()));
    let following = RenderEffect::new(move |previous: Option<()>| {
        let css = semantic_sheet(choice.get());
        // The first run is the install above.
        if previous.is_some()
            && let Some(sheet) = sheet.as_ref()
        {
            sheet.replace(&css);
        }
    });
    on_cleanup_local(move || drop(following))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_scheme_takes_the_tones_of_its_appearance() {
        let css = semantic_sheet(ThemeChoice::GruvboxLight);
        assert!(css.contains(&format!("--wt-color-win: {}", Semantic::LIGHT.win)));
        assert!(!css.contains("@media"));
    }

    #[test]
    fn following_the_desktop_puts_the_dark_tones_behind_the_media_query() {
        let css = semantic_sheet(ThemeChoice::System);
        let query = css.find("@media (prefers-color-scheme: dark)").expect("the query is written");
        let dark_win = css.find(&format!("--wt-color-win: {}", Semantic::DARK.win)).expect("the dark set is written");
        assert!(dark_win > query);
    }
}
