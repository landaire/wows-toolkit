//! The OS secret store through the `keyring` crate: the freedesktop Secret Service on Linux
//! and the BSDs, the Keychain on macOS, the Credential Manager on Windows.

use std::collections::HashMap;

use crate::error::AuthError;
use crate::store::KeyringEntry;
use crate::store::SecretStore;

const KEYRING_SERVICE: &str = "wows-toolkit-wgc";

/// The Secret Service `application` attribute, which keyring browsers such as Seahorse and
/// KeePassXC show beside the label.
const APPLICATION: &str = "wows-toolkit";

/// The OS secret store. On Linux a locked collection is unlocked through the Secret Service
/// `Prompt` object, which shows the provider's own dialog.
pub struct OsKeyring;

impl SecretStore for OsKeyring {
    fn get(&self, entry: &KeyringEntry) -> Result<Option<String>, AuthError> {
        match keyring_entry(entry)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(source) => Err(map_keyring_error(source)),
        }
    }

    fn set(&self, entry: &KeyringEntry, secret: &str) -> Result<(), AuthError> {
        let handle = keyring_entry(entry)?;
        handle.set_password(secret).map_err(map_keyring_error)?;

        // A new item already has the label; this relabels one an earlier release created.
        // Failing leaves only the old label, and the secret is saved either way.
        let _ = handle.update_attributes(&label_attributes(entry));

        Ok(())
    }

    fn delete(&self, entry: &KeyringEntry) -> Result<(), AuthError> {
        match keyring_entry(entry)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(source) => Err(map_keyring_error(source)),
        }
    }
}

/// Found by service and user name alone, as earlier releases stored it; the label and
/// `application` attribute only describe the item.
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
fn keyring_entry(entry: &KeyringEntry) -> Result<keyring::Entry, AuthError> {
    let mut credential = keyring::secret_service::SsCredential::new_with_target(None, KEYRING_SERVICE, &entry.user)
        .map_err(map_keyring_error)?;
    credential.label = entry.label.clone();
    credential.attributes.insert("application".to_string(), APPLICATION.to_string());

    Ok(keyring::Entry::new_with_credential(Box::new(credential)))
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
fn keyring_entry(entry: &KeyringEntry) -> Result<keyring::Entry, AuthError> {
    keyring::Entry::new(KEYRING_SERVICE, &entry.user).map_err(map_keyring_error)
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
fn label_attributes(entry: &KeyringEntry) -> HashMap<&str, &str> {
    HashMap::from([("label", entry.label.as_str()), ("application", APPLICATION)])
}

/// The Credential Manager shows an entry's comment; the Keychain ignores these.
#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
fn label_attributes(entry: &KeyringEntry) -> HashMap<&str, &str> {
    HashMap::from([("comment", entry.label.as_str())])
}

/// `NoStorageAccess` is a store that refused: a collection that stayed locked, a dismissed
/// unlock prompt, or a provider with no default collection. A `PlatformFailure` means no store
/// only when the platform proves it absent; any other failure may be transient and must not
/// move secrets into the plaintext file.
fn map_keyring_error(source: keyring::Error) -> AuthError {
    classify(source, session_bus_connects)
}

/// [`map_keyring_error`] with the session bus probe given, so tests need no bus.
fn classify(source: keyring::Error, session_bus_connects: impl FnOnce() -> bool) -> AuthError {
    match source {
        keyring::Error::PlatformFailure(cause) if proves_store_absent(cause.as_ref(), session_bus_connects) => {
            AuthError::NoSecretStore
        }
        keyring::Error::NoStorageAccess(_) => AuthError::KeyringLocked,
        other => AuthError::Keyring { source: other },
    }
}

/// No session bus to connect to, or no Secret Service provider owning or activatable under
/// its bus name.
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
fn proves_store_absent(
    cause: &(dyn std::error::Error + Send + Sync + 'static),
    session_bus_connects: impl FnOnce() -> bool,
) -> bool {
    const ABSENT: [&str; 3] = [
        "org.freedesktop.DBus.Error.ServiceUnknown",
        "org.freedesktop.DBus.Error.NameHasNoOwner",
        "org.freedesktop.DBus.Error.NoServer",
    ];
    // libdbus reports a missing session bus it cannot autolaunch as NotSupported, which a
    // provider rejecting the session encryption also returns. A failed fresh connection tells them
    // apart.
    const NOT_SUPPORTED: &str = "org.freedesktop.DBus.Error.NotSupported";

    match cause.downcast_ref::<dbus_secret_service::Error>() {
        Some(dbus_secret_service::Error::Unavailable) => true,
        Some(dbus_secret_service::Error::Dbus(err)) => match err.name() {
            Some(name) if ABSENT.contains(&name) => true,
            Some(NOT_SUPPORTED) => !session_bus_connects(),
            _ => false,
        },
        _ => false,
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
fn session_bus_connects() -> bool {
    dbus::blocking::Connection::new_session().is_ok()
}

/// The Keychain and the Credential Manager are part of the OS.
#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
fn proves_store_absent(
    _cause: &(dyn std::error::Error + Send + Sync + 'static),
    _session_bus_connects: impl FnOnce() -> bool,
) -> bool {
    false
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
fn session_bus_connects() -> bool {
    true
}

#[cfg(all(test, any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
mod tests {
    use super::*;

    fn platform_failure(name: &str) -> keyring::Error {
        let dbus = dbus::Error::new_custom(name, "test");
        keyring::Error::PlatformFailure(Box::new(dbus_secret_service::Error::Dbus(dbus)))
    }

    #[test]
    fn only_a_missing_bus_or_provider_means_no_secret_store() {
        let bus_up = || true;
        for name in [
            "org.freedesktop.DBus.Error.ServiceUnknown",
            "org.freedesktop.DBus.Error.NameHasNoOwner",
            "org.freedesktop.DBus.Error.NoServer",
        ] {
            assert!(matches!(classify(platform_failure(name), bus_up), AuthError::NoSecretStore), "{name}");
        }

        for name in [
            "org.freedesktop.DBus.Error.NoReply",
            "org.freedesktop.DBus.Error.Timeout",
            "org.freedesktop.DBus.Error.Spawn.ChildExited",
            "org.freedesktop.DBus.Error.Failed",
            "org.freedesktop.DBus.Error.NotSupported",
        ] {
            assert!(matches!(classify(platform_failure(name), bus_up), AuthError::Keyring { .. }), "{name}");
        }

        let parse = keyring::Error::PlatformFailure(Box::new(dbus_secret_service::Error::Parse));
        assert!(matches!(classify(parse, bus_up), AuthError::Keyring { .. }));
    }

    #[test]
    fn not_supported_without_a_session_bus_means_no_secret_store() {
        let not_supported = || platform_failure("org.freedesktop.DBus.Error.NotSupported");

        assert!(matches!(classify(not_supported(), || false), AuthError::NoSecretStore));
        assert!(matches!(classify(not_supported(), || true), AuthError::Keyring { .. }));
    }
}
