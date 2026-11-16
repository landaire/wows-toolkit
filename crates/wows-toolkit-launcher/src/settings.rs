use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// The `settings` row both front ends store [`LauncherSettings`] under.
pub const SETTINGS_KEY: &str = "launcher";

/// Identifies a saved account and its keyring entry (`realm:account_id`). A WG account exists on
/// exactly one realm, so the realm is part of its identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountKey {
    pub realm: wgc_auth::AccountRealm,
    pub account_id: wgc_auth::AccountId,
}

impl AccountKey {
    pub fn of(session: &wgc_auth::StoredSession) -> AccountKey {
        AccountKey { realm: session.realm, account_id: session.account_id }
    }
}

/// One saved sign-in. No secret material: the token pair lives only in the keyring.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedAccount {
    pub key: AccountKey,
    pub nickname: wgc_auth::Nickname,
    pub install: InstallChoice,
}

/// Which install an account plays.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallChoice {
    /// The app's game directory (`wows_dir`).
    Toolkit,
    Path(PathBuf),
}

impl InstallChoice {
    /// The directory this choice names. `None` for [`InstallChoice::Toolkit`] while the app has
    /// no game directory.
    pub fn resolve(&self, wows_dir: Option<&Path>) -> Option<PathBuf> {
        match self {
            InstallChoice::Toolkit => wows_dir.map(Path::to_path_buf),
            InstallChoice::Path(path) => Some(path.clone()),
        }
    }
}

/// Which wording of the launcher's opt-in notice the user accepted. A new wording asks again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermsVersion(u32);

impl TermsVersion {
    /// The notice the front ends show now.
    pub const CURRENT: TermsVersion = TermsVersion(1);
}

/// The user's acceptance of the launcher's opt-in notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LauncherConsent {
    pub terms: TermsVersion,
}

/// Persisted launcher state.
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(from = "StoredLauncherSettings")]
pub struct LauncherSettings {
    pub manual_installs: Vec<PathBuf>,
    pub wgc_data_dir_override: Option<PathBuf>,
    /// Which Proton or umu build runs the game on Linux.
    pub runner: wgc_launch::RunnerChoice,
    /// User-supplied extra command-line args applied to every launch.
    pub extra_args: Vec<String>,
    /// User-supplied environment variables applied to every launch.
    pub env_vars: Vec<(String, String)>,
    /// Args appended when Safe Mode is toggled. Kept a setting so a change to the payload that
    /// makes WoWs skip res_mods is a one-line correction, not a code change.
    pub safe_mode_args: Vec<String>,
    /// WGNI tracking id (`tid`), generated once on first sign-in and reused after. Not secret: a
    /// stable per-install identifier WGC itself keeps in a device file.
    ///
    /// A raw `String` rather than `wgc_core::TrackingId` on purpose: that newtype's derived
    /// `Deserialize` accepts an empty string, so a hand-edited config could smuggle an invalid
    /// value past it. [`LauncherSettings::ensure_tracking_id`] is the one place that validates
    /// and regenerates.
    pub tracking_id: Option<String>,

    /// In the order the user arranged them. The keyring cannot list its entries, so this is the
    /// only index of saved sign-ins.
    pub accounts: Vec<SavedAccount>,
    /// Which account Play uses. Always one of `accounts` when set.
    pub default_account: Option<AccountKey>,
    /// Preselected realm in the add-account form.
    pub sign_in_realm: Option<wgc_auth::AccountRealm>,

    /// Steam installs that Play runs through the toolkit's WGC sign-in and the WGC layer instead
    /// of `steam -applaunch`.
    pub wgc_login_installs: Vec<PathBuf>,

    /// The opt-in that turns the launcher on. `None` keeps every launcher surface hidden and its
    /// background work stopped. Cleared when the user turns the launcher off, so turning it on
    /// again asks again.
    pub consent: Option<LauncherConsent>,
}

/// A stored value that may name something this version no longer supports, such as an account on
/// a realm WG has dropped. Such a value is skipped instead of failing the whole row.
#[derive(Deserialize)]
#[serde(untagged)]
enum Lenient<T> {
    Known(T),
    Unsupported(serde::de::IgnoredAny),
}

