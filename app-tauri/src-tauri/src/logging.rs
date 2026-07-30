use std::io::{self, Write};
use std::path::Path;

use tracing_subscriber::EnvFilter;

use crate::redaction;

struct RedactingMakeWriter {
    inner: tracing_appender::rolling::RollingFileAppender,
}

struct RedactingWriter<'a> {
    inner: tracing_appender::rolling::RollingWriter<'a>,
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for RedactingMakeWriter {
    type Writer = RedactingWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.make_writer(),
        }
    }
}

impl Write for RedactingWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buffer);
        self.inner.write_all(redaction::redact(&text).as_bytes())?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Build the log filter.
///
/// Without `RUST_LOG` set, any target not matched by a directive defaults to
/// **ERROR only**. The filter used to carry a single `borg_ui=debug` directive,
/// which covered this crate — whose real name is `borg_ui_lib`, per the `[lib]`
/// rename in `src-tauri/Cargo.toml`, and which matched only because directives
/// match by target prefix. `borg_core`, where the backup/restore/SSH logging
/// actually lives (`borg.rs`, `ssh.rs`), matched nothing, so every event below
/// ERROR was dropped and exported log files came out effectively empty.
///
/// `env_directives` is the raw `RUST_LOG` value (empty when unset) rather than
/// being read in here, so the no-`RUST_LOG` case — the one that shipped broken —
/// is testable without mutating process environment.
fn log_filter(env_directives: &str) -> EnvFilter {
    EnvFilter::new(env_directives)
        .add_directive(
            "borg_ui_lib=debug"
                .parse()
                .expect("valid tracing directive"),
        )
        .add_directive("borg_core=debug".parse().expect("valid tracing directive"))
}

pub fn initialize(log_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(log_dir).map_err(|e| e.to_string())?;
    let appender = tracing_appender::rolling::daily(log_dir, "borgui.log");
    let env_directives = std::env::var("RUST_LOG").unwrap_or_default();
    tracing_subscriber::fmt()
        .with_env_filter(log_filter(&env_directives))
        .with_writer(RedactingMakeWriter { inner: appender })
        .try_init()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct BufferWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for BufferWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("buffer not poisoned")
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for BufferWriter {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Emit one debug event per target under the real filter and return what
    /// actually reached the writer.
    fn captured_debug_output(env_directives: &str) -> String {
        let buffer = BufferWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(log_filter(env_directives))
            .with_writer(buffer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(target: "borg_core", "core-debug-marker");
            tracing::debug!(target: "borg_core::borg", "core-module-debug-marker");
            tracing::warn!(target: "borg_core::ssh", "core-warn-marker");
            tracing::debug!(target: "borg_ui_lib", "ui-debug-marker");
            tracing::debug!(target: "some_other_crate", "unrelated-debug-marker");
        });
        let captured = buffer.0.lock().expect("buffer not poisoned").clone();
        String::from_utf8(captured).expect("log output is utf-8")
    }

    /// The actual regression: with no `RUST_LOG`, borg-core's debug/warn events
    /// must reach the log file. This failed before the `borg_core` directive was
    /// added, which is why exported support bundles had empty logs.
    #[test]
    fn borg_core_debug_events_are_logged_without_rust_log() {
        let output = captured_debug_output("");
        assert!(output.contains("core-debug-marker"), "{output}");
        assert!(output.contains("core-module-debug-marker"), "{output}");
        assert!(output.contains("core-warn-marker"), "{output}");
    }

    #[test]
    fn tauri_backend_debug_events_are_logged_without_rust_log() {
        // The crate is `borg_ui_lib`, not `borg_ui` — a rename here silently
        // stops all backend logging, so pin the real name.
        let output = captured_debug_output("");
        assert!(output.contains("ui-debug-marker"), "{output}");
    }

    /// The filter must still be a filter — unrelated crates stay at the
    /// ERROR-only default rather than flooding the user's log file.
    #[test]
    fn unrelated_crates_stay_quiet_without_rust_log() {
        let output = captured_debug_output("");
        assert!(!output.contains("unrelated-debug-marker"), "{output}");
    }

    /// An explicit `RUST_LOG` still composes — our directives are added on top,
    /// so a developer can widen logging without losing borg-core coverage.
    #[test]
    fn explicit_rust_log_directives_are_honoured() {
        let output = captured_debug_output("some_other_crate=debug");
        assert!(output.contains("unrelated-debug-marker"), "{output}");
        assert!(output.contains("core-debug-marker"), "{output}");
    }
}
