//! Pregel runtime channel abstractions.

use std::fmt;
use std::sync::Arc;

use crate::pregel::types::ChannelValue;

/// Reducer function used by aggregate channels.
pub type ReducerFn =
    Arc<dyn Fn(Option<&ChannelValue>, &[ChannelValue]) -> ChannelValue + Send + Sync>;

/// Pregel runtime channel contract.
pub trait Channel: Send + Sync + fmt::Debug {
    /// Returns the current channel snapshot.
    fn snapshot(&self) -> ChannelValue;

    /// Applies pending updates and returns whether the channel changed.
    fn update(&mut self, values: &[ChannelValue]) -> bool;

    /// Marks the current value as consumed by the step.
    fn consume(&mut self) -> bool;

    /// Marks the channel as finished and returns whether its availability changed.
    fn finish(&mut self) -> bool;

    /// Returns whether this channel can still participate in scheduling.
    fn is_available(&self) -> bool;

    /// Returns the channel type name for debugging.
    fn channel_type(&self) -> &'static str;
}

/// Boxed runtime channel.
pub type BoxedChannel = Box<dyn Channel>;

/// Declarative channel spec stored on a graph definition.
#[derive(Clone)]
pub struct ChannelSpec {
    pub kind: ChannelKind,
}

impl fmt::Debug for ChannelSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelSpec")
            .field("kind", &self.kind)
            .finish()
    }
}

/// Supported built-in channel kinds.
#[derive(Clone)]
pub enum ChannelKind {
    LastValue,
    /// Value is cleared after each step (read-once semantics).
    Ephemeral,
    Topic {
        accumulate: bool,
    },
    Tasks,
    BinaryAggregate {
        reducer: ReducerFn,
    },
    /// Synchronization barrier: becomes available only after all `expected`
    /// names have been written. Resets after consumption.
    NamedBarrier {
        expected: Vec<String>,
    },
}

impl fmt::Debug for ChannelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LastValue => write!(f, "LastValue"),
            Self::Ephemeral => write!(f, "Ephemeral"),
            Self::Topic { accumulate } => f
                .debug_struct("Topic")
                .field("accumulate", accumulate)
                .finish(),
            Self::Tasks => write!(f, "Tasks"),
            Self::BinaryAggregate { .. } => write!(f, "BinaryAggregate"),
            Self::NamedBarrier { expected } => f
                .debug_struct("NamedBarrier")
                .field("expected", expected)
                .finish(),
        }
    }
}

impl ChannelSpec {
    /// Creates a new channel spec.
    pub fn new(kind: ChannelKind) -> Self {
        Self { kind }
    }
}

/// Channel that retains only the latest written value.
#[derive(Debug, Default, Clone)]
pub struct LastValueChannel {
    value: Option<ChannelValue>,
    available: bool,
}

impl LastValueChannel {
    pub fn new() -> Self {
        Self {
            value: None,
            available: true,
        }
    }
}

impl Channel for LastValueChannel {
    fn snapshot(&self) -> ChannelValue {
        self.value.clone().unwrap_or(ChannelValue::Null)
    }

    fn update(&mut self, values: &[ChannelValue]) -> bool {
        let Some(last) = values.last() else {
            return false;
        };
        let changed = self.value.as_ref() != Some(last);
        self.value = Some(last.clone());
        changed
    }

    fn consume(&mut self) -> bool {
        false
    }

    fn finish(&mut self) -> bool {
        let changed = self.available;
        self.available = false;
        changed
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn channel_type(&self) -> &'static str {
        "LastValueChannel"
    }
}

/// Channel that retains a value for exactly one step after it was written,
/// then clears it on the next `consume()`. This gives downstream tasks one
/// step to read the value before it disappears.
///
/// Phase 1 (write step): `update()` sets value, `consume()` marks "pending clear".
/// Phase 2 (next step): downstream reads snapshot, `consume()` actually clears.
#[derive(Debug, Default, Clone)]
pub struct EphemeralChannel {
    value: Option<ChannelValue>,
    pending_clear: bool,
    available: bool,
}