impl<T> Lenient<T> {
    fn known(self, what: &str) -> Option<T> {
        match self {
            Lenient::Known(value) => Some(value),
            Lenient::Unsupported(_) => {
                tracing::warn!("launcher settings: dropping an unsupported {what}");
                None
            }
        }
    }
}

/// The row as stored, including fields older versions wrote. Deserialization goes through this
/// so a row written with `active_account` becomes a one-entry account list.
#[derive(Deserialize)]
struct StoredLauncherSettings {
    #[serde(default)]
    manual_installs: Vec<PathBuf>,
    #[serde(default)]
    wgc_data_dir_override: Option<PathBuf>,
    #[serde(default)]
    runner: wgc_launch::RunnerChoice,
    #[serde(default)]
    extra_args: Vec<String>,
    #[serde(default)]
    env_vars: Vec<(String, String)>,
    #[serde(default)]
    safe_mode_args: Vec<String>,
    #[serde(default)]
    tracking_id: Option<String>,
    #[serde(default)]
    accounts: Vec<Lenient<SavedAccount>>,
    #[serde(default)]
    default_account: Option<Lenient<AccountKey>>,
    #[serde(default)]
    sign_in_realm: Option<Lenient<wgc_auth::AccountRealm>>,
    #[serde(default)]
    wgc_login_installs: Vec<PathBuf>,
    #[serde(default)]
    consent: Option<LauncherConsent>,
    /// The single signed-in account older versions kept.
    #[serde(default)]
    active_account: Option<Lenient<LegacyAccount>>,
}

#[derive(Deserialize)]
struct LegacyAccount {
    realm: wgc_auth::AccountRealm,
    account_id: wgc_auth::AccountId,
    nickname: wgc_auth::Nickname,
}

impl From<StoredLauncherSettings> for LauncherSettings {
    fn from(stored: StoredLauncherSettings) -> LauncherSettings {
        let StoredLauncherSettings {
            manual_installs,
            wgc_data_dir_override,
            runner,
            extra_args,
            env_vars,
            safe_mode_args,
            tracking_id,
            accounts,
            default_account,
            sign_in_realm,
            wgc_login_installs,
            consent,
            active_account,
        } = stored;
        let mut accounts: Vec<SavedAccount> =
            accounts.into_iter().filter_map(|account| account.known("account")).collect();
        let mut default_account = default_account.and_then(|key| key.known("default account"));
        let mut sign_in_realm = sign_in_realm.and_then(|realm| realm.known("sign-in realm"));
        let active_account = active_account.and_then(|account| account.known("signed-in account"));

        if let Some(legacy) = active_account
            && accounts.is_empty()
        {
            let key = AccountKey { realm: legacy.realm, account_id: legacy.account_id };
            accounts.push(SavedAccount { key, nickname: legacy.nickname, install: InstallChoice::Toolkit });
            default_account = Some(key);
            sign_in_realm = sign_in_realm.or(Some(legacy.realm));
        }

        // A default that names no saved account is dropped rather than left dangling.
        if default_account.is_some_and(|key| !accounts.iter().any(|account| account.key == key)) {
            default_account = None;
        }

        LauncherSettings {
            manual_installs,
            wgc_data_dir_override,
            runner,
            extra_args,
            env_vars,
            safe_mode_args,
            tracking_id,
            accounts,
            default_account,
            sign_in_realm,
            wgc_login_installs,
            consent,
        }
    }
}

impl LauncherSettings {
    /// Whether the user opted in to the launcher under the current notice.
    pub fn is_enabled(&self) -> bool {
        self.consent.is_some_and(|consent| consent.terms == TermsVersion::CURRENT)
    }

    /// Turns the launcher on, recording that the user accepted the current notice.
    pub fn enable(&mut self) {
        self.consent = Some(LauncherConsent { terms: TermsVersion::CURRENT });
    }

    /// Turns the launcher off and drops the acceptance.
    pub fn disable(&mut self) {
        self.consent = None;
    }

    /// The user's args and env for a launch, with the safe-mode args appended when `safe_mode`
    /// is set.
    pub fn launch_args(&self, safe_mode: bool) -> wgc_launch::LaunchArgs {
        let mut extra = self.extra_args.clone();
        if safe_mode {
            extra.extend(self.safe_mode_args.iter().cloned());
        }
        wgc_launch::LaunchArgs { extra, env: self.env_vars.clone() }
    }

