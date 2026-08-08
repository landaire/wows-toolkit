use std::collections::HashMap;
use std::sync::Arc;

use egui::Color32;
use egui::RichText;
use serde::Serialize;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::VehicleEntity;
use wows_replays::types::AccountId;
use wows_replays::types::Relation;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::Species;

use crate::data::wows_data::BuildData;
use crate::data::wows_data::GameAsset;

// Build/loadout types and their extractor live in wows-replay-insights (egui-free).
pub use wows_replay_insights::battle_report::TranslatedBuild;
pub use wows_replay_insights::battle_report::TranslatedModule;

/// Returns the ship class icon for a given species.
pub fn ship_class_icon_from_species(species: Species, wows_data: &BuildData) -> Option<Arc<GameAsset>> {
    wows_data.assets.ship_icons.get(&species).cloned()
}

/// What a player is, for colouring. Resolved to a colour at draw time so the
/// scoreboard follows the active theme without rebuilding the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerTint {
    SelfPlayer,
    Ally,
    Enemy,
    DivisionMate,
    Abuser,
}

impl PlayerTint {
    /// Classifies by relation alone (self/ally/enemy). Division-mate and
    /// abuser overrides are layered on top by the caller, since those are
    /// stronger classifications than plain relation.
    pub fn from_relation(relation: Relation) -> Self {
        if relation.is_self() {
            Self::SelfPlayer
        } else if relation.is_ally() {
            Self::Ally
        } else {
            Self::Enemy
        }
    }

    pub fn color(self, visuals: &egui::Visuals) -> Color32 {
        let sem = crate::ui::theme::semantic::semantic(visuals);
        match self {
            Self::SelfPlayer => sem.text_strong,
            Self::Ally => sem.win,
            Self::Enemy => sem.loss,
            Self::DivisionMate => sem.division,
            Self::Abuser => sem.abuser,
        }
    }

    /// Applies the abuser override on top of this (row) tint: abuser beats
    /// everything else, including DivisionMate. This is the role the
    /// player's name renders with; the row's other colours (stats, icon)
    /// use the tint as-is, without this override.
    pub fn with_abuser_override(self, is_abuser: bool) -> Self {
        if is_abuser { Self::Abuser } else { self }
    }
}

/// A clan tag's colour: the server-supplied clan colour, or, for replays
/// that omit it (pre-clan-color builds), the plain relation colour as a
/// fallback so the tag still renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClanColor {
    Fixed(Color32),
    Relation(PlayerTint),
}

impl ClanColor {
    pub fn color(self, visuals: &egui::Visuals) -> Color32 {
        match self {
            // Server-supplied, arbitrary RGB, no legibility guarantee.
            Self::Fixed(color) => crate::ui::theme::contrast::readable_on(color, visuals.panel_fill),
            Self::Relation(tint) => tint.color(visuals),
        }
    }
}

/// Information about a player's skill build.
#[derive(Clone, Serialize)]
pub struct SkillInfo {
    pub skill_points: usize,
    pub num_skills: usize,
    pub highest_tier: usize,
    pub num_tier_1_skills: usize,
    #[serde(skip)]
    pub hover_text: Option<String>,
    #[serde(skip)]
    pub label_text: String,
    /// How the point count reads at a glance; resolved to a colour at draw
    /// time. See `crate::util::formatting::SkillTier`.
    #[serde(skip)]
    pub tier: crate::util::formatting::SkillTier,
}

// Per-type damage/hit/potential breakdowns are field-identical to the insights
// numeric model; re-exported so the export and UI share one definition.
pub use wows_replay_insights::battle_report::Damage;
pub use wows_replay_insights::battle_report::Hits;
pub use wows_replay_insights::battle_report::PotentialDamage;

/// Damage interaction between two players.
#[derive(Debug, Default)]
pub struct DamageInteraction {
    pub damage_dealt: u64,
    pub damage_dealt_text: String,
    /// Per-type breakdown (e.g. "AP: 5,000\nDepth Charge (Direct): 3,000")
    pub damage_dealt_hover_text: String,
    /// % of this player's total dealt damage
    pub damage_dealt_percentage: f64,
    pub damage_dealt_percentage_text: String,
    /// % of the victim's total received damage
    pub damage_dealt_inverse_percentage: f64,
    pub damage_dealt_inverse_percentage_text: String,
    pub damage_received: u64,
    pub damage_received_text: String,
    /// Per-type breakdown of received damage
    pub damage_received_hover_text: String,
    /// % of this player's total received damage
    pub damage_received_percentage: f64,
    pub damage_received_percentage_text: String,
    /// % of the attacker's total dealt damage
    pub damage_received_inverse_percentage: f64,
    pub damage_received_inverse_percentage_text: String,
}

