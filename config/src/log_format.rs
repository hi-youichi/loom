//! Custom event formatters for tracing log output.
//!
//! Two formats are provided:
//! - `TextWithSpanIds`: plain-text with `thread_id`, `trace_id`, `span_id` per line
//! - `JsonWithSpanIds`: structured JSON with the same fields

use std::fmt;
use std::thread;

use tracing_core::Subscriber;
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
// FormattedFields is re-exported at the fmt level in newer versions
use tracing_subscriber::fmt::FormattedFields;
use tracing_subscriber::fmt::time::{FormatTime, SystemTime};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::registry::{LookupSpan, SpanRef};

/// Plain-text formatter that prefixes each line with `thread_id`, `trace_id`, and `span_id` from the current span scope.
///
/// Output format: `TIMESTAMP thread_id=TID trace_id=X span_id=Y LEVEL target: event_fields` when the event has a parent span;
/// otherwise `TIMESTAMP thread_id=TID LEVEL target: event_fields` (no trace_id/span_id prefix).
pub struct TextWithSpanIds {
    timer: SystemTime,
    pub(crate) with_level: bool,
    pub(crate) with_target: bool,
    pub(crate) with_module_path: bool,
}

impl Default for TextWithSpanIds {
    fn default() -> Self {
        Self {
            timer: SystemTime,
            with_level: true,
            with_target: true,
            with_module_path: true,
        }
    }
}

impl TextWithSpanIds {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn with_level(mut self, on: bool) -> Self {
        self.with_level = on;
        self
    }

    #[allow(dead_code)]
    pub fn with_target(mut self, on: bool) -> Self {
        self.with_target = on;
        self
    }

    #[allow(dead_code)]
    pub fn with_module_path(mut self, on: bool) -> Self {
        self.with_module_path = on;
        self
    }
}

fn extract_thread_id_from_fields(fields: &str) -> Option<String> {
    let prefix = "thread_id=";
    let start = fields.find(prefix)?;
    let rest = &fields[start + prefix.len()..];
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(stripped[..end].to_string())
    } else {
        let end = rest
            .find(|c: char| c.is_whitespace())
            .unwrap_or(rest.len());
        if end > 0 {
            Some(rest[..end].to_string())
        } else {
            None
        }
    }
}

fn extract_app_thread_id<S, N>(ctx: &FmtContext<'_, S, N>) -> Option<String>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    let current = ctx.lookup_current()?;
    for s in current.scope() {
        if let Some(fields) = s.extensions().get::<FormattedFields<N>>() {
            if !fields.is_empty() {
                let formatted = fields.to_string();
                if let Some(tid) = extract_thread_id_from_fields(&formatted) {
                    return Some(tid);
                }
            }
        }
    }
    None
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

        if let Some(app_tid) = extract_app_thread_id(ctx) {
            write!(writer, " thread_id={}", app_tid)?;
        } else {
            let tid = thread::current().id();
            write!(writer, " thread_id={:?}", tid)?;
        }

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
        if self.with_module_path {
            if let Some(module_path) = event.metadata().module_path() {
                write!(writer, " {}:", module_path)?;
            }
        }
        write!(writer, " ")?;

        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// JSON formatter that emits structured log lines with `thread_id`, `trace_id`, and `span_id`.
///
/// Each event is a single JSON object on one line (NDJSON-friendly).
/// Fields attached to the event are inlined at the top level of the object.
pub struct JsonWithSpanIds {
    timer: SystemTime,
}

impl Default for JsonWithSpanIds {
    fn default() -> Self {
        Self {
            timer: SystemTime,
        }
    }
}

impl JsonWithSpanIds {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }
}

struct JsonFieldsCollector<'a> {
    buffer: &'a mut String,
    first: bool,
}

impl<'a> tracing_subscriber::field::Visit for JsonFieldsCollector<'a> {
    fn record_debug(&mut self, field: &tracing_core::Field, value: &dyn std::fmt::Debug) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer.push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer
            .push_str(&serde_json::to_string(&format!("{:?}", value)).unwrap_or_default());
    }

    fn record_str(&mut self, field: &tracing_core::Field, value: &str) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer.push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer
            .push_str(&serde_json::to_string(value).unwrap_or_default());
    }

    fn record_bool(&mut self, field: &tracing_core::Field, value: bool) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer.push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer
            .push_str(&serde_json::to_string(&value).unwrap_or_default());
    }

    fn record_i64(&mut self, field: &tracing_core::Field, value: i64) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer.push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer.push_str(&value.to_string());
    }

    fn record_u64(&mut self, field: &tracing_core::Field, value: u64) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer.push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer.push_str(&value.to_string());
    }

    fn record_f64(&mut self, field: &tracing_core::Field, value: f64) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer.push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer.push_str(&value.to_string());
    }

    fn record_error(
        &mut self,
        field: &tracing_core::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        self.record_debug(field, &value);
    }
}