    /// The extra args as the one-per-line text the settings field edits.
    pub fn extra_args_text(&self) -> String {
        self.extra_args.join("\n")
    }

    /// Replace the extra args from one-per-line text; blank lines are dropped.
    pub fn set_extra_args_text(&mut self, text: &str) {
        self.extra_args = text.lines().map(str::to_string).filter(|s| !s.is_empty()).collect();
    }

    /// The safe-mode args as one-per-line text.
    pub fn safe_mode_args_text(&self) -> String {
        self.safe_mode_args.join("\n")
    }

    /// Replace the safe-mode args from one-per-line text; blank lines are dropped.
    pub fn set_safe_mode_args_text(&mut self, text: &str) {
        self.safe_mode_args = text.lines().map(str::to_string).filter(|s| !s.is_empty()).collect();
    }

    /// The env vars as the `KEY=VALUE`-per-line text the settings field edits.
    pub fn env_vars_text(&self) -> String {
        self.env_vars.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n")
    }

    /// Replace the env vars from `KEY=VALUE`-per-line text. A line without `=` or with an empty
    /// key is dropped.
    pub fn set_env_vars_text(&mut self, text: &str) {
        self.env_vars = text
            .lines()
            .filter_map(|line| line.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
            .filter(|(k, _)| !k.is_empty())
            .collect();
    }

    pub fn account(&self, key: AccountKey) -> Option<&SavedAccount> {
        self.accounts.iter().find(|account| account.key == key)
    }

    /// The account Play uses.
    pub fn default_account(&self) -> Option<&SavedAccount> {
        self.default_account.and_then(|key| self.account(key))
    }

    /// Adds the account, or refreshes the nickname of the one already listed. The first account
    /// saved becomes the default. Returns whether anything changed.
    pub fn upsert_account(&mut self, key: AccountKey, nickname: wgc_auth::Nickname) -> bool {
        let changed = match self.accounts.iter_mut().find(|account| account.key == key) {
            Some(account) if account.nickname == nickname => false,
            Some(account) => {
                account.nickname = nickname;
                true
            }
            None => {
                self.accounts.push(SavedAccount { key, nickname, install: InstallChoice::Toolkit });
                true
            }
        };

        let realm_changed = self.sign_in_realm != Some(key.realm);
        self.sign_in_realm = Some(key.realm);
        let default_changed = self.default_account.is_none();
        if default_changed {
            self.default_account = Some(key);
        }

        changed || realm_changed || default_changed
    }

    /// Drops the account's row. The next account in the list becomes the default when it was.
    pub fn remove_account(&mut self, key: AccountKey) -> bool {
        let before = self.accounts.len();
        self.accounts.retain(|account| account.key != key);
        if self.default_account == Some(key) {
            self.default_account = self.accounts.first().map(|account| account.key);
        }

        self.accounts.len() != before
    }

    /// Makes `key` the account Play uses. A no-op for an account not in the list.
    pub fn set_default_account(&mut self, key: AccountKey) -> bool {
        if self.account(key).is_none() || self.default_account == Some(key) {
            return false;
        }
        self.default_account = Some(key);

        true
    }

    /// Moves the account at `from` to `to`, shifting the ones between.
    pub fn reorder_accounts(&mut self, from: usize, to: usize) -> bool {
        if from == to || from >= self.accounts.len() || to >= self.accounts.len() {
            return false;
        }
        let account = self.accounts.remove(from);
        self.accounts.insert(to, account);

        true
    }

    /// Points the account at another install.
    pub fn set_account_install(&mut self, key: AccountKey, install: InstallChoice) -> bool {
        let Some(account) = self.accounts.iter_mut().find(|account| account.key == key) else {
            return false;
        };
        if account.install == install {
            return false;
        }
        account.install = install;

        true
    }

    /// Whether Play runs the Steam install at `install` through the WGC sign-in.
    pub fn uses_wgc_login(&self, install: &Path) -> bool {
        self.wgc_login_installs.iter().any(|path| path == install)
    }

    /// Turns the WGC sign-in on or off for the Steam install at `install`. Returns whether
    /// anything changed.
    pub fn set_wgc_login(&mut self, install: &Path, on: bool) -> bool {
        if self.uses_wgc_login(install) == on {
            return false;
        }
        if on {
            self.wgc_login_installs.push(install.to_path_buf());
        } else {
            self.wgc_login_installs.retain(|path| path != install);
        }

        true
    }

    /// The stored WGNI tracking id, when one was minted and is still valid.
    pub(crate) fn tracking_id(&self) -> Option<wgc_core::TrackingId> {
        let raw = self.tracking_id.as_deref()?;
        if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }

        wgc_core::TrackingId::parse(raw).ok()
    }

