//! Custom event formatters for tracing log output.
//!
//! Two formats are provided:
//! - `TextWithSpanIds`: plain-text with `thread_id`, `root_span_id`, `span_id` per line
//! - `JsonWithSpanIds`: structured JSON with the same fields
//!
//! Naming clarification:
//! - `thread_id` — the session/business identifier (e.g. `session-abc-123`)
//! - `root_span_id` — the tracing span-tree root span's numeric ID (was previously
//!   confusingly named `trace_id`); different from `thread_id`
//! - `span_id` — the current span's numeric ID

use std::fmt;
use std::thread;

use tracing_core::Subscriber;
use tracing_subscriber::fmt::format::{FormatEvent, FormatFields, Writer};
// FormattedFields is re-exported at the fmt level in newer versions
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::fmt::FormattedFields;
use tracing_subscriber::registry::{LookupSpan, SpanRef};

struct LocalTimer;

impl FormatTime for LocalTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(
            w,
            "{}",
            chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z")
        )
    }
}

/// Plain-text formatter that prefixes each line with `thread_id`, `root_span_id`, and `span_id` from the current span scope.
///
/// Output format: `TIMESTAMP thread_id=TID root_span_id=X span_id=Y LEVEL target: event_fields` when the event has a parent span;
/// otherwise `TIMESTAMP thread_id=TID LEVEL target: event_fields` (no root_span_id/span_id prefix).
pub struct TextWithSpanIds {
    timer: LocalTimer,
    pub(crate) with_level: bool,
    pub(crate) with_target: bool,
    pub(crate) with_module_path: bool,
}

impl Default for TextWithSpanIds {
    fn default() -> Self {
        Self {
            timer: LocalTimer,
            with_level: true,
            with_target: true,
            with_module_path: true,
        }
    }
}

impl TextWithSpanIds {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_level(mut self, on: bool) -> Self {
        self.with_level = on;
        self
    }
    pub fn with_target(mut self, on: bool) -> Self {
        self.with_target = on;
        self
    }
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
        let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
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

/// Visitor that looks for a `thread_id` field on an event.
struct ThreadIdVisitor {
    thread_id: Option<String>,
}

impl tracing_core::field::Visit for ThreadIdVisitor {
    fn record_debug(&mut self, field: &tracing_core::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "thread_id" {
            let formatted = format!("{:?}", value);
            // `thread_id = ?Option<String>` yields `Some("x")`; unwrap so logs show the
            // raw id. `None` carries no id — leave the visitor untouched so the
            // formatter falls back to span scope / OS thread id.
            match formatted.as_str() {
                "None" => {}
                _ => self.thread_id = Some(unwrap_option_debug(&formatted)),
            }
        }
    }

    fn record_str(&mut self, field: &tracing_core::Field, value: &str) {
        if field.name() == "thread_id" {
            self.thread_id = Some(value.to_string());
        }
    }
}

/// Extract thread_id from an event's own fields (fallback when no span carries it).
fn extract_thread_id_from_event(event: &tracing_core::Event<'_>) -> Option<String> {
    let mut visitor = ThreadIdVisitor { thread_id: None };
    event.record(&mut visitor);
    visitor.thread_id
}

/// Unwraps the `Some(...)`/`"..."` Debug wrappers produced by
/// `thread_id = ?Option<String>` event fields: `Some("x")` -> `x`.
/// Strings that are not Option-wrapped are returned unchanged.
fn unwrap_option_debug(formatted: &str) -> String {
    let inner = formatted
        .strip_prefix("Some(")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(formatted)
        .trim();
    inner
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(inner)
        .to_string()
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

        if let Some(event_tid) = extract_thread_id_from_event(event) {
            write!(writer, " thread_id={}", event_tid)?;
        } else if let Some(app_tid) = extract_app_thread_id(ctx) {
            write!(writer, " thread_id={}", app_tid)?;
        } else {
            let tid = thread::current().id();
            write!(writer, " thread_id={:?}", tid)?;
        }

        if let Some(span) = ctx.parent_span() {
            let span_id = span.id().into_u64().to_string();
            let root_span_id = span
                .scope()
                .from_root()
                .next()
                .map(|root: SpanRef<'_, S>| root.id().into_u64().to_string())
                .unwrap_or_else(|| span_id.clone());
            write!(writer, " root_span_id={} span_id={}", root_span_id, span_id)?;
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

/// JSON formatter that emits structured log lines with `thread_id`, `root_span_id`, and `span_id`.
///
/// Each event is a single JSON object on one line (NDJSON-friendly).
/// Fields attached to the event are inlined at the top level of the object.
pub struct JsonWithSpanIds {
    timer: LocalTimer,
}

impl Default for JsonWithSpanIds {
    fn default() -> Self {
        Self { timer: LocalTimer }
    }
}

impl JsonWithSpanIds {
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
        self.buffer
            .push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer
            .push_str(&serde_json::to_string(&format!("{:?}", value)).unwrap_or_default());
    }

    fn record_str(&mut self, field: &tracing_core::Field, value: &str) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer
            .push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer
            .push_str(&serde_json::to_string(value).unwrap_or_default());
    }

    fn record_bool(&mut self, field: &tracing_core::Field, value: bool) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer
            .push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer
            .push_str(&serde_json::to_string(&value).unwrap_or_default());
    }

    fn record_i64(&mut self, field: &tracing_core::Field, value: i64) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer
            .push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer.push_str(&value.to_string());
    }

