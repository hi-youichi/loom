//! Custom event formatter that adds `trace_id` and `span_id` to each log line (plain text).
//!
//! Used by `logging::init()` so file logs can be correlated by trace/span.
//! Interacts with: `tracing_subscriber::fmt::Layer`, `FmtContext`, `FormatEvent`.

use std::fmt;

use tracing_core::Subscriber;
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
use tracing_subscriber::fmt::time::{FormatTime, SystemTime};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::registry::{LookupSpan, SpanRef};

/// Plain-text formatter that prefixes each line with `trace_id` and `span_id` from the current span scope.
///
/// Output format: `TIMESTAMP trace_id=X span_id=Y LEVEL target: event_fields` when the event has a parent span;
/// otherwise `TIMESTAMP LEVEL target: event_fields` (no trace_id/span_id prefix).
pub struct TextWithSpanIds {
    timer: SystemTime,
    with_level: bool,
    with_target: bool,
}

impl Default for TextWithSpanIds {
    fn default() -> Self {
        Self {
            timer: SystemTime::default(),
            with_level: true,
            with_target: true,
        }
    }
}

impl TextWithSpanIds {
    /// Builds a formatter with level and target enabled (same as default fmt layer).
    pub fn new() -> Self {
        Self::default()
    }

    /// Disable level in the output.
    #[allow(dead_code)]
    pub fn with_level(mut self, on: bool) -> Self {
        self.with_level = on;
        self
    }

    /// Disable target (module path) in the output.
    #[allow(dead_code)]
    pub fn with_target(mut self, on: bool) -> Self {
        self.with_target = on;
        self
    }
}

impl<S, N> FormatEvent<S, N> for TextWithSpanIds
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing_core::Event<'_>,
    ) -> fmt::Result {
        self.timer.format_time(&mut writer)?;
        if let Some(span) = ctx.parent_span() {
            let span_id = span.id().into_u64().to_string();
            let trace_id = span
                .scope()
                .from_root()
                .next()
                .map(|root: SpanRef<'_, S>| root.id().into_u64().to_string())
                .unwrap_or_else(|| span_id.clone());
            write!(writer, " trace_id={} span_id={}", trace_id, span_id)?;
        }

        if self.with_level {
            write!(writer, " {}:", event.metadata().level())?;
        }
        if self.with_target {
            write!(writer, " {}:", event.metadata().target())?;
        }
        write!(writer, " ")?;

        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt;

    #[derive(Clone)]
    struct VecWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for VecWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn builder_flags_can_be_toggled() {
        let formatter = TextWithSpanIds::new().with_level(false).with_target(false);
        assert!(!formatter.with_level);
        assert!(!formatter.with_target);
    }

    #[test]
    fn format_event_includes_span_ids_and_fields() {
        let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = {
            let sink = Arc::clone(&sink);
            move || VecWriter(Arc::clone(&sink))
        };

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(TextWithSpanIds::new())
                .with_writer(writer)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("root");
            let _guard = span.enter();
            tracing::info!(k = "v", "hello");
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        assert!(output.contains("trace_id="));
        assert!(output.contains("span_id="));
        assert!(output.contains("INFO"));
        assert!(output.contains("hello"));
        assert!(output.contains("k=\"v\""));
    }
}
