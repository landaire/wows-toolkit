use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use super::*;

/// What the stand-in secret store does when asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Stores,
    /// No Secret Service provider on the bus.
    Missing,
    /// A provider whose collection stays locked, or whose prompt was dismissed.
    Locked,
}

/// An in-memory secret store: user name to (label, secret).
struct FakeKeyring {
    answer: Mutex<Answer>,
    entries: Mutex<BTreeMap<String, (String, String)>>,
}

impl FakeKeyring {
    fn new(answer: Answer) -> Arc<FakeKeyring> {
        Arc::new(FakeKeyring { answer: Mutex::new(answer), entries: Mutex::new(BTreeMap::new()) })
    }

    fn answer_with(&self, answer: Answer) {
        *self.answer.lock().unwrap() = answer;
    }

    fn check(&self) -> Result<(), AuthError> {
        match *self.answer.lock().unwrap() {
            Answer::Stores => Ok(()),
            Answer::Missing => Err(AuthError::NoSecretStore),
            Answer::Locked => Err(AuthError::KeyringLocked),
        }
    }

    fn labels(&self) -> Vec<(String, String)> {
        self.entries.lock().unwrap().iter().map(|(user, (label, _))| (user.clone(), label.clone())).collect()
    }
}

impl SecretStore for FakeKeyring {
    fn get(&self, entry: &KeyringEntry) -> Result<Option<String>, AuthError> {
        self.check()?;

        Ok(self.entries.lock().unwrap().get(&entry.user).map(|(_, secret)| secret.clone()))
    }

    fn set(&self, entry: &KeyringEntry, secret: &str) -> Result<(), AuthError> {
        self.check()?;
        self.entries.lock().unwrap().insert(entry.user.clone(), (entry.label.clone(), secret.to_string()));

        Ok(())
    }

    fn delete(&self, entry: &KeyringEntry) -> Result<(), AuthError> {
        self.check()?;
        self.entries.lock().unwrap().remove(&entry.user);

        Ok(())
    }
}

fn session(nickname: &str) -> StoredSession {
    StoredSession {
        realm: AccountRealm::Eu,
        account_id: AccountId::new(697_769_785),
        nickname: Nickname::new(nickname),
        tokens: TokenPair {
            access_token: AccessToken::new("secret-token".to_string()),
            exchange_code: ExchangeCode::from_raw("DEADBEEF00112233445566778899AABB".to_string()),
        },
        expiry: TokenExpiry::KnownAt(UNIX_EPOCH + Duration::from_secs(1_800_000_000)),
    }
}

fn store(keyring: &Arc<FakeKeyring>, dir: &tempfile::TempDir) -> CredentialStore {
    CredentialStore::with_backends(Arc::clone(keyring) as Arc<dyn SecretStore>, Some(PlaintextFile::in_dir(dir.path())))
}

fn file_path(dir: &tempfile::TempDir) -> PathBuf {
    PlaintextFile::in_dir(dir.path()).path().to_path_buf()
}

#[test]
fn without_a_secret_store_sessions_and_token2_go_to_the_plaintext_file() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Missing);
    let store = store(&keyring, &dir);
    let saved = session("Captain_Ahab");

    let session_backend = store.save_session(&saved).unwrap();
    let token2_backend = store
        .save_token2(saved.realm, saved.account_id, &saved.nickname, &GameToken2::new("token2-secret".into()))
        .unwrap();
    let loaded = store.load_session(saved.realm, saved.account_id).unwrap().expect("session saved");
    let token2 = store.load_token2(saved.realm, saved.account_id).unwrap().expect("token2 saved");

    assert_eq!(session_backend, StorageBackend::PlaintextFile);
    assert_eq!(token2_backend, StorageBackend::PlaintextFile);
    assert_eq!(loaded.backend, StorageBackend::PlaintextFile);
    assert_eq!(loaded.value.nickname.as_str(), "Captain_Ahab");
    assert_eq!(loaded.value.tokens.access_token.expose_secret(), "secret-token");
    assert_eq!(token2.value.expose_secret(), "token2-secret");
    insta::assert_snapshot!(std::fs::read_to_string(file_path(&dir)).unwrap(), @r#"{"sessions":{"eu:697769785":"{\"access_token\":\"secret-token\",\"exchange_code\":\"DEADBEEF00112233445566778899AABB\",\"nickname\":\"Captain_Ahab\",\"expiry\":{\"KnownAt\":{\"unix_secs\":1800000000}}}"},"token2":{"eu:697769785":"token2-secret"}}"#);
}

