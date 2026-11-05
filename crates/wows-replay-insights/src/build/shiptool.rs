//! Build URLs for <https://shiptool.st/link>.

use std::fmt::Display;
use std::fmt::Write as _;

use super::ResolvedBuild;

const SHIPTOOL_BASE: &str = "https://shiptool.st/link";
const UTM_SOURCE: &str = "wows_toolkit";

/// Builds a ShipTool link for a resolved ship loadout.
pub fn build_url(build: &ResolvedBuild) -> String {
    let mut url = String::with_capacity(192);
    write!(url, "{SHIPTOOL_BASE}?ship={}", build.ship.index()).expect("writing to a String cannot fail");

    append_list(&mut url, "upgrades", build.upgrades.iter().map(|param| param.index()));
    if let Some(captain) = build.captain.as_ref() {
        write!(url, "&commander={}", captain.index()).expect("writing to a String cannot fail");
    }
    append_list(&mut url, "skills", build.skills.iter());
    append_list(&mut url, "signals", build.signals.iter().map(|param| param.index()));
    write!(url, "&utm_source={UTM_SOURCE}").expect("writing to a String cannot fail");

    url
}

fn append_list<T>(url: &mut String, name: &str, values: impl IntoIterator<Item = T>)
where
    T: Display,
{
    let mut values = values.into_iter().peekable();
    if values.peek().is_none() {
        return;
    }

    write!(url, "&{name}=").expect("writing to a String cannot fail");
    for (index, value) in values.enumerate() {
        if index > 0 {
            url.push(',');
        }
        write!(url, "{value}").expect("writing to a String cannot fail");
    }
}

#[cfg(test)]
mod tests {
    use wowsunpack::game_params::types::Achievement;
    use wowsunpack::game_params::types::GameParams;
    use wowsunpack::game_params::types::Param;
    use wowsunpack::game_params::types::ParamData;
    use wowsunpack::game_params::types::Species;
    use wowsunpack::game_types::GameParamId;

    use super::*;

    fn param(id: u32, index: &str) -> Param {
        Param::builder()
            .id(GameParamId::from(id))
            .index(index.to_owned())
            .name(index.to_owned())
            .nation("USSR".to_owned())
            .data(ParamData::Achievement(
                Achievement::builder()
                    .is_group(false)
                    .one_per_battle(false)
                    .ui_type(String::new())
                    .ui_name(String::new())
                    .build(),
            ))
            .build()
    }

    #[test]
    fn build_url_carries_the_supported_loadout_fields_and_referrer() {
        let params = GameParams::from(vec![
            param(1, "PRSC310"),
            param(2, "PCM030"),
            param(3, "PCM042"),
            param(4, "PRW406"),
            param(5, "PCEF005"),
            param(6, "PCEF030"),
        ]);
        let build = ResolvedBuild::from_ids(
            GameParamId::from(1u32),
            &[],
            &[GameParamId::from(2u32), GameParamId::from(3u32)],
            Some(GameParamId::from(4u32)),
            &[3, 12, 17],
            &[GameParamId::from(5u32), GameParamId::from(6u32)],
            &[],
            Species::Cruiser,
            wowsunpack::data::Version::base(15, 7, 0),
            &params,
        )
        .expect("test build resolves");

        assert_eq!(
            build_url(&build),
            "https://shiptool.st/link?ship=PRSC310&upgrades=PCM030,PCM042&commander=PRW406&skills=3,12,17&signals=PCEF005,PCEF030&utm_source=wows_toolkit"
        );
    }
}
