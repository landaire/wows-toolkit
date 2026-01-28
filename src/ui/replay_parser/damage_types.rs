//! Damage and hit type constants and descriptions for replay parsing.

pub const DAMAGE_MAIN_AP: &str = "damage_main_ap";
pub const DAMAGE_MAIN_CS: &str = "damage_main_cs";
pub const DAMAGE_MAIN_HE: &str = "damage_main_he";
pub const DAMAGE_ATBA_AP: &str = "damage_atba_ap";
pub const DAMAGE_ATBA_CS: &str = "damage_atba_cs";
pub const DAMAGE_ATBA_HE: &str = "damage_atba_he";
pub const DAMAGE_TPD_NORMAL: &str = "damage_tpd_normal";
pub const DAMAGE_TPD_DEEP: &str = "damage_tpd_deep";
pub const DAMAGE_TPD_ALTER: &str = "damage_tpd_alter";
pub const DAMAGE_TPD_PHOTON: &str = "damage_tpd_photon";
pub const DAMAGE_BOMB: &str = "damage_bomb";
// pub const DAMAGE_BOMB_AVIA: &str = "damage_bomb_avia";
pub const DAMAGE_BOMB_ALT: &str = "damage_bomb_alt";
// pub const DAMAGE_BOMB_AIRSUPPORT: &str = "damage_bomb_airsupport";
pub const DAMAGE_DBOMB_AIRSUPPORT: &str = "damage_dbomb_airsupport";
pub const DAMAGE_TBOMB: &str = "damage_tbomb";
pub const DAMAGE_TBOMB_ALT: &str = "damage_tbomb_alt";
pub const DAMAGE_TBOMB_AIRSUPPORT: &str = "damage_tbomb_airsupport";
pub const DAMAGE_FIRE: &str = "damage_fire";
pub const DAMAGE_RAM: &str = "damage_ram";
pub const DAMAGE_FLOOD: &str = "damage_flood";
pub const DAMAGE_DBOMB_DIRECT: &str = "damage_dbomb_direct";
pub const DAMAGE_DBOMB_SPLASH: &str = "damage_dbomb_splash";
pub const DAMAGE_SEA_MINE: &str = "damage_sea_mine";
pub const DAMAGE_ROCKET: &str = "damage_rocket";
pub const DAMAGE_ROCKET_AIRSUPPORT: &str = "damage_rocket_airsupport";
pub const DAMAGE_SKIP: &str = "damage_skip";
pub const DAMAGE_SKIP_ALT: &str = "damage_skip_alt";
pub const DAMAGE_SKIP_AIRSUPPORT: &str = "damage_skip_airsupport";
pub const DAMAGE_WAVE: &str = "damage_wave";
pub const DAMAGE_CHARGE_LASER: &str = "damage_charge_laser";
pub const DAMAGE_PULSE_LASER: &str = "damage_pulse_laser";
pub const DAMAGE_AXIS_LASER: &str = "damage_axis_laser";
pub const DAMAGE_PHASER_LASER: &str = "damage_phaser_laser";

pub const HITS_MAIN_AP: &str = "hits_main_ap";
pub const HITS_MAIN_CS: &str = "hits_main_cs";
pub const HITS_MAIN_HE: &str = "hits_main_he";
pub const HITS_ATBA_AP: &str = "hits_atba_ap";
pub const HITS_ATBA_CS: &str = "hits_atba_cs";
pub const HITS_ATBA_HE: &str = "hits_atba_he";
pub const HITS_TPD_NORMAL: &str = "hits_tpd";
pub const HITS_BOMB: &str = "hits_bomb";
// pub const HITS_BOMB_AVIA: &str = "hits_bomb_avia";
pub const HITS_BOMB_ALT: &str = "hits_bomb_alt";
pub const HITS_BOMB_AIRSUPPORT: &str = "hits_bomb_airsupport";
pub const HITS_DBOMB_AIRSUPPORT: &str = "hits_dbomb_airsupport";
pub const HITS_TBOMB: &str = "hits_tbomb";
pub const HITS_TBOMB_ALT: &str = "hits_tbomb_alt";
pub const HITS_TBOMB_AIRSUPPORT: &str = "hits_tbomb_airsupport";
pub const HITS_RAM: &str = "hits_ram";
pub const HITS_DBOMB_DIRECT: &str = "hits_dbomb_direct";
pub const HITS_DBOMB_SPLASH: &str = "hits_dbomb_splash";
pub const HITS_SEA_MINE: &str = "hits_sea_mine";
pub const HITS_ROCKET: &str = "hits_rocket";
pub const HITS_ROCKET_AIRSUPPORT: &str = "hits_rocket_airsupport";
pub const HITS_SKIP: &str = "hits_skip";
pub const HITS_SKIP_ALT: &str = "hits_skip_alt";
pub const HITS_SKIP_AIRSUPPORT: &str = "hits_skip_airsupport";
pub const HITS_WAVE: &str = "hits_wave";
pub const HITS_CHARGE_LASER: &str = "hits_charge_laser";
pub const HITS_PULSE_LASER: &str = "hits_pulse_laser";
pub const HITS_AXIS_LASER: &str = "hits_axis_laser";
pub const HITS_PHASER_LASER: &str = "hits_phaser_laser";

