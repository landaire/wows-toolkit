//! Process-level services the desktop front ends share: the HTTP client, the
//! tokio runtime, file dialogs, logging, settings storage, release updates and
//! the Twitch chat poll. Blocking
//! functions, futures and plain data only: each front end schedules them on its
//! own executor.

pub mod child_process;
pub mod dialog;
pub mod http;
pub mod logging;
pub mod runtime;
pub mod settings;
pub mod settings_writer;
pub mod supervision;
pub mod twitch;
pub mod update;
