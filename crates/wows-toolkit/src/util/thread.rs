use std::thread::{self, JoinHandle};

/// Spawn a thread that logs panics instead of silently dying.
///
/// Works exactly like `std::thread::spawn` but wraps the closure in
/// `catch_unwind`. If the closure panics the panic message is logged
/// as an error and the thread exits normally (returning `None`).
pub fn spawn_logged<F, T>(name: &str, f: F) -> JoinHandle<Option<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let thread_name = name.to_owned();
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(val) => Some(val),
            Err(payload) => {
                let msg = panic_payload_to_string(&payload);
                tracing::error!("thread '{}' panicked: {}", thread_name, msg);
                None
            }
        })
        .expect("failed to spawn thread")
}

fn panic_payload_to_string(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}
