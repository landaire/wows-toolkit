//! The theme and language last applied in any window, which the app's other
//! windows follow: each window's engine draws in the palette it was told, so
//! a change made in one is carried to the rest.

use std::sync::LazyLock;

use nsh_core::prelude::JsObject;
use tokio::sync::watch;

use crate::js_newtype::js_newtype;

/// Counts changes to the shared theme and language.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LookRevision(u32);

js_newtype!(LookRevision, u32);

/// The theme and language last applied, `None` for one no window has applied.
#[derive(Debug, Clone, Default, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct SharedLook {
    pub revision: LookRevision,
    /// The stored palette name (`System`, `Dracula`, ...).
    pub theme: Option<String>,
    pub locale: Option<String>,
}

static LOOK: LazyLock<watch::Sender<SharedLook>> = LazyLock::new(|| watch::Sender::new(SharedLook::default()));

/// Records the palette a window applied. The same one again is no change, so
/// a window following the others does not wake them.
pub fn publish_theme(name: &str) {
    LOOK.send_if_modified(|look| {
        if look.theme.as_deref() == Some(name) {
            return false;
        }

        look.theme = Some(name.to_owned());
        look.revision = LookRevision(look.revision.0.wrapping_add(1));
        true
    });
}

/// Records the language a window applied, as [`publish_theme`] does.
pub fn publish_locale(code: &str) {
    LOOK.send_if_modified(|look| {
        if look.locale.as_deref() == Some(code) {
            return false;
        }

        look.locale = Some(code.to_owned());
        look.revision = LookRevision(look.revision.0.wrapping_add(1));
        true
    });
}

/// The shared theme and language once they differ from `after`.
pub async fn next(after: LookRevision) -> SharedLook {
    let mut looks = LOOK.subscribe();
    // The sender lives for the process, so the wait only ends on a change.
    let _ = looks.wait_for(|look| look.revision != after).await;

    looks.borrow().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_change_is_counted() {
        let start = LOOK.borrow().revision;

        publish_theme("Dracula");
        let changed = LOOK.borrow().revision;
        publish_theme("Dracula");

        assert_ne!(changed, start);
        assert_eq!(LOOK.borrow().revision, changed);
        assert_eq!(LOOK.borrow().theme.as_deref(), Some("Dracula"));
    }
}