impl DamageInteraction {
    pub fn damage_dealt(&self) -> u64 {
        self.damage_dealt
    }

    pub fn damage_dealt_percentage(&self) -> f64 {
        self.damage_dealt_percentage
    }

    pub fn damage_received(&self) -> u64 {
        self.damage_received
    }

    pub fn damage_received_percentage(&self) -> f64 {
        self.damage_received_percentage
    }
}

/// An achievement earned in battle.
#[derive(Clone)]
pub struct Achievement {
    pub game_param: Arc<Param>,
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub count: usize,
}

/// A ribbon earned in battle.
#[derive(Clone)]
pub struct Ribbon {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub is_subribbon: bool,
    pub count: u64,
}

/// One consumable slot equipped on a player's ship, with charge usage
/// resolved from the battle controller's activation log.
#[derive(Clone)]
pub struct PlayerConsumable {
    pub display_name: String,
    pub description: String,
    pub icon_key: String,
    pub charges_used: u32,
    pub total_charges: wowsunpack::game_types::ChargeCount,
}

/// Report for a single player in a battle.
pub struct PlayerReport {
    pub player: Arc<Player>,
    /// Row colour: relation, with a division-mate override. Does not reflect
    /// abuser status; the name specifically applies that on top (see
    /// [`Self::name_text`]).
    pub tint: PlayerTint,
    pub is_abuser: bool,
    pub name_text: String,
    pub clan_tag: Option<String>,
    pub clan_color: Option<ClanColor>,
    pub ship_species_text: String,
    pub icon: Option<Arc<GameAsset>>,
    pub division_label: Option<String>,
    pub base_xp: Option<i64>,
    pub base_xp_text: Option<String>,
    pub raw_xp: Option<i64>,
    pub raw_xp_text: Option<String>,
    pub observed_damage: u64,
    pub observed_damage_text: String,
    pub actual_damage: Option<u64>,
    pub actual_damage_report: Option<Damage>,
    pub actual_damage_text: Option<String>,
    /// RichText to support monospace font
    pub actual_damage_hover_text: Option<RichText>,
    pub hits: Option<u64>,
    pub hits_report: Option<Hits>,
    pub hits_text: Option<String>,
    /// RichText to support monospace font
    pub hits_hover_text: Option<RichText>,
    pub ship_name: String,
    pub spotting_damage: Option<u64>,
    pub spotting_damage_text: Option<String>,
    pub spotting_damage_hover_text: Option<RichText>,
    pub potential_damage: Option<u64>,
    pub potential_damage_text: Option<String>,
    pub potential_damage_hover_text: Option<RichText>,
    pub potential_damage_report: Option<PotentialDamage>,
    pub time_lived_secs: Option<u64>,
    pub time_lived_text: Option<String>,
    pub skill_info: SkillInfo,
    pub received_damage: Option<u64>,
    pub received_damage_text: Option<String>,
    pub received_damage_hover_text: Option<RichText>,
    pub received_damage_report: Option<Damage>,
    pub damage_interactions: Option<HashMap<AccountId, DamageInteraction>>,
    pub fires: Option<u64>,
    pub floods: Option<u64>,
    pub citadels: Option<u64>,
    pub crits: Option<u64>,
    pub distance_traveled: Option<f64>,
    pub is_test_ship: bool,
    pub relation: Relation,
    pub manual_stat_hide_toggle: bool,
    // TODO: Maybe in the future refactor this to be a HashMap<Rc<Player>, DeathInfo> ?
    pub kills: Option<i64>,
    pub observed_kills: i64,
    pub translated_build: Option<TranslatedBuild>,
    pub achievements: Vec<Achievement>,
    pub ribbons: HashMap<String, Ribbon>,
    pub consumables: Vec<PlayerConsumable>,
    /// Number of Repair Party (`RepairParty`) activations observed for this
    /// player. `None` when the ship doesn't carry a Repair Party slot. This
    /// count only covers consumable activations seen in the parsed packets,
    /// so it may be incomplete for ships outside the recording perspective(s).
    pub heal_count: Option<u32>,
    pub personal_rating: Option<crate::util::personal_rating::PersonalRatingResult>,
    pub has_vehicle_entity: bool,
    /// Effective fire chance for the recording player. `None` for every other
    /// player: attribution relies on the self-player SetFire ribbon, which the
    /// server only sends for the recording perspective. Also `None` when the
    /// analysis could not resolve at all (missing build data, an unresolved
    /// secondary battery, no hull with fire-section geometry). A result with no
    /// eligible hits is still `Some`, rendered as an unknown rate.
    pub fire_chance: Option<wows_replay_insights::fire_chance::analysis::EffectiveFireChance>,
}