#[cfg(unix)]
#[test]
fn the_plaintext_file_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Missing);

    store(&keyring, &dir).save_session(&session("Captain_Ahab")).unwrap();

    let path = file_path(&dir);
    let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let dir_mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
    assert_eq!((file_mode, dir_mode), (0o600, 0o700));
}

#[test]
fn a_plaintext_write_waits_for_a_lock_held_elsewhere() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Missing);
    let store = store(&keyring, &dir);
    let saved = session("Captain_Ahab");
    let lock_path = PlaintextFile::in_dir(dir.path()).lock_path();
    std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();

    // A separate open file stands in for another process: flock and LockFileEx conflict
    // between handles, not only between processes.
    let other = std::fs::File::create(&lock_path).unwrap();
    other.lock().unwrap();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let writer = std::thread::spawn(move || {
        let backend = store.save_session(&saved);
        done_tx.send(()).unwrap();
        backend
    });

    let waited = done_rx.recv_timeout(Duration::from_millis(200));
    drop(other);

    assert!(waited.is_err(), "the save finished while another handle held the lock");
    assert_eq!(writer.join().unwrap().unwrap(), StorageBackend::PlaintextFile);
    assert!(file_path(&dir).exists());
}

#[test]
fn a_locked_keyring_never_falls_back_to_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Locked);
    let store = store(&keyring, &dir);

    let saved = store.save_session(&session("Captain_Ahab"));
    let loaded = store.load_session(AccountRealm::Eu, AccountId::new(697_769_785));

    assert!(matches!(saved, Err(AuthError::KeyringLocked)));
    assert!(matches!(loaded, Err(AuthError::KeyringLocked)));
    assert!(!file_path(&dir).exists());
}

#[test]
fn entries_move_into_the_keyring_once_it_answers_and_the_file_goes() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Missing);
    let store = store(&keyring, &dir);
    let saved = session("Captain_Ahab");
    store.save_session(&saved).unwrap();
    store
        .save_token2(saved.realm, saved.account_id, &saved.nickname, &GameToken2::new("token2-secret".into()))
        .unwrap();

    keyring.answer_with(Answer::Stores);
    let loaded = store.load_session(saved.realm, saved.account_id).unwrap().expect("session moved");

    assert_eq!(loaded.backend, StorageBackend::Keyring);
    assert_eq!(loaded.value.tokens.access_token.expose_secret(), "secret-token");
    assert!(!file_path(&dir).exists());
    insta::assert_debug_snapshot!(keyring.labels(), @r#"
    [
        (
            "eu:697769785",
            "WoWs Toolkit: Wargaming sign-in for Captain_Ahab (EU)",
        ),
        (
            "token2:eu:697769785",
            "WoWs Toolkit: game token for Captain_Ahab (EU)",
        ),
    ]
    "#);
}

#[test]
fn a_file_entry_is_newer_than_the_keyring_entry_it_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Stores);
    let store = store(&keyring, &dir);
    store.save_session(&session("Old_Name")).unwrap();
    keyring.answer_with(Answer::Missing);
    store.save_session(&session("New_Name")).unwrap();

    keyring.answer_with(Answer::Stores);
    let loaded = store.load_session(AccountRealm::Eu, AccountId::new(697_769_785)).unwrap().expect("session");

    assert_eq!(loaded.value.nickname.as_str(), "New_Name");
    assert!(!file_path(&dir).exists());
}

#[test]
fn a_save_to_the_keyring_is_not_replaced_by_an_older_file_entry() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Missing);
    let store = store(&keyring, &dir);
    store.save_session(&session("Old_Name")).unwrap();

    keyring.answer_with(Answer::Stores);
    store.save_session(&session("New_Name")).unwrap();
    let loaded = store.load_session(AccountRealm::Eu, AccountId::new(697_769_785)).unwrap().expect("session");

    assert_eq!(loaded.value.nickname.as_str(), "New_Name");
    assert!(!file_path(&dir).exists());
}

#[test]
fn deleting_removes_the_entry_from_both_backends() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Stores);
    let store = store(&keyring, &dir);
    let saved = session("Captain_Ahab");
    store.save_session(&saved).unwrap();
    keyring.answer_with(Answer::Missing);
    store
        .save_token2(saved.realm, saved.account_id, &saved.nickname, &GameToken2::new("token2-secret".into()))
        .unwrap();
    store.save_session(&saved).unwrap();

    keyring.answer_with(Answer::Stores);
    store.delete_session(saved.realm, saved.account_id).unwrap();
    store.delete_token2(saved.realm, saved.account_id).unwrap();

    assert!(keyring.labels().is_empty());
    assert!(!file_path(&dir).exists());
}

