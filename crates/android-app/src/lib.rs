//! Calvados Android app: the foreground service behind `PersistentService.java`.

pub use protocol as proto;

mod bluetooth;
mod exports;
mod intents;
mod jni;
mod link;
mod media;
mod notification;
mod service;

use std::path::Path;
use std::sync::Once;

static TRACING_INIT: Once = Once::new();

/// Debug for our crates; the per-frame `aacp_raw` hex trace would flood
/// logcat's small buffer, so only the log file gets it.
const CONSOLE_FILTER: &str = "calvados=debug,protocol=debug,info";
const FILE_FILTER: &str = "calvados=debug,protocol=debug,protocol::aacp_raw=trace,info";

/// Log to logcat and, given a directory, to daily files there, two weeks
/// kept (~0.5 MB a day) to look back at a reboot.
pub(crate) fn init_tracing(log_dir: Option<&Path>) {
    use tracing_appender::rolling::{RollingFileAppender, Rotation};
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{EnvFilter, fmt, registry};

    TRACING_INIT.call_once(|| {
        let filter = |default: &str| {
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default))
        };
        #[cfg(target_os = "android")]
        let console = paranoid_android::layer(env!("CARGO_PKG_NAME"))
            .with_span_events(fmt::format::FmtSpan::CLOSE)
            .with_thread_names(true);
        #[cfg(not(target_os = "android"))]
        let console = fmt::layer();

        let appender = log_dir.map(|dir| {
            RollingFileAppender::builder()
                .rotation(Rotation::DAILY)
                .filename_prefix("calvados")
                .filename_suffix("log")
                .max_log_files(14)
                .build(dir)
        });
        let (appender, error) = match appender {
            Some(Ok(appender)) => (Some(appender), None),
            Some(Err(e)) => (None, Some(e)),
            None => (None, None),
        };
        let file = appender.map(|appender| {
            fmt::layer()
                .with_ansi(false)
                .with_thread_names(true)
                .with_writer(appender)
                .with_filter(filter(FILE_FILTER))
        });
        let _ = registry()
            .with(console.with_filter(filter(CONSOLE_FILTER)))
            .with(file)
            .try_init();
        if let Some(e) = error {
            tracing::warn!("log file unavailable: {e}");
        }
    });
}