#[allow(dead_code)]
impl PlayerReport {
    pub fn remove_nda_info(&mut self) {
        self.observed_damage = 0;
        self.observed_damage_text = "NDA".to_string();
        self.actual_damage = Some(0);
        self.actual_damage_text = Some("NDA".into());
        self.actual_damage_hover_text = None;
        self.spotting_damage_hover_text = None;
        self.potential_damage = Some(0);
        self.potential_damage_text = Some("NDA".into());
        self.potential_damage_hover_text = None;
        self.received_damage = Some(0);
        self.received_damage_text = Some("NDA".into());
        self.received_damage_hover_text = None;
        self.fires = Some(0);
        self.floods = Some(0);
        self.citadels = Some(0);
        self.crits = Some(0);
        self.fire_chance = None;
    }

    pub fn player(&self) -> &Player {
        &self.player
    }

    pub fn vehicle(&self) -> Option<&VehicleEntity> {
        self.player.vehicle_entity()
    }

    pub fn tint(&self) -> PlayerTint {
        self.tint
    }

    /// The row tint, with the abuser override layered on top if applicable.
    /// This is the role the player's name renders with.
    pub fn name_tint(&self) -> PlayerTint {
        self.tint.with_abuser_override(self.is_abuser)
    }

    pub fn name_text(&self, visuals: &egui::Visuals) -> RichText {
        RichText::new(&self.name_text).color(self.name_tint().color(visuals))
    }

    pub fn clan_text(&self, visuals: &egui::Visuals) -> Option<RichText> {
        let tag = self.clan_tag.as_ref()?;
        // clan_color is always set alongside clan_tag; the fallback here is a
        // safety net for the draw loop, not an expected path.
        let color = self.clan_color.unwrap_or(ClanColor::Relation(self.tint)).color(visuals);
        Some(RichText::new(format!("[{tag}]")).color(color))
    }

    pub fn ship_species_text(&self) -> &str {
        &self.ship_species_text
    }

    pub fn icon(&self) -> Option<Arc<GameAsset>> {
        self.icon.clone()
    }

    pub fn division_label(&self) -> Option<&String> {
        self.division_label.as_ref()
    }

    pub fn base_xp(&self) -> Option<i64> {
        self.base_xp
    }

    pub fn base_xp_text(&self) -> Option<&String> {
        self.base_xp_text.as_ref()
    }

    pub fn raw_xp(&self) -> Option<i64> {
        self.raw_xp
    }

    pub fn raw_xp_text(&self) -> Option<&String> {
        self.raw_xp_text.as_ref()
    }

    pub fn observed_damage(&self) -> u64 {
        self.observed_damage
    }

    pub fn observed_damage_text(&self) -> &str {
        &self.observed_damage_text
    }

    pub fn actual_damage(&self) -> Option<u64> {
        self.actual_damage
    }

    pub fn actual_damage_report(&self) -> Option<&Damage> {
        self.actual_damage_report.as_ref()
    }

    pub fn actual_damage_text(&self) -> Option<&String> {
        self.actual_damage_text.as_ref()
    }

    pub fn actual_damage_hover_text(&self) -> Option<&RichText> {
        self.actual_damage_hover_text.as_ref()
    }

    pub fn ship_name(&self) -> &str {
        &self.ship_name
    }

    pub fn spotting_damage(&self) -> Option<u64> {
        self.spotting_damage
    }

    pub fn spotting_damage_text(&self) -> Option<&String> {
        self.spotting_damage_text.as_ref()
    }

    pub fn spotting_damage_hover_text(&self) -> Option<&RichText> {
        self.spotting_damage_hover_text.as_ref()
    }

    pub fn potential_damage(&self) -> Option<u64> {
        self.potential_damage
    }

    pub fn potential_damage_text(&self) -> Option<&String> {
        self.potential_damage_text.as_ref()
    }

    pub fn potential_damage_hover_text(&self) -> Option<&RichText> {
        self.potential_damage_hover_text.as_ref()
    }

    pub fn potential_damage_report(&self) -> Option<&PotentialDamage> {
        self.potential_damage_report.as_ref()
    }

    pub fn time_lived_secs(&self) -> Option<u64> {
        self.time_lived_secs
    }

    pub fn time_lived_text(&self) -> Option<&String> {
        self.time_lived_text.as_ref()
    }

    pub fn skill_info(&self) -> &SkillInfo {
        &self.skill_info
    }

    pub fn received_damage(&self) -> Option<u64> {
        self.received_damage
    }