pub static DAMAGE_DESCRIPTIONS: [(&str, &str); 32] = [
    (DAMAGE_MAIN_AP, "AP"),
    (DAMAGE_MAIN_CS, "SAP"),
    (DAMAGE_MAIN_HE, "HE"),
    (DAMAGE_ATBA_AP, "AP Sec"),
    (DAMAGE_ATBA_CS, "SAP Sec"),
    (DAMAGE_ATBA_HE, "HE Sec"),
    (DAMAGE_TPD_NORMAL, "Torps"),
    (DAMAGE_TPD_DEEP, "Deep Water Torps"),
    (DAMAGE_TPD_ALTER, "Alt Torps"),
    (DAMAGE_TPD_PHOTON, "Photon Torps"),
    (DAMAGE_BOMB, "HE Bomb"),
    // (DAMAGE_BOMB_AVIA, "Bomb"),
    (DAMAGE_BOMB_ALT, "Alt Bomb"),
    // (DAMAGE_BOMB_AIRSUPPORT, "Air Support Bomb"),
    (DAMAGE_DBOMB_AIRSUPPORT, "Air Support Depth Charge"),
    (DAMAGE_TBOMB, "Torpedo Bomber"),
    (DAMAGE_TBOMB_ALT, "Torpedo Bomber (Alt)"),
    (DAMAGE_TBOMB_AIRSUPPORT, "Torpedo Bomber Air Support"),
    (DAMAGE_FIRE, "Fire"),
    (DAMAGE_RAM, "Ram"),
    (DAMAGE_FLOOD, "Flood"),
    (DAMAGE_DBOMB_DIRECT, "Depth Charge (Direct)"),
    (DAMAGE_DBOMB_SPLASH, "Depth Charge (Splash)"),
    (DAMAGE_SEA_MINE, "Sea Mine"),
    (DAMAGE_ROCKET, "Rocket"),
    (DAMAGE_ROCKET_AIRSUPPORT, "Air Supp Rocket"),
    (DAMAGE_SKIP, "Skip Bomb"),
    (DAMAGE_SKIP_ALT, "Alt Skip Bomb"),
    (DAMAGE_SKIP_AIRSUPPORT, "Air Supp Skip Bomb"),
    (DAMAGE_WAVE, "Wave"),
    (DAMAGE_CHARGE_LASER, "Charge Laser"),
    (DAMAGE_PULSE_LASER, "Pulse Laser"),
    (DAMAGE_AXIS_LASER, "Axis Laser"),
    (DAMAGE_PHASER_LASER, "Phaser Laser"),
];

pub static HITS_DESCRIPTIONS: [(&str, &str); 28] = [
    (HITS_MAIN_AP, "AP"),
    (HITS_MAIN_CS, "SAP"),
    (HITS_MAIN_HE, "HE"),
    (HITS_ATBA_AP, "AP Sec"),
    (HITS_ATBA_CS, "SAP Sec"),
    (HITS_ATBA_HE, "HE Sec"),
    (HITS_TPD_NORMAL, "Torps"),
    (HITS_BOMB, "HE Bomb"),
    // (HITS_BOMB_AVIA, "Bomb"),
    (HITS_BOMB_ALT, "Alt Bomb"),
    (HITS_BOMB_AIRSUPPORT, "Air Support Bomb"),
    (HITS_DBOMB_AIRSUPPORT, "Air Support Depth Charge"),
    (HITS_TBOMB, "Torpedo Bomber"),
    (HITS_TBOMB_ALT, "Torpedo Bomber (Alt)"),
    (HITS_TBOMB_AIRSUPPORT, "Torpedo Bomber Air Support"),
    (HITS_RAM, "Ram"),
    (HITS_DBOMB_DIRECT, "Depth Charge (Direct)"),
    (HITS_DBOMB_SPLASH, "Depth Charge (Splash)"),
    (HITS_SEA_MINE, "Sea Mine"),
    (HITS_ROCKET, "Rocket"),
    (HITS_ROCKET_AIRSUPPORT, "Air Supp Rocket"),
    (HITS_SKIP, "Skip Bomb"),
    (HITS_SKIP_ALT, "Alt Skip Bomb"),
    (HITS_SKIP_AIRSUPPORT, "Air Supp Skip Bomb"),
    (HITS_WAVE, "Wave"),
    (HITS_CHARGE_LASER, "Charge Laser"),
    (HITS_PULSE_LASER, "Pulse Laser"),
    (HITS_AXIS_LASER, "Axis Laser"),
    (HITS_PHASER_LASER, "Phaser Laser"),
];

pub static POTENTIAL_DAMAGE_DESCRIPTIONS: [(&str, &str); 4] =
    [("agro_art", "Artillery"), ("agro_tpd", "Torpedo"), ("agro_air", "Planes"), ("agro_dbomb", "Depth Charge")];
