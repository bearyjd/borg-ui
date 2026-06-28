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

pub fn initialize(log_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(log_dir).map_err(|e| e.to_string())?;
    let appender = tracing_appender::rolling::daily(log_dir, "borgui.log");
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("borg_ui=debug".parse().expect("valid tracing directive")),
        )
        .with_writer(RedactingMakeWriter { inner: appender })
        .try_init()
        .map_err(|e| e.to_string())
}
