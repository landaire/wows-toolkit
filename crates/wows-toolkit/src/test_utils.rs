use std::sync::Mutex;

static PANIC_HOOK_LOCK: Mutex<()> = Mutex::new(());

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
