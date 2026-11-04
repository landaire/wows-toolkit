use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// The `settings` row both front ends store [`LauncherSettings`] under.
pub const SETTINGS_KEY: &str = "launcher";

/// Persisted state for the Launcher tab.
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct LauncherSettings {
    #[serde(default)]
    pub manual_installs: Vec<PathBuf>,
    #[serde(default)]
    pub wgc_data_dir_override: Option<PathBuf>,
    #[serde(default)]
    pub proton_prefix: Option<PathBuf>,
    #[serde(default)]
    pub proton_binary: Option<PathBuf>,
    /// User-supplied extra command-line args applied to every launch.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// User-supplied environment variables applied to every launch.
    #[serde(default)]
    pub env_vars: Vec<(String, String)>,
    /// Args appended when Safe Mode is toggled. The exact payload that makes
    /// WoWs skip res_mods is confirmed during the launch-handshake capture; kept
    /// a setting so it is a one-line correction, not a code change.
    #[serde(default)]
    pub safe_mode_args: Vec<String>,
    /// WGNI tracking id (`tid`), generated once on first sign-in and reused
    /// after. Not secret: a stable per-install identifier WGC itself keeps in
    /// a device file.
    ///
    /// A raw `String` rather than `wgc_core::TrackingId` on purpose: that
    /// newtype's derived `Deserialize` accepts an empty string, so a
    /// hand-edited config could smuggle an invalid value past it. Keeping the
    /// field a bare `String` makes [`LauncherSettings::ensure_tracking_id`]
    /// the one place that validates and regenerates.
    #[serde(default)]
    pub tracking_id: Option<String>,
    /// Which signed-in account (if any) to load a session for from the OS
    /// keyring at startup. No secret material lives here; the token pair is
    /// only ever in the keyring.
    #[serde(default)]
    pub active_account: Option<PersistedAccount>,
}

/// Non-secret index of a signed-in account, persisted so startup knows which
/// keyring entry to load. See [`LauncherSettings::active_account`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedAccount {
    pub realm: wgc_auth::AccountRealm,
    pub account_id: wgc_auth::AccountId,
    pub nickname: wgc_auth::Nickname,
}

impl LauncherSettings {
    /// The Proton binary and prefix, when both are configured.
    pub fn proton_config(&self) -> Option<wgc_launch::proton::ProtonConfig> {
        match (&self.proton_binary, &self.proton_prefix) {
            (Some(binary), Some(prefix)) => {
                Some(wgc_launch::proton::ProtonConfig { proton_binary: binary.clone(), compat_prefix: prefix.clone() })
            }
            _ => None,
        }
    }

    /// The user's args and env for a launch, with the safe-mode args
    /// appended when `safe_mode` is set.
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

    /// The env vars as the `KEY=VALUE`-per-line text the settings field edits.
    pub fn env_vars_text(&self) -> String {
        self.env_vars.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n")
    }

    /// Replace the env vars from `KEY=VALUE`-per-line text. A line without
    /// `=` or with an empty key is dropped.
    pub fn set_env_vars_text(&mut self, text: &str) {
        self.env_vars = text
            .lines()
            .filter_map(|line| line.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
            .filter(|(k, _)| !k.is_empty())
            .collect();
    }

    /// The persisted WGNI tracking id (`tid`), minting one the first time.
    /// Returns whether the settings changed and need saving.
    ///
    /// WGNI only accepts a numeric tid, so an empty or non-numeric stored
    /// value (a legacy or hand-edited config) is replaced rather than reported:
    /// the user cannot act on that error.
    pub fn ensure_tracking_id(&mut self) -> (wgc_core::TrackingId, bool) {
        if let Some(raw) = &self.tracking_id
            && !raw.is_empty()
            && raw.bytes().all(|b| b.is_ascii_digit())
            && let Ok(id) = wgc_core::TrackingId::parse(raw)
        {
            return (id, false);
        }
        let generated = generate_tracking_id();
        let id = wgc_core::TrackingId::parse(&generated).expect("a generated tracking id is non-empty");
        self.tracking_id = Some(generated);
        (id, true)
    }
}

/// Mints a WGNI tracking id. WGC stores this as a nanosecond unix timestamp,
/// a 19-digit numeric string, and WGNI only accepts a numeric tid. The
/// toolkit has no device file, so it generates one from the current time.
fn generate_tracking_id() -> String {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(since_epoch) => since_epoch.as_nanos().to_string(),
        // A pre-epoch clock is not realistic; a random 19-digit number keeps
        // the tid WGNI-valid.
        Err(_) => rand::random_range(1_000_000_000_000_000_000u64..10_000_000_000_000_000_000u64).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_survive_a_json_round_trip() {
        let saved = LauncherSettings {
            manual_installs: vec![PathBuf::from("/games/WoWs/World_of_Warships")],
            proton_binary: Some(PathBuf::from("/steam/steamapps/common/Proton 10.0/proton")),
            proton_prefix: Some(PathBuf::from("/steam/steamapps/compatdata/552990")),
            extra_args: vec!["--foo".to_string()],
            tracking_id: Some("1755792000000000000".to_string()),
            active_account: Some(PersistedAccount {
                realm: wgc_auth::AccountRealm::Eu,
                account_id: wgc_auth::AccountId::new(697_769_785),
                nickname: wgc_auth::Nickname::new("gapedd"),
            }),
            ..Default::default()
        };
        let loaded: LauncherSettings = serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(loaded.manual_installs, saved.manual_installs);
        assert_eq!(loaded.proton_binary, saved.proton_binary);
        assert_eq!(loaded.proton_prefix, saved.proton_prefix);
        assert_eq!(loaded.extra_args, saved.extra_args);
        assert_eq!(loaded.tracking_id, saved.tracking_id);
        let account = loaded.active_account.expect("active_account round-trips");
        assert_eq!(account.realm, wgc_auth::AccountRealm::Eu);
        assert_eq!(account.account_id.get(), 697_769_785);
        assert_eq!(account.nickname.as_str(), "gapedd");
    }

    #[test]
    fn a_row_written_before_a_field_existed_still_loads() {
        let loaded: LauncherSettings = serde_json::from_str(r#"{"manual_installs":["/a"]}"#).unwrap();
        assert_eq!(loaded.manual_installs, vec![PathBuf::from("/a")]);
        assert!(loaded.active_account.is_none());
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
