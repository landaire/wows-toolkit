//! Build URLs for <https://app.wowssb.com/ship>.
//!
//! Two URL forms are produced:
//!
//! * [`build_url`]: deflate + base64 encoded JSON. Larger but human-readable
//!   when decoded; used for sharing.
//! * [`build_short_url`]: semicolon-separated raw fields. Compact; used when
//!   URL length matters (chat, embeds).
//!
//! Both forms encode the same data and are accepted by wowssb interchangeably.

use std::io::Write;

use flate2::Compression;
use flate2::write::DeflateEncoder;
use serde_json::json;

use super::ResolvedBuild;

const WOWSSB_BASE: &str = "https://app.wowssb.com/ship";
const BUILD_VERSION: u32 = 2;

/// Long form: deflated, base64-encoded JSON payload.
///
/// `build_name` becomes the BuildName field shown in the wowssb UI.
/// `referrer` is appended as `&ref=` for affiliate tracking; pass `None` to
/// omit.
pub fn build_url(build: &ResolvedBuild, build_name: &str, referrer: Option<&str>) -> String {
    let ship_index = build.ship.index();
    let nation = build.ship.nation().replace('_', "");

    let modules: Vec<&str> = build.modules.iter().map(|p| p.index()).collect();
    let upgrades: Vec<&str> = build.upgrades.iter().map(|p| p.index()).collect();
    let consumables: Vec<&str> = build.slots.iter().map(|s| s.ability.index()).collect();
    let signals: Vec<&str> = build.signals.iter().map(|p| p.index()).collect();

    let payload = json!({
        "BuildName": build_name,
        "ShipIndex": ship_index,
        "Nation": nation,
        "Modules": modules,
        "Upgrades": upgrades,
        "Captain": build.captain_index(),
        "Skills": build.skills,
        "Consumables": consumables,
        "Signals": signals,
        "BuildVersion": BUILD_VERSION,
    });

    let json_blob = serde_json::to_string(&payload).expect("serialize ship config");
    let mut deflated = Vec::new();
    {
        let mut encoder = DeflateEncoder::new(&mut deflated, Compression::best());
        encoder.write_all(json_blob.as_bytes()).expect("deflate ship config");
    }
    let encoded = data_encoding::BASE64.encode(&deflated).replace('/', "%2F").replace('+', "%2B");

    format_url(ship_index, &encoded, referrer)
}

/// Short form: semicolon-separated raw fields. Avoids the JSON+deflate
/// overhead at the cost of less self-description.
pub fn build_short_url(build: &ResolvedBuild, build_name: &str, referrer: Option<&str>) -> String {
    let ship_index = build.ship.index();

    let modules = join_indices(&build.modules);
    let upgrades = join_indices(&build.upgrades);
    let consumables: String = build.slots.iter().map(|s| s.ability.index()).collect::<Vec<_>>().join(",");
    let signals = join_indices(&build.signals);
    let skills: String = build.skills.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");

    let parts = [
        ship_index.to_owned(),
        modules,
        upgrades,
        build.captain_index().to_owned(),
        skills,
        consumables,
        signals,
        BUILD_VERSION.to_string(),
        build_name.to_owned(),
    ];

    format_url(ship_index, &parts.join(";"), referrer)
}

fn join_indices(params: &[wowsunpack::Rc<wowsunpack::game_params::types::Param>]) -> String {
    params.iter().map(|p| p.index()).collect::<Vec<_>>().join(",")
}

fn format_url(ship_index: &str, build_data: &str, referrer: Option<&str>) -> String {
    match referrer {
        Some(r) => format!("{WOWSSB_BASE}?shipIndexes={ship_index}&build={build_data}&ref={r}"),
        None => format!("{WOWSSB_BASE}?shipIndexes={ship_index}&build={build_data}"),
    }
}