    /// The persisted WGNI tracking id (`tid`), minting one the first time. Returns whether the
    /// settings changed and need saving.
    ///
    /// WGNI only accepts a numeric tid, so an empty or non-numeric stored value (a legacy or
    /// hand-edited config) is replaced rather than reported: the user cannot act on that error.
    pub fn ensure_tracking_id(&mut self) -> (wgc_core::TrackingId, bool) {
        if let Some(id) = self.tracking_id() {
            return (id, false);
        }
        let generated = generate_tracking_id();
        let id = wgc_core::TrackingId::parse(&generated).expect("a generated tracking id is non-empty");
        self.tracking_id = Some(generated);
        (id, true)
    }
}

/// Mints a WGNI tracking id. WGC stores this as a nanosecond unix timestamp, a 19-digit numeric
/// string, and WGNI only accepts a numeric tid. The toolkit has no device file, so it generates
/// one from the current time.
fn generate_tracking_id() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(since_epoch) => since_epoch.as_nanos().to_string(),
        // A pre-epoch clock is not realistic; a random 19-digit number keeps the tid WGNI-valid.
        Err(_) => rand::random_range(1_000_000_000_000_000_000u64..10_000_000_000_000_000_000u64).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(realm: wgc_auth::AccountRealm, id: u64) -> AccountKey {
        AccountKey { realm, account_id: wgc_auth::AccountId::new(id) }
    }

    #[test]
    fn a_row_with_the_single_active_account_becomes_a_one_entry_list() {
        let row = r#"{
            "manual_installs": ["/games/WoWs"],
            "tracking_id": "1755792000000000000",
            "active_account": {"realm": "Eu", "account_id": 697769785, "nickname": "gapedd"}
        }"#;

        let loaded: LauncherSettings = serde_json::from_str(row).unwrap();

        insta::assert_yaml_snapshot!(loaded);
    }

    #[test]
    fn a_row_from_a_signed_in_account_starts_with_the_launcher_off() {
        let row = r#"{"active_account": {"realm": "Na", "account_id": 1034874630, "nickname": "Ahab"}}"#;
        let loaded: LauncherSettings = serde_json::from_str(row).unwrap();
        assert!(!loaded.is_enabled());
    }

    #[test]
    fn acceptance_of_an_older_notice_does_not_enable_the_launcher() {
        let row = r#"{"consent": {"terms": 0}}"#;
        let loaded: LauncherSettings = serde_json::from_str(row).unwrap();
        assert!(!loaded.is_enabled());
    }

    #[test]
    fn a_migrated_row_saves_without_the_old_field_and_loads_the_same() {
        let row = r#"{"active_account": {"realm": "Na", "account_id": 1034874630, "nickname": "Ahab"}}"#;
        let loaded: LauncherSettings = serde_json::from_str(row).unwrap();

        let saved = serde_json::to_value(&loaded).unwrap();
        assert!(saved.get("active_account").is_none());
        let reloaded: LauncherSettings = serde_json::from_value(saved).unwrap();
        assert_eq!(reloaded.accounts, loaded.accounts);
        assert_eq!(reloaded.default_account, loaded.default_account);
    }

    #[test]
    fn a_row_with_an_account_list_ignores_a_leftover_active_account() {
        let row = r#"{
            "accounts": [{"key": {"realm": "Asia", "account_id": 2051681179}, "nickname": "Starbuck", "install": "Toolkit"}],
            "active_account": {"realm": "Eu", "account_id": 697769785, "nickname": "gapedd"}
        }"#;

        let loaded: LauncherSettings = serde_json::from_str(row).unwrap();

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].nickname.as_str(), "Starbuck");
        assert_eq!(loaded.default_account, None);
    }

    #[test]
    fn accounts_on_an_unsupported_realm_are_dropped_and_the_rest_load() {
        let row = r#"{
            "accounts": [
                {"key": {"realm": "Ru", "account_id": 12345}, "nickname": "Ivan", "install": "Toolkit"},
                {"key": {"realm": "Na", "account_id": 1034874630}, "nickname": "Ahab", "install": "Toolkit"}
            ],
            "default_account": {"realm": "Ru", "account_id": 12345},
            "sign_in_realm": "Ru",
            "manual_installs": ["/games/WoWs"]
        }"#;

        let loaded: LauncherSettings = serde_json::from_str(row).unwrap();

        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].nickname.as_str(), "Ahab");
        assert_eq!(loaded.default_account, None);
        assert_eq!(loaded.sign_in_realm, None);
        assert_eq!(loaded.manual_installs, vec![PathBuf::from("/games/WoWs")]);
    }

    #[test]
    fn a_legacy_account_on_an_unsupported_realm_is_dropped() {
        let row = r#"{"active_account": {"realm": "Ru", "account_id": 12345, "nickname": "Ivan"}}"#;
        let loaded: LauncherSettings = serde_json::from_str(row).unwrap();
        assert!(loaded.accounts.is_empty());
    }

    #[test]
    fn a_default_naming_no_saved_account_is_dropped() {
        let row = r#"{"default_account": {"realm": "Eu", "account_id": 1}}"#;
        let loaded: LauncherSettings = serde_json::from_str(row).unwrap();
        assert_eq!(loaded.default_account, None);
    }

    #[test]
    fn signing_in_again_refreshes_the_entry_instead_of_adding_one() {
        let mut settings = LauncherSettings::default();
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let na = key(wgc_auth::AccountRealm::Na, 1_034_874_630);

        assert!(settings.upsert_account(eu, wgc_auth::Nickname::new("old")));
        assert!(settings.upsert_account(na, wgc_auth::Nickname::new("Ahab")));
        assert!(settings.upsert_account(eu, wgc_auth::Nickname::new("new")));

        assert_eq!(settings.accounts.len(), 2);
        assert_eq!(settings.account(eu).unwrap().nickname.as_str(), "new");
        assert_eq!(settings.default_account, Some(eu), "the first account saved stays the default");
        assert_eq!(settings.sign_in_realm, Some(wgc_auth::AccountRealm::Eu));
    }

    #[test]
    fn removing_the_default_account_moves_the_default_to_the_next() {
        let mut settings = LauncherSettings::default();
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let na = key(wgc_auth::AccountRealm::Na, 1_034_874_630);
        let _ = settings.upsert_account(eu, wgc_auth::Nickname::new("a"));
        let _ = settings.upsert_account(na, wgc_auth::Nickname::new("b"));

        assert!(settings.remove_account(eu));
        assert_eq!(settings.default_account, Some(na));
        assert!(settings.remove_account(na));
        assert_eq!(settings.default_account, None);
    }

    #[test]
    fn env_var_text_drops_lines_without_a_key() {
        let mut settings = LauncherSettings::default();
        settings.set_env_vars_text("A=1\nnot a pair\n=2\n B = 3 ");
        assert_eq!(settings.env_vars, vec![("A".into(), "1".into()), ("B".into(), "3".into())]);
    }

    #[test]
    fn safe_mode_appends_its_args_after_the_users() {
        let settings = LauncherSettings {
            extra_args: vec!["-user".into()],
            safe_mode_args: vec!["-safe".into()],
            ..Default::default()
        };
        assert_eq!(settings.launch_args(false).extra, vec!["-user".to_string()]);
        assert_eq!(settings.launch_args(true).extra, vec!["-user".to_string(), "-safe".to_string()]);
    }

    #[test]
    fn a_non_numeric_tracking_id_is_replaced() {
        let mut settings = LauncherSettings { tracking_id: Some("abc".into()), ..Default::default() };
        let (_, changed) = settings.ensure_tracking_id();
        assert!(changed);
        let stored = settings.tracking_id.clone().unwrap();
        assert!(stored.bytes().all(|b| b.is_ascii_digit()));
        let (_, changed_again) = settings.ensure_tracking_id();
        assert!(!changed_again);
        assert_eq!(settings.tracking_id.unwrap(), stored);
    }
}