    pub fn received_damage_text(&self) -> Option<&String> {
        self.received_damage_text.as_ref()
    }

    pub fn received_damage_hover_text(&self) -> Option<&RichText> {
        self.received_damage_hover_text.as_ref()
    }

    pub fn received_damage_report(&self) -> Option<&Damage> {
        self.received_damage_report.as_ref()
    }

    pub fn fires(&self) -> Option<u64> {
        self.fires
    }

    pub fn floods(&self) -> Option<u64> {
        self.floods
    }

    pub fn citadels(&self) -> Option<u64> {
        self.citadels
    }

    pub fn crits(&self) -> Option<u64> {
        self.crits
    }

    pub fn distance_traveled(&self) -> Option<f64> {
        self.distance_traveled
    }

    pub fn is_test_ship(&self) -> bool {
        self.is_test_ship
    }

    pub fn observed_kills(&self) -> i64 {
        self.observed_kills
    }

    pub fn kills(&self) -> Option<i64> {
        self.kills
    }

    pub fn translated_build(&self) -> Option<&TranslatedBuild> {
        self.translated_build.as_ref()
    }

    pub fn should_hide_stats(&self) -> bool {
        self.manual_stat_hide_toggle || (!self.relation.is_self() && self.is_test_ship)
    }

    pub fn hits_report(&self) -> Option<&Hits> {
        self.hits_report.as_ref()
    }

    pub fn damage_interactions(&self) -> Option<&HashMap<AccountId, DamageInteraction>> {
        self.damage_interactions.as_ref()
    }

    pub fn personal_rating(&self) -> Option<&crate::util::personal_rating::PersonalRatingResult> {
        self.personal_rating.as_ref()
    }

    pub fn relation(&self) -> Relation {
        self.relation
    }
}

#[cfg(test)]
mod tests {
    use egui::Visuals;

    use super::*;

    #[test]
    fn from_relation_maps_self_ally_enemy() {
        assert_eq!(PlayerTint::from_relation(Relation::new(0)), PlayerTint::SelfPlayer);
        assert_eq!(PlayerTint::from_relation(Relation::new(1)), PlayerTint::Ally);
        assert_eq!(PlayerTint::from_relation(Relation::new(2)), PlayerTint::Enemy);
    }

    /// The subtle case: abuser overrides the name even when the row tint is
    /// DivisionMate, the next-strongest classification below abuser.
    #[test]
    fn name_role_is_abuser_even_over_division_mate() {
        assert_eq!(PlayerTint::DivisionMate.with_abuser_override(true), PlayerTint::Abuser);
    }

    #[test]
    fn name_role_falls_back_to_row_tint_when_not_abuser() {
        assert_eq!(PlayerTint::DivisionMate.with_abuser_override(false), PlayerTint::DivisionMate);
    }

    #[test]
    fn row_tint_never_becomes_abuser_on_its_own() {
        // Abuser only ever appears via with_abuser_override (the name role);
        // the row tint (from_relation, plus a DivisionMate override applied
        // by the caller) never produces Abuser directly.
        for relation in [Relation::new(0), Relation::new(1), Relation::new(2)] {
            assert_ne!(PlayerTint::from_relation(relation), PlayerTint::Abuser);
        }
        assert_ne!(PlayerTint::DivisionMate, PlayerTint::Abuser);
    }

    #[test]
    fn color_resolves_per_theme_not_a_constant() {
        let dark = PlayerTint::SelfPlayer.color(&Visuals::dark());
        let light = PlayerTint::SelfPlayer.color(&Visuals::light());
        assert_ne!(dark, light, "SelfPlayer tint must resolve differently between themes");
    }

    /// A deliberately awful server-supplied clan colour (worst case: no
    /// contrast at all against the panel) must still clear the floor once
    /// resolved, in both themes.
    #[test]
    fn fixed_clan_color_is_repaired_against_the_panel_in_both_themes() {
        use crate::ui::theme::contrast::CONTRAST_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;

        let dark_visuals = Visuals::dark();
        let black = ClanColor::Fixed(Color32::BLACK).color(&dark_visuals); // theme-exempt: worst-case test input
        let r = contrast_ratio(black, dark_visuals.panel_fill);
        assert!(r >= CONTRAST_FLOOR, "black clan colour on dark panel only reached {r}");

        let light_visuals = Visuals::light();
        let white = ClanColor::Fixed(Color32::WHITE).color(&light_visuals); // theme-exempt: worst-case test input
        let r = contrast_ratio(white, light_visuals.panel_fill);
        assert!(r >= CONTRAST_FLOOR, "white clan colour on light panel only reached {r}");
    }
}