    fn record_u64(&mut self, field: &tracing_core::Field, value: u64) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer
            .push_str(&serde_json::to_string(field.name()).unwrap_or_default());
        self.buffer.push(':');
        self.buffer.push_str(&value.to_string());
    }

    fn record_f64(&mut self, field: &tracing_core::Field, value: f64) {
        if !self.first {
            self.buffer.push(',');
        }
        self.first = false;
        self.buffer
            .push_str(&serde_json::to_string(field.name()).unwrap_or_default());
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
            .or_else(|| extract_thread_id_from_event(event))
            .unwrap_or_else(|| format!("{:?}", thread::current().id()));
        let level = event.metadata().level().to_string();
        let target = event.metadata().target().to_string();
        let module_path = event.metadata().module_path().map(|s| s.to_string());

        let (root_span_id, span_id) = if let Some(span) = ctx.parent_span() {
            let sid = span.id().into_u64().to_string();
            let rsid = span
                .scope()
                .from_root()
                .next()
                .map(|root: SpanRef<'_, S>| root.id().into_u64().to_string())
                .unwrap_or_else(|| sid.clone());
            (Some(rsid), Some(sid))
        } else {
            (None, None)
        };

        let mut fields_json = String::new();
        let mut collector = JsonFieldsCollector {
            buffer: &mut fields_json,
            first: true,
        };
        event.record(&mut collector);

        write!(
            writer,
            "{{\"timestamp\":{}",
            serde_json::to_string(&timestamp).unwrap_or_default()
        )?;
        write!(
            writer,
            ",\"level\":{}",
            serde_json::to_string(&level).unwrap_or_default()
        )?;
        write!(
            writer,
            ",\"target\":{}",
            serde_json::to_string(&target).unwrap_or_default()
        )?;
        write!(
            writer,
            ",\"thread_id\":{}",
            serde_json::to_string(&tid).unwrap_or_default()
        )?;
        if let Some(rsid) = root_span_id {
            write!(
                writer,
                ",\"root_span_id\":{}",
                serde_json::to_string(&rsid).unwrap_or_default()
            )?;
        }
        if let Some(sid) = span_id {
            write!(
                writer,
                ",\"span_id\":{}",
                serde_json::to_string(&sid).unwrap_or_default()
            )?;
        }
        write!(
            writer,
            ",\"module_path\":{}",
            serde_json::to_string(&module_path).unwrap_or_default()
        )?;
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
    fn root_span_thread_id_field_is_picked_up_by_formatter() {
        // Regression test: when a parent span declares a `thread_id` field with
        // a business identifier (e.g. `session-1717...`), the formatter must
        // surface that exact value in the `thread_id=` prefix — NOT fall back
        // to the OS `ThreadId(N)` Debug format.
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
            let span = tracing::info_span!("root", thread_id = "session-1717-test");
            let _guard = span.enter();
            tracing::info!("hello");
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        assert!(
            output.contains("thread_id=session-1717-test"),
            "expected business thread_id in output, got: {output}"
        );
        assert!(
            !output.contains("ThreadId("),
            "formatter fell back to OS ThreadId instead of using span field, output: {output}"
        );
    }

    #[test]
    fn event_thread_id_field_overrides_span_thread_id() {
        // Mirror the previous commit's `extract_thread_id_from_event` fallback:
        // when the event itself carries a `thread_id` field, that value should
        // win over the parent span's field.
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
            let span = tracing::info_span!("root", thread_id = "session-span");
            let _guard = span.enter();
            tracing::info!(thread_id = "event-direct", "override");
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        assert!(
            output.contains("thread_id=event-direct"),
            "event thread_id field should win over span field, got: {output}"
        );
    }

    /// Regression test: `thread_id = ?Option<String>` (Debug format) must surface
    /// the unwrapped id (`session-...`), not `Some("session-...")`, and a `None`
    /// value must not hijack the fallback chain.
    #[test]
    fn event_option_thread_id_is_unwrapped() {
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
            let tid: Option<String> = Some("session-opt-789".to_string());
            tracing::debug!(thread_id = ?tid, "option field");
            let none_tid: Option<String> = None;
            tracing::debug!(thread_id = ?none_tid, "none field");
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        assert!(
            output.contains("thread_id=session-opt-789"),
            "Some(..) must be unwrapped, got: {output}"
        );
        for line in output.lines() {
            // Timestamp contains no space, so the segment after the first space
            // is the thread_id prefix — it must never leak `Some(`.
            let after_ts = line.split_once(' ').map(|(_, rest)| rest).unwrap_or("");
            assert!(
                !after_ts.starts_with("thread_id=Some(") && !after_ts.starts_with("thread_id=None"),
                "thread_id prefix must not be Option-wrapped, got: {line}"
            );
        }
    }

    #[test]
    fn json_format_propagates_span_thread_id() {
        // JSON path parity: the same span-field lookup must feed the JSON
        // `thread_id` key, not the OS ThreadId Debug value.
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
            let span = tracing::info_span!("root", thread_id = "session-json-test");
            let _guard = span.enter();
            tracing::info!("json hello");
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim())
            .unwrap_or_else(|e| panic!("not valid JSON: {output}: {e}"));
        assert_eq!(
            parsed["thread_id"], "session-json-test",
            "JSON thread_id should equal span field value, parsed: {parsed}"
        );
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
        assert!(
            output.contains("thread_id="),
            "missing thread_id in: {output}"
        );
        assert!(
            output.contains("root_span_id="),
            "missing root_span_id in: {output}"
        );
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
        let parsed: serde_json::Value = serde_json::from_str(output.trim())
            .unwrap_or_else(|e| panic!("not valid JSON: {output}: {e}"));
        assert!(parsed.get("timestamp").is_some(), "missing timestamp");
        assert_eq!(parsed["level"], "INFO");
        assert!(parsed.get("target").is_some());
        assert!(parsed.get("thread_id").is_some());
        assert!(parsed.get("root_span_id").is_some(), "missing root_span_id");
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
        let parsed: serde_json::Value = serde_json::from_str(output.trim())
            .unwrap_or_else(|e| panic!("not valid JSON: {output}: {e}"));
        assert!(
            parsed.get("root_span_id").is_none(),
            "unexpected root_span_id"
        );
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
        assert!(
            output.contains("thread_id="),
            "missing thread_id in: {output}"
        );
        assert!(
            !output.contains("root_span_id="),
            "unexpected root_span_id in: {output}"
        );
        assert!(
            !output.contains("span_id="),
            "unexpected span_id in: {output}"
        );
    }

    /// Regression test for thread_id inheritance from a parent span's fields.
    ///
    /// Mirrors the production pattern used in `cli/src/run/agent.rs::run_agent_wrapper`
    /// and `anureo-acp/src/agent.rs::prompt`, where the root span carries the business
    /// `thread_id` (e.g. `session-1717...`) so every nested event inherits it via
    /// the parent-scope fallback in `extract_app_thread_id`. Without this, events
    /// that don't explicitly inject `thread_id` would fall back to `ThreadId(N)`.
    #[test]
    fn format_event_inherits_thread_id_from_parent_span_field() {
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
            // Parent span carries the business thread_id (root-span pattern).
            let parent = tracing::info_span!("cli_run", thread_id = "session-abc-123");
            let _guard = parent.enter();
            // Child event does NOT specify thread_id.
            tracing::info!("no explicit thread_id");
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        assert!(
            output.contains("thread_id=session-abc-123"),
            "expected inherited thread_id, got: {output}"
        );
        assert!(
            !output.contains("ThreadId("),
            "should not fall back to ThreadId when parent has thread_id, got: {output}"
        );
    }

    /// Regression test for the event-field fallback in `extract_thread_id_from_event`.
    ///
    /// Validates the second level of the three-level fallback chain: when neither
    /// the parent scope nor the OS thread id is appropriate, an explicit
    /// `thread_id` field on the event itself is used.
    #[test]
    fn format_event_thread_id_from_event_field_when_no_parent_span() {
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
            // No parent span; thread_id comes from the event itself.
            tracing::info!(
                thread_id = "worker-event-id-456",
                "event with thread_id field"
            );
        });

        let output = String::from_utf8(sink.lock().unwrap().clone()).unwrap();
        assert!(
            output.contains("thread_id=worker-event-id-456"),
            "expected event thread_id, got: {output}"
        );
        assert!(
            !output.contains("ThreadId("),
            "should not fall back to ThreadId when event has thread_id, got: {output}"
        );
    }
}