#[test]
fn deleting_without_a_secret_store_clears_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Missing);
    let store = store(&keyring, &dir);
    let saved = session("Captain_Ahab");
    store.save_session(&saved).unwrap();

    store.delete_session(saved.realm, saved.account_id).unwrap();

    assert!(store.load_session(saved.realm, saved.account_id).unwrap().is_none());
    assert!(!file_path(&dir).exists());
}

#[test]
fn keyring_entries_keep_their_lookup_keys_and_get_descriptive_labels() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Stores);
    let store = store(&keyring, &dir);
    let saved = session("Captain_Ahab");

    store.save_session(&saved).unwrap();
    store
        .save_token2(AccountRealm::Asia, AccountId::new(2_051_681_179), &saved.nickname, &GameToken2::new("t".into()))
        .unwrap();

    assert!(!file_path(&dir).exists());
    insta::assert_debug_snapshot!(keyring.labels(), @r#"
    [
        (
            "eu:697769785",
            "WoWs Toolkit: Wargaming sign-in for Captain_Ahab (EU)",
        ),
        (
            "token2:asia:2051681179",
            "WoWs Toolkit: game token for Captain_Ahab (Asia)",
        ),
    ]
    "#);
}

#[test]
fn probing_reports_where_a_save_would_go() {
    let dir = tempfile::tempdir().unwrap();
    let keyring = FakeKeyring::new(Answer::Missing);
    let store = store(&keyring, &dir);
    let without_dir = CredentialStore::with_backends(Arc::clone(&keyring) as Arc<dyn SecretStore>, None);

    let missing = store.probe().unwrap();
    let missing_without_dir = without_dir.probe();
    keyring.answer_with(Answer::Locked);
    let locked = store.probe();
    keyring.answer_with(Answer::Stores);
    let stores = store.probe().unwrap();

    assert_eq!(missing, StorageBackend::PlaintextFile);
    assert!(matches!(missing_without_dir, Err(AuthError::NoSecretStore)));
    assert!(matches!(locked, Err(AuthError::KeyringLocked)));
    assert_eq!(stores, StorageBackend::Keyring);
}

#[test]
fn blob_roundtrips_known_expiry_to_the_same_unix_second() {
    let expires_at = SystemTime::now() + Duration::from_secs(3600);
    let saved = StoredSession { expiry: TokenExpiry::KnownAt(expires_at), ..session("Other") };
    let json = serde_json::to_string(&StoredSessionBlob::from_session(&saved)).expect("serializes");
    let restored: StoredSessionBlob = serde_json::from_str(&json).expect("parses");
    let restored = restored.into_session(saved.realm, saved.account_id);

    let TokenExpiry::KnownAt(t) = restored.expiry else {
        panic!("expected KnownAt");
    };
    assert_eq!(
        t.duration_since(UNIX_EPOCH).unwrap().as_secs(),
        expires_at.duration_since(UNIX_EPOCH).unwrap().as_secs()
    );
}

// Keyring-touching: CI/headless environments have no Secret Service. Run manually on a real
// desktop session.
#[test]
#[ignore = "touches the OS keyring / Secret Service"]
fn save_load_delete_roundtrip_via_os_keyring() {
    let store = CredentialStore::new(None);
    let saved = StoredSession { account_id: AccountId::new(999_999_001), ..session("KeyringTest") };

    assert_eq!(store.save_session(&saved).expect("save"), StorageBackend::Keyring);
    let loaded = store.load_session(saved.realm, saved.account_id).expect("load").expect("stored");
    assert_eq!(loaded.value.nickname.as_str(), "KeyringTest");
    store.delete_session(saved.realm, saved.account_id).expect("delete");
    assert!(store.load_session(saved.realm, saved.account_id).expect("load after delete").is_none());
}

#[test]
#[ignore = "touches the OS keyring / Secret Service"]
fn game_token2_save_load_delete_roundtrip_via_os_keyring() {
    let store = CredentialStore::new(None);
    let realm = AccountRealm::Na;
    let account_id = AccountId::new(1_999_999_001);
    let nickname = Nickname::new("KeyringTest");

    store.save_token2(realm, account_id, &nickname, &GameToken2::new("keyring-test-token2".to_string())).expect("save");
    let loaded = store.load_token2(realm, account_id).expect("load").expect("token2 stored");
    assert_eq!(loaded.value.expose_secret(), "keyring-test-token2");

    // A session entry for the same account must not shadow the token2 entry.
    assert!(store.load_session(realm, account_id).expect("load session").is_none());

    store.delete_token2(realm, account_id).expect("delete");
    assert!(store.load_token2(realm, account_id).expect("load after delete").is_none());
}
