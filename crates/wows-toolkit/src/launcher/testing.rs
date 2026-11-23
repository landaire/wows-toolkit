//! Lets a test keep the launcher from reading its settings row, so the test sets the state itself
//! instead of the network, the keyring and this machine's installs.

use std::cell::Cell;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

thread_local! {
    static HELD: Cell<bool> = const { Cell::new(false) };
    static STORE: RefCell<Arc<TestStore>> = RefCell::new(Arc::new(TestStore::default()));
}

/// Keeps a shell built on this thread from reading the launcher's settings row and starting its
/// work, and gives it a fresh secret store, so no read count or closed gate carries over from an
/// earlier test on the thread. The launcher stays unloaded until the test adopts settings.
pub fn hold_startup() {
    HELD.set(true);
    STORE.replace(Arc::new(TestStore::default()));
}

/// Lets a shell built on this thread read its settings row as the app does, with a fresh secret
/// store. Shells that are not launcher tests start here, so a hold left by an earlier test on the
/// thread does not change what they show.
pub fn release_startup() {
    HELD.set(false);
    STORE.replace(Arc::new(TestStore::default()));
}

pub(super) fn held() -> bool {
    HELD.get()
}

/// The secret store the launcher on this thread uses in place of the OS keyring.
pub fn store() -> Arc<TestStore> {
    STORE.with_borrow(Arc::clone)
}

/// A secret store that holds nothing and counts its reads. While closed, a read waits, as one
/// behind an unlock prompt does.
#[derive(Default)]
pub struct TestStore {
    reads: AtomicUsize,
    closed: Mutex<bool>,
    opened: Condvar,
}

impl TestStore {
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }

    pub fn close(&self) {
        *self.closed.lock().expect("the gate is not poisoned") = true;
    }

    pub fn open(&self) {
        *self.closed.lock().expect("the gate is not poisoned") = false;
        self.opened.notify_all();
    }
}

impl wgc_auth::SecretStore for TestStore {
    fn get(&self, _: &wgc_auth::KeyringEntry) -> Result<Option<String>, wgc_auth::AuthError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let closed = self.closed.lock().expect("the gate is not poisoned");
        drop(self.opened.wait_while(closed, |closed| *closed).expect("the gate is not poisoned"));

        Ok(None)
    }

    fn set(&self, _: &wgc_auth::KeyringEntry, _: &str) -> Result<(), wgc_auth::AuthError> {
        Ok(())
    }

    fn delete(&self, _: &wgc_auth::KeyringEntry) -> Result<(), wgc_auth::AuthError> {
        Ok(())
    }
}