impl<S, N> FormatEvent<S, N> for JsonWithSpanIds
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
        let mut timestamp = String::new();
        {
            let mut ts_writer = Writer::new(&mut timestamp);
            self.timer.format_time(&mut ts_writer)?;
        }

        let tid = extract_app_thread_id(ctx)
            .unwrap_or_else(|| format!("{:?}", thread::current().id()));
        let level = event.metadata().level().to_string();
        let target = event.metadata().target().to_string();
        let module_path = event.metadata().module_path().map(|s| s.to_string());

        let (trace_id, span_id) = if let Some(span) = ctx.parent_span() {
            let sid = span.id().into_u64().to_string();
            let trid = span
                .scope()
                .from_root()
                .next()
                .map(|root: SpanRef<'_, S>| root.id().into_u64().to_string())
                .unwrap_or_else(|| sid.clone());
            (Some(trid), Some(sid))
        } else {
            (None, None)
        };

        let mut fields_json = String::new();
        let mut collector = JsonFieldsCollector {
            buffer: &mut fields_json,
            first: true,
        };
        event.record(&mut collector);

        write!(writer, "{{\"timestamp\":{}", serde_json::to_string(&timestamp).unwrap_or_default())?;
        write!(writer, ",\"level\":{}", serde_json::to_string(&level).unwrap_or_default())?;
        write!(writer, ",\"target\":{}", serde_json::to_string(&target).unwrap_or_default())?;
        write!(writer, ",\"thread_id\":{}", serde_json::to_string(&tid).unwrap_or_default())?;
        if let Some(trid) = trace_id {
            write!(writer, ",\"trace_id\":{}", serde_json::to_string(&trid).unwrap_or_default())?;
        }
        if let Some(sid) = span_id {
            write!(writer, ",\"span_id\":{}", serde_json::to_string(&sid).unwrap_or_default())?;
        }
        write!(writer, ",\"module_path\":{}", serde_json::to_string(&module_path).unwrap_or_default())?;
        if !fields_json.is_empty() {
            write!(writer, ",{}", fields_json)?;
        }
        writeln!(writer, "}}")
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
        let formatter = TextWithSpanIds::default()
            .with_level(false)
            .with_target(false);
        assert!(!formatter.with_level);
        assert!(!formatter.with_target);
    }

    #[test]
    fn format_event_includes_thread_trace_span_and_fields() {
        let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = {
            let sink = Arc::clone(&sink);
            move || VecWriter(Arc::clone(&sink))
        };

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(TextWithSpanIds::default())
                .with_writer(writer)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("root");
            let _guard = span.enter();
            tracing::info!(k = "v", "hello");
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        assert!(output.contains("thread_id="), "missing thread_id in: {output}");
        assert!(output.contains("trace_id="), "missing trace_id in: {output}");
        assert!(output.contains("span_id="), "missing span_id in: {output}");
        assert!(output.contains("INFO"), "missing INFO in: {output}");
        assert!(output.contains("hello"), "missing hello in: {output}");
        assert!(output.contains("k=\"v\""), "missing k=\"v\" in: {output}");
    }

    #[test]
    fn json_format_produces_valid_json_with_span() {
        let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = {
            let sink = Arc::clone(&sink);
            move || VecWriter(Arc::clone(&sink))
        };

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(JsonWithSpanIds::default())
                .with_writer(writer)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("root");
            let _guard = span.enter();
            tracing::info!(k = "v", "hello");
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).expect(&format!("not valid JSON: {output}"));
        assert!(parsed.get("timestamp").is_some(), "missing timestamp");
        assert_eq!(parsed["level"], "INFO");
        assert!(parsed.get("target").is_some());
        assert!(parsed.get("thread_id").is_some());
        assert!(parsed.get("trace_id").is_some(), "missing trace_id");
        assert!(parsed.get("span_id").is_some(), "missing span_id");
    }

    #[test]
    fn json_format_without_span_omits_trace_and_span_ids() {
        let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = {
            let sink = Arc::clone(&sink);
            move || VecWriter(Arc::clone(&sink))
        };

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(JsonWithSpanIds::default())
                .with_writer(writer)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("no span event");
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).expect(&format!("not valid JSON: {output}"));
        assert!(parsed.get("trace_id").is_none(), "unexpected trace_id");
        assert!(parsed.get("span_id").is_none(), "unexpected span_id");
    }

    #[test]
    fn format_event_without_span_still_has_thread_id() {
        let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = {
            let sink = Arc::clone(&sink);
            move || VecWriter(Arc::clone(&sink))
        };

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(TextWithSpanIds::default())
                .with_writer(writer)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("no span event");
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        assert!(output.contains("thread_id="), "missing thread_id in: {output}");
        assert!(!output.contains("trace_id="), "unexpected trace_id in: {output}");
        assert!(!output.contains("span_id="), "unexpected span_id in: {output}");
    }
}
