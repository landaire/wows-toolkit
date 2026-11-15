//! The globe that marks a realm beside its badge, facing the realm's part of the world.

use rust_i18n::t;
use wgc_auth::AccountRealm;
use wows_toolkit_icons::Glyph;
use zgui::component;
use zgui::view;
use zgui::view::Attrs;
use zgui::view::IntoView;

use crate::icons::AppIconProps;

/// The globe facing `realm`.
pub fn realm_glyph(realm: AccountRealm) -> Glyph {
    match realm {
        AccountRealm::Eu => Glyph::EarthEurope,
        AccountRealm::Na => Glyph::EarthAmericas,
        AccountRealm::Asia => Glyph::EarthAsia,
    }
}

/// The realm's full name, which a screen reader says for its globe.
pub fn realm_name(realm: AccountRealm) -> String {
    match realm {
        AccountRealm::Eu => t!("ui.launcher.realm.eu"),
        AccountRealm::Na => t!("ui.launcher.realm.na"),
        AccountRealm::Asia => t!("ui.launcher.realm.asia"),
    }
    .into_owned()
}

/// The globe for `realm`, named with the realm's full name.
#[component]
pub fn RealmGlyph(realm: AccountRealm, #[prop(attrs)] attrs: Attrs) -> impl IntoView {
    view! {
        AppIcon(glyph = realm_glyph(realm), label = realm_name(realm), {..attrs})
    }
}
