use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

static PANIC_HOOK_LOCK: Mutex<()> = Mutex::new(());

/// Block until `inbox` yields its first queued message or `timeout` passes.
/// Later messages taken in the same poll are dropped, so use this only where
/// the test cares about the first message (or the only one).
pub(crate) fn recv_inbox_timeout<T>(inbox: &egui_inbox::UiInbox<T>, timeout: Duration) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(value) = inbox.read_without_ctx().next() {
            return Some(value);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Block until a completion channel reports its worker's result. `None` on
/// timeout or when the worker dropped its sender without sending.
pub(crate) fn recv_completion_timeout(
    inbox: &egui_inbox::UiInbox<crate::task::CompletionEvent>,
    timeout: Duration,
) -> Option<crate::task::TaskCompletion> {
    match recv_inbox_timeout(inbox, timeout)? {
        crate::ui_channel::StreamEvent::Item(result) => Some(result),
        crate::ui_channel::StreamEvent::Closed => None,
    }
}

pub(crate) fn with_silenced_panic_hook<T>(body: impl FnOnce() -> T) -> T {
    let lock = PANIC_HOOK_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
    std::panic::set_hook(previous);
    drop(lock);
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn silenced_hook_is_restored_after_the_callback_panics() {
        let panicked = std::panic::catch_unwind(|| super::with_silenced_panic_hook(|| panic!("expected")));
        assert!(panicked.is_err());
        assert_eq!(super::with_silenced_panic_hook(|| 7), 7);
    }
}
