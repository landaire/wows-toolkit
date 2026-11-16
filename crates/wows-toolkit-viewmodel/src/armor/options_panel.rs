//! The armor viewer's options panel as the reader leaves it: the tab it shows and which of its
//! sections are open. Persisted under `keys::ARMOR_OPTIONS_LAYOUT`.

use serde::Deserialize;
use serde::Serialize;

/// A tab of the options panel. [`OptionsTab::ALL`] is the strip's order, top to bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionsTab {
    /// The hull, modules, camo and LOD the ship is drawn with, its textures and the export.
    Ship,
    /// What is drawn: armor zones and plates, hull parts and splash boxes.
    Layers,
    /// Casting shells at the hull: trajectories, the penetration check and splash.
    #[default]
    Analysis,
    /// How it is drawn: the model settings, the camera and the lighting.
    Display,
}

impl OptionsTab {
    pub const ALL: [Self; 4] = [Self::Ship, Self::Layers, Self::Analysis, Self::Display];

    /// The tab after this one on the strip, wrapping to the first.
    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    /// The tab before this one on the strip, wrapping to the last.
    pub fn previous(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn index(self) -> usize {
        match self {
            Self::Ship => 0,
            Self::Layers => 1,
            Self::Analysis => 2,
            Self::Display => 3,
        }
    }

    /// The test id fragment of the tab.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Ship => "Ship",
            Self::Layers => "Layers",
            Self::Analysis => "Analysis",
            Self::Display => "Display",
        }
    }

    /// The tab whose [`OptionsTab::id`] is `id`.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.id() == id)
    }

    /// The tab a search shows: the chosen one while it holds a match or nothing matched anywhere,
    /// otherwise the first tab on the strip that holds one. `matches` is `None` while nothing is
    /// searched for.
    pub fn shown(self, matches: Option<&TabMatches>) -> Self {
        let Some(matches) = matches else { return self };
        if matches.count(self) > 0 {
            return self;
        }

        Self::ALL.into_iter().find(|tab| matches.count(*tab) > 0).unwrap_or(self)
    }

    /// The tab Ctrl+Tab moves to from the one `matches` leaves shown: the next or previous on the
    /// strip that holds a match, wrapping. The shown tab while no other tab holds one.
    pub fn cycled(self, forward: bool, matches: Option<&TabMatches>) -> Self {
        let shown = self.shown(matches);
        let step = |tab: Self| if forward { tab.next() } else { tab.previous() };

        std::iter::successors(Some(step(shown)), |tab| Some(step(*tab)))
            .take(Self::ALL.len() - 1)
            .find(|tab| matches.is_none_or(|matches| matches.count(*tab) > 0))
            .unwrap_or(shown)
    }
}

/// How many controls a search kept in each tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabMatches([usize; OptionsTab::ALL.len()]);

impl TabMatches {
    pub fn count(&self, tab: OptionsTab) -> usize {
        self.0[tab.index()]
    }

    pub fn add(&mut self, tab: OptionsTab, count: usize) {
        self.0[tab.index()] += count;
    }

    pub fn total(&self) -> usize {
        self.0.iter().sum()
    }
}

/// A folding section of the options panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptionSection {
    /// The hull upgrade, module alternatives, camo and LOD.
    Configuration,
    Textures,
    Visibility,
    HullParts,
    Trajectory,
    PenCheck,
    Display,
    CameraRings,
    Lighting,
}

impl OptionSection {
    /// The tab the section is shown in.
    pub const fn tab(self) -> OptionsTab {
        match self {
            Self::Configuration | Self::Textures => OptionsTab::Ship,
            Self::Visibility | Self::HullParts => OptionsTab::Layers,
            Self::Trajectory | Self::PenCheck => OptionsTab::Analysis,
            Self::Display | Self::CameraRings | Self::Lighting => OptionsTab::Display,
        }
    }

    /// The test id fragment of the section.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Configuration => "Configuration",
            Self::Textures => "Textures",
            Self::Visibility => "Visibility",
            Self::HullParts => "HullParts",
            Self::Trajectory => "Trajectory",
            Self::PenCheck => "PenCheck",
            Self::Display => "Display",
            Self::CameraRings => "CameraRings",
            Self::Lighting => "Lighting",
        }
    }
}

/// Which sections of the options panel are expanded. A section missing from a saved layout takes
/// its default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptionSectionsOpen {
    pub configuration: bool,
    pub textures: bool,
    pub visibility: bool,
    pub hull_parts: bool,
    pub trajectory: bool,
    pub pen_check: bool,
    pub display: bool,
    pub camera_rings: bool,
    pub lighting: bool,
}

impl Default for OptionSectionsOpen {
    /// The first section of each tab, which is the one read most there. The rest start closed.
    fn default() -> Self {
        Self {
            configuration: true,
            textures: false,
            visibility: true,
            hull_parts: false,
            trajectory: true,
            pen_check: false,
            display: true,
            camera_rings: false,
            lighting: false,
        }
    }
}