impl EphemeralChannel {
    pub fn new() -> Self {
        Self {
            value: None,
            pending_clear: false,
            available: true,
        }
    }
}

impl Channel for EphemeralChannel {
    fn snapshot(&self) -> ChannelValue {
        self.value.clone().unwrap_or(ChannelValue::Null)
    }

    fn update(&mut self, values: &[ChannelValue]) -> bool {
        let Some(last) = values.last() else {
            return false;
        };
        let changed = self.value.as_ref() != Some(last);
        self.value = Some(last.clone());
        self.pending_clear = false;
        changed
    }

    fn consume(&mut self) -> bool {
        if self.pending_clear {
            if self.value.is_some() {
                self.value = None;
                self.pending_clear = false;
                return true;
            }
            self.pending_clear = false;
            return false;
        }
        if self.value.is_some() {
            self.pending_clear = true;
        }
        false
    }

    fn finish(&mut self) -> bool {
        let changed = self.available;
        self.available = false;
        changed
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn channel_type(&self) -> &'static str {
        "EphemeralChannel"
    }
}

/// Channel that stores a list of values, optionally accumulating across steps.
#[derive(Debug, Clone)]
pub struct TopicChannel {
    values: Vec<ChannelValue>,
    accumulate: bool,
    available: bool,
}

impl TopicChannel {
    pub fn new(accumulate: bool) -> Self {
        Self {
            values: Vec::new(),
            accumulate,
            available: true,
        }
    }
}

/// Specialized mailbox channel used for task packets.
#[derive(Debug, Clone, Default)]
pub struct TasksChannel {
    values: Vec<ChannelValue>,
    available: bool,
}

impl TasksChannel {
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            available: true,
        }
    }
}

impl Channel for TopicChannel {
    fn snapshot(&self) -> ChannelValue {
        ChannelValue::Array(self.values.clone())
    }

    fn update(&mut self, values: &[ChannelValue]) -> bool {
        if values.is_empty() {
            return false;
        }
        if self.accumulate {
            self.values.extend(values.iter().cloned());
        } else {
            self.values = values.to_vec();
        }
        true
    }

    fn consume(&mut self) -> bool {
        if self.accumulate || self.values.is_empty() {
            return false;
        }
        self.values.clear();
        true
    }

    fn finish(&mut self) -> bool {
        let changed = self.available;
        self.available = false;
        changed
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn channel_type(&self) -> &'static str {
        "TopicChannel"
    }
}

impl Channel for TasksChannel {
    fn snapshot(&self) -> ChannelValue {
        ChannelValue::Array(self.values.clone())
    }

    fn update(&mut self, values: &[ChannelValue]) -> bool {
        if values.is_empty() {
            return false;
        }
        self.values.extend(values.iter().cloned());
        true
    }

    fn consume(&mut self) -> bool {
        if self.values.is_empty() {
            return false;
        }
        self.values.clear();
        true
    }

    fn finish(&mut self) -> bool {
        let changed = self.available;
        self.available = false;
        changed
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn channel_type(&self) -> &'static str {
        "TasksChannel"
    }
}

/// Channel that aggregates updates through a reducer function.
#[derive(Clone)]
pub struct BinaryAggregateChannel {
    value: Option<ChannelValue>,
    reducer: ReducerFn,
    available: bool,
}

impl fmt::Debug for BinaryAggregateChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BinaryAggregateChannel")
            .field("value", &self.value)
            .field("available", &self.available)
            .finish()
    }
}

impl BinaryAggregateChannel {
    pub fn new(reducer: ReducerFn) -> Self {
        Self {
            value: None,
            reducer,
            available: true,
        }
    }
}

impl Channel for BinaryAggregateChannel {
    fn snapshot(&self) -> ChannelValue {
        self.value.clone().unwrap_or(ChannelValue::Null)
    }

