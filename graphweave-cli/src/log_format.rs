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