impl OptionSectionsOpen {
    /// Every section closed.
    pub fn closed() -> Self {
        Self {
            configuration: false,
            textures: false,
            visibility: false,
            hull_parts: false,
            trajectory: false,
            pen_check: false,
            display: false,
            camera_rings: false,
            lighting: false,
        }
    }

    pub fn is_open(self, section: OptionSection) -> bool {
        match section {
            OptionSection::Configuration => self.configuration,
            OptionSection::Textures => self.textures,
            OptionSection::Visibility => self.visibility,
            OptionSection::HullParts => self.hull_parts,
            OptionSection::Trajectory => self.trajectory,
            OptionSection::PenCheck => self.pen_check,
            OptionSection::Display => self.display,
            OptionSection::CameraRings => self.camera_rings,
            OptionSection::Lighting => self.lighting,
        }
    }

    fn open_mut(&mut self, section: OptionSection) -> &mut bool {
        match section {
            OptionSection::Configuration => &mut self.configuration,
            OptionSection::Textures => &mut self.textures,
            OptionSection::Visibility => &mut self.visibility,
            OptionSection::HullParts => &mut self.hull_parts,
            OptionSection::Trajectory => &mut self.trajectory,
            OptionSection::PenCheck => &mut self.pen_check,
            OptionSection::Display => &mut self.display,
            OptionSection::CameraRings => &mut self.camera_rings,
            OptionSection::Lighting => &mut self.lighting,
        }
    }

    pub fn open(&mut self, section: OptionSection) {
        *self.open_mut(section) = true;
    }

    pub fn toggle(&mut self, section: OptionSection) {
        let open = self.open_mut(section);
        *open = !*open;
    }
}

/// The options panel as the reader left it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OptionsLayout {
    pub tab: OptionsTab,
    pub sections: OptionSectionsOpen,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tabs_cycle_in_strip_order_and_wrap() {
        let forward: Vec<OptionsTab> =
            std::iter::successors(Some(OptionsTab::Ship), |tab| Some(tab.next())).take(5).collect();
        assert_eq!(
            forward,
            [OptionsTab::Ship, OptionsTab::Layers, OptionsTab::Analysis, OptionsTab::Display, OptionsTab::Ship]
        );
        assert_eq!(OptionsTab::Ship.previous(), OptionsTab::Display);
        assert_eq!(OptionsTab::Analysis.previous(), OptionsTab::Layers);
    }

    #[test]
    fn a_search_moves_off_a_tab_without_matches_to_the_first_with_one() {
        let mut matches = TabMatches::default();
        matches.add(OptionsTab::Display, 2);
        matches.add(OptionsTab::Layers, 1);

        assert_eq!(OptionsTab::Analysis.shown(Some(&matches)), OptionsTab::Layers);
        assert_eq!(OptionsTab::Display.shown(Some(&matches)), OptionsTab::Display, "a tab with a match stays");
        assert_eq!(OptionsTab::Analysis.shown(None), OptionsTab::Analysis, "nothing searched keeps the choice");
        assert_eq!(
            OptionsTab::Ship.shown(Some(&TabMatches::default())),
            OptionsTab::Ship,
            "with no match anywhere the choice stays"
        );
    }

    #[test]
    fn cycling_during_a_search_skips_the_tabs_without_a_match() {
        let mut matches = TabMatches::default();
        matches.add(OptionsTab::Ship, 1);
        matches.add(OptionsTab::Display, 3);

        assert_eq!(OptionsTab::Analysis.cycled(true, Some(&matches)), OptionsTab::Display, "from the tab shown");
        assert_eq!(OptionsTab::Display.cycled(true, Some(&matches)), OptionsTab::Ship, "wrapping past Layers");
        assert_eq!(OptionsTab::Ship.cycled(false, Some(&matches)), OptionsTab::Display);
        assert_eq!(OptionsTab::Layers.cycled(true, None), OptionsTab::Analysis, "nothing searched visits every tab");

        let mut one = TabMatches::default();
        one.add(OptionsTab::Layers, 2);
        assert_eq!(OptionsTab::Analysis.cycled(true, Some(&one)), OptionsTab::Layers, "a lone match stays shown");
    }

    #[test]
    fn a_saved_layout_reads_back_and_a_missing_section_takes_its_default() {
        let saved = r#"{"tab":"layers","sections":{"configuration":false,"lighting":true}}"#;

        let layout: OptionsLayout = serde_json::from_str(saved).expect("the layout parses");

        assert_eq!(layout.tab, OptionsTab::Layers);
        assert!(!layout.sections.configuration);
        assert!(layout.sections.lighting);
        assert!(layout.sections.trajectory, "a section the saved layout lacks takes its default");
        let written = serde_json::to_string(&layout).expect("the layout serializes");
        assert_eq!(serde_json::from_str::<OptionsLayout>(&written).expect("it reads back"), layout);
    }
}