    fn update(&mut self, values: &[ChannelValue]) -> bool {
        if values.is_empty() {
            return false;
        }
        self.value = Some((self.reducer)(self.value.as_ref(), values));
        true
    }

    fn consume(&mut self) -> bool {
        false
    }

    fn finish(&mut self) -> bool {
        let changed = self.available;
        self.available = false;
        changed
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn channel_type(&self) -> &'static str {
        "BinaryAggregateChannel"
    }
}

/// Synchronization barrier channel: becomes available only after all
/// expected names have been written. Consuming resets the seen set.
#[derive(Debug, Clone)]
pub struct NamedBarrierChannel {
    expected: std::collections::HashSet<String>,
    seen: std::collections::HashSet<String>,
    available: bool,
}

impl NamedBarrierChannel {
    pub fn new(expected: impl IntoIterator<Item = String>) -> Self {
        Self {
            expected: expected.into_iter().collect(),
            seen: std::collections::HashSet::new(),
            available: true,
        }
    }

    fn barrier_met(&self) -> bool {
        self.expected.iter().all(|name| self.seen.contains(name))
    }
}

impl Channel for NamedBarrierChannel {
    fn snapshot(&self) -> ChannelValue {
        if self.barrier_met() {
            ChannelValue::Bool(true)
        } else {
            ChannelValue::Null
        }
    }

    fn update(&mut self, values: &[ChannelValue]) -> bool {
        let mut changed = false;
        for value in values {
            if let Some(name) = value.as_str() {
                if self.expected.contains(name) && self.seen.insert(name.to_string()) {
                    changed = true;
                }
            }
        }
        changed
    }

    fn consume(&mut self) -> bool {
        if !self.barrier_met() || self.seen.is_empty() {
            return false;
        }
        self.seen.clear();
        true
    }

    fn finish(&mut self) -> bool {
        let changed = self.available;
        self.available = false;
        changed
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn channel_type(&self) -> &'static str {
        "NamedBarrierChannel"
    }
}

/// Builds a boxed channel instance from a declarative spec.
pub fn build_channel(spec: &ChannelSpec) -> BoxedChannel {
    match &spec.kind {
        ChannelKind::LastValue => Box::new(LastValueChannel::new()),
        ChannelKind::Ephemeral => Box::new(EphemeralChannel::new()),
        ChannelKind::Topic { accumulate } => Box::new(TopicChannel::new(*accumulate)),
        ChannelKind::Tasks => Box::new(TasksChannel::new()),
        ChannelKind::BinaryAggregate { reducer } => {
            Box::new(BinaryAggregateChannel::new(Arc::clone(reducer)))
        }
        ChannelKind::NamedBarrier { expected } => {
            Box::new(NamedBarrierChannel::new(expected.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn last_value_channel_keeps_latest_value() {
        let mut channel = LastValueChannel::new();
        assert!(channel.update(&[json!(1), json!(2)]));
        assert_eq!(channel.snapshot(), json!(2));
    }

    #[test]
    fn topic_channel_accumulates_when_enabled() {
        let mut channel = TopicChannel::new(true);
        assert!(channel.update(&[json!("a")]));
        assert!(channel.update(&[json!("b")]));
        assert_eq!(channel.snapshot(), json!(["a", "b"]));
    }

    #[test]
    fn tasks_channel_uses_reserved_name_with_spec_builder() {
        let spec = ChannelSpec::new(ChannelKind::Tasks);
        let mut channel = build_channel(&spec);
        assert_eq!(channel.channel_type(), "TasksChannel");
        assert!(channel.update(&[json!({"target": "n1"})]));
        assert_eq!(channel.snapshot(), json!([{"target": "n1"}]));
        assert!(channel.consume());
        assert_eq!(channel.snapshot(), json!([]));
        assert_eq!(crate::pregel::types::TASKS_CHANNEL, "__tasks__");
    }

    #[test]
    fn binary_aggregate_channel_reduces_values() {
        let reducer: ReducerFn = Arc::new(|current, updates| {
            let base = current.and_then(|v| v.as_i64()).unwrap_or(0);
            let sum = updates.iter().filter_map(|v| v.as_i64()).sum::<i64>();
            json!(base + sum)
        });
        let mut channel = BinaryAggregateChannel::new(reducer);
        assert!(channel.update(&[json!(1), json!(2)]));
        assert!(channel.update(&[json!(3)]));
        assert_eq!(channel.snapshot(), json!(6));
    }

    #[test]
    fn ephemeral_channel_clears_after_two_phase_consume() {
        let mut ch = EphemeralChannel::new();
        assert_eq!(ch.snapshot(), json!(null));

        assert!(ch.update(&[json!("temp")]));
        assert_eq!(ch.snapshot(), json!("temp"));

        assert!(
            !ch.consume(),
            "first consume marks pending, does not clear yet"
        );
        assert_eq!(ch.snapshot(), json!("temp"), "value survives one consume");

        assert!(ch.consume(), "second consume actually clears");
        assert_eq!(ch.snapshot(), json!(null));

        assert!(!ch.consume(), "third consume is no-op");
    }

    #[test]
    fn ephemeral_channel_keeps_last_value_before_consume() {
        let mut ch = EphemeralChannel::new();
        assert!(ch.update(&[json!(1), json!(2), json!(3)]));
        assert_eq!(ch.snapshot(), json!(3));
    }

    #[test]
    fn ephemeral_channel_build_from_spec() {
        let spec = ChannelSpec::new(ChannelKind::Ephemeral);
        let mut ch = build_channel(&spec);
        assert_eq!(ch.channel_type(), "EphemeralChannel");
        assert!(ch.update(&[json!("x")]));
        assert_eq!(ch.snapshot(), json!("x"));
        ch.consume();
        assert_eq!(ch.snapshot(), json!("x"), "value survives first consume");
        assert!(ch.consume());
        assert_eq!(ch.snapshot(), json!(null), "cleared after second consume");
    }

    #[test]
    fn named_barrier_channel_available_after_all_names_written() {
        let mut ch = NamedBarrierChannel::new(["a".to_string(), "b".to_string()]);
        assert_eq!(ch.snapshot(), json!(null));

        assert!(ch.update(&[json!("a")]));
        assert_eq!(ch.snapshot(), json!(null), "barrier not yet met");

        assert!(ch.update(&[json!("b")]));
        assert_eq!(ch.snapshot(), json!(true), "barrier met");
    }

    #[test]
    fn named_barrier_channel_ignores_unknown_names() {
        let mut ch = NamedBarrierChannel::new(["x".to_string()]);
        assert!(!ch.update(&[json!("unknown")]));
        assert_eq!(ch.snapshot(), json!(null));
    }

    #[test]
    fn named_barrier_channel_consume_resets() {
        let mut ch = NamedBarrierChannel::new(["done".to_string()]);
        assert!(ch.update(&[json!("done")]));
        assert_eq!(ch.snapshot(), json!(true));

        assert!(ch.consume());
        assert_eq!(ch.snapshot(), json!(null), "barrier resets after consume");

        assert!(ch.update(&[json!("done")]));
        assert_eq!(ch.snapshot(), json!(true), "can re-satisfy after reset");
    }

    #[test]
    fn named_barrier_channel_consume_noop_when_not_met() {
        let mut ch = NamedBarrierChannel::new(["a".to_string(), "b".to_string()]);
        ch.update(&[json!("a")]);
        assert!(!ch.consume(), "consume returns false when barrier not met");
    }

    #[test]
    fn named_barrier_channel_build_from_spec() {
        let spec = ChannelSpec::new(ChannelKind::NamedBarrier {
            expected: vec!["s1".to_string(), "s2".to_string()],
        });
        let mut ch = build_channel(&spec);
        assert_eq!(ch.channel_type(), "NamedBarrierChannel");
        ch.update(&[json!("s1"), json!("s2")]);
        assert_eq!(ch.snapshot(), json!(true));
    }
}
