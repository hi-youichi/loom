//! Pregel runtime channel abstractions.

use std::fmt;
use std::sync::Arc;

use crate::types::ChannelValue;

/// Reducer function used by aggregate channels.
pub type ReducerFn =
    Arc<dyn Fn(Option<&ChannelValue>, &[ChannelValue]) -> ChannelValue + Send + Sync>;

/// Reducer function used by binary aggregate channels.
/// Higher-ranked over the input lifetimes so the alias can be used in struct fields.
pub type BinaryAggregateReducer = Arc<
    dyn for<'a, 'b> Fn(Option<&'a ChannelValue>, &'b [ChannelValue]) -> ChannelValue + Send + Sync,
>;

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

    #[test]
    fn test_channel_spec_new() {
        let spec = ChannelSpec::new(ChannelKind::LastValue);
        assert!(matches!(spec.kind, ChannelKind::LastValue));
    }

    #[test]
    fn test_channel_spec_clone() {
        let spec = ChannelSpec::new(ChannelKind::Topic { accumulate: true });
        let cloned = spec.clone();
        assert!(matches!(cloned.kind, ChannelKind::Topic { accumulate: true }));
    }

    #[test]
    fn test_channel_spec_debug() {
        let spec = ChannelSpec::new(ChannelKind::LastValue);
        let debug_str = format!("{:?}", spec);
        assert!(debug_str.contains("ChannelSpec"));
    }

    #[test]
    fn test_channel_kind_variants() {
        let last_value = ChannelKind::LastValue;
        let ephemeral = ChannelKind::Ephemeral;
        let topic = ChannelKind::Topic { accumulate: false };
        let tasks = ChannelKind::Tasks;

        assert!(!matches!(last_value, ChannelKind::Topic { .. }));
        assert!(!matches!(ephemeral, ChannelKind::LastValue));
        assert!(matches!(topic, ChannelKind::Topic { .. }));
        assert!(matches!(tasks, ChannelKind::Tasks));
    }

    #[test]
    fn test_channel_kind_debug() {
        let last_value = ChannelKind::LastValue;
        let debug_str = format!("{:?}", last_value);
        assert!(debug_str.contains("LastValue"));

        let topic = ChannelKind::Topic { accumulate: true };
        let debug_str = format!("{:?}", topic);
        assert!(debug_str.contains("Topic"));
    }

    #[test]
    fn test_build_channel_last_value() {
        let spec = ChannelSpec::new(ChannelKind::LastValue);
        let channel = build_channel(&spec);

        assert_eq!(channel.channel_type(), "LastValueChannel");
        assert!(channel.is_available());
        assert_eq!(channel.snapshot(), serde_json::Value::Null);
    }

    #[test]
    fn test_build_channel_ephemeral() {
        let spec = ChannelSpec::new(ChannelKind::Ephemeral);
        let channel = build_channel(&spec);

        assert_eq!(channel.channel_type(), "EphemeralChannel");
        assert!(channel.is_available());
    }

    #[test]
    fn test_build_channel_topic() {
        let spec = ChannelSpec::new(ChannelKind::Topic { accumulate: true });
        let channel = build_channel(&spec);

        assert_eq!(channel.channel_type(), "TopicChannel");
        assert!(channel.is_available());
    }

    #[test]
    fn test_build_channel_topic_non_accumulate() {
        let spec = ChannelSpec::new(ChannelKind::Topic { accumulate: false });
        let channel = build_channel(&spec);

        assert_eq!(channel.channel_type(), "TopicChannel");
        assert!(channel.is_available());
    }

    #[test]
    fn test_build_channel_tasks() {
        let spec = ChannelSpec::new(ChannelKind::Tasks);
        let channel = build_channel(&spec);

        assert_eq!(channel.channel_type(), "TasksChannel");
        assert!(channel.is_available());
    }

    #[test]
    fn test_build_channel_binary_aggregate() {
        let reducer: BinaryAggregateReducer = Arc::new(
            |old: Option<&ChannelValue>, new: &[ChannelValue]| {
                if let Some(old_value) = old {
                    return old_value.clone();
                }
                if new.is_empty() {
                    return ChannelValue::Null;
                }
                new.last().unwrap().clone()
            }
        );

        let spec = ChannelSpec::new(ChannelKind::BinaryAggregate { reducer });
        let channel = build_channel(&spec);

        assert_eq!(channel.channel_type(), "BinaryAggregateChannel");
        assert!(channel.is_available());
    }

    #[test]
    fn test_build_channel_named_barrier() {
        let spec = ChannelSpec::new(ChannelKind::NamedBarrier {
            expected: vec!["node1".to_string(), "node2".to_string()],
        });
        let channel = build_channel(&spec);

        assert_eq!(channel.channel_type(), "NamedBarrierChannel");
        assert!(channel.is_available());
    }

    #[test]
    fn test_last_value_channel_lifecycle() {
        let mut channel = LastValueChannel::new();

        assert_eq!(channel.snapshot(), serde_json::Value::Null);
        assert!(channel.is_available());

        let changed = channel.update(&[serde_json::json!("value1"), serde_json::json!("value2")]);
        assert!(changed);
        assert_eq!(channel.snapshot(), serde_json::json!("value2"));

        let changed = channel.consume();
        assert!(!changed);

        let changed = channel.finish();
        assert!(changed);
        assert!(!channel.is_available());
    }

    #[test]
    fn test_ephemeral_channel_lifecycle() {
        let mut channel = EphemeralChannel::new();

        assert_eq!(channel.snapshot(), serde_json::Value::Null);

        let changed = channel.update(&[serde_json::json!("value1")]);
        assert!(changed);
        assert_eq!(channel.snapshot(), serde_json::json!("value1"));

        let changed = channel.consume();
        assert!(!changed);
        assert_eq!(channel.snapshot(), serde_json::json!("value1"));

        let changed = channel.consume();
        assert!(changed);
        assert_eq!(channel.snapshot(), serde_json::Value::Null);

        let changed = channel.finish();
        assert!(changed);
    }

    #[test]
    fn test_topic_channel_accumulate() {
        let mut channel = TopicChannel::new(true);

        channel.update(&[serde_json::json!("value1")]);
        channel.update(&[serde_json::json!("value2")]);

        let snapshot = channel.snapshot();
        assert_eq!(snapshot, serde_json::json!(["value1", "value2"]));

        let changed = channel.consume();
        assert!(!changed);

        let snapshot = channel.snapshot();
        assert_eq!(snapshot, serde_json::json!(["value1", "value2"]));
    }

    #[test]
    fn test_topic_channel_no_accumulate() {
        let mut channel = TopicChannel::new(false);

        channel.update(&[serde_json::json!("value1")]);
        channel.update(&[serde_json::json!("value2")]);

        let snapshot = channel.snapshot();
        assert_eq!(snapshot, serde_json::json!(["value2"]));

        let changed = channel.consume();
        assert!(changed);

        let snapshot = channel.snapshot();
        assert_eq!(snapshot, serde_json::json!([]));
    }

    #[test]
    fn test_tasks_channel_lifecycle() {
        let mut channel = TasksChannel::new();

        channel.update(&[serde_json::json!("task1"), serde_json::json!("task2")]);

        let snapshot = channel.snapshot();
        assert_eq!(snapshot, serde_json::json!(["task1", "task2"]));

        let changed = channel.consume();
        assert!(changed);

        let snapshot = channel.snapshot();
        assert_eq!(snapshot, serde_json::json!([]));
    }

    #[test]
    fn test_binary_aggregate_channel() {
        let reducer = Arc::new(|old: Option<&ChannelValue>, new: &[ChannelValue]| {
            let old_count = old.and_then(|v| v.as_i64()).unwrap_or(0);
            let new_count: i64 = new.iter().filter_map(|v| v.as_i64()).sum();
            serde_json::json!(old_count + new_count)
        });

        let mut channel = BinaryAggregateChannel::new(reducer);

        let changed = channel.update(&[serde_json::json!(5)]);
        assert!(changed);
        assert_eq!(channel.snapshot(), serde_json::json!(5));

        let changed = channel.update(&[serde_json::json!(3)]);
        assert!(changed);
        assert_eq!(channel.snapshot(), serde_json::json!(8));

        let changed = channel.consume();
        assert!(!changed);
    }

    #[test]
    fn test_named_barrier_channel() {
        let mut channel = NamedBarrierChannel::new(vec!["node1".to_string(), "node2".to_string()]);

        assert_eq!(channel.snapshot(), serde_json::Value::Null);
        assert!(!channel.barrier_met());

        let changed = channel.update(&[serde_json::json!("node1")]);
        assert!(changed);
        assert_eq!(channel.snapshot(), serde_json::Value::Null);

        let changed = channel.update(&[serde_json::json!("node2")]);
        assert!(changed);
        assert_eq!(channel.snapshot(), serde_json::json!(true));

        let changed = channel.update(&[serde_json::json!("node1")]);
        assert!(!changed);

        let changed = channel.consume();
        assert!(changed);
        assert_eq!(channel.snapshot(), serde_json::Value::Null);
    }

    #[test]
    fn test_channel_update_empty_values() {
        let mut last_value = LastValueChannel::new();
        let changed = last_value.update(&[]);
        assert!(!changed);

        let mut topic = TopicChannel::new(false);
        let changed = topic.update(&[]);
        assert!(!changed);
    }

    #[test]
    fn test_channel_finish_returns_false_when_already_finished() {
        let mut last_value = LastValueChannel::new();
        let changed = last_value.finish();
        assert!(changed);

        let changed = last_value.finish();
        assert!(!changed);
    }

    #[test]
    fn test_channel_consumes_are_noop_when_no_value() {
        let mut last_value = LastValueChannel::new();
        let changed = last_value.consume();
        assert!(!changed);
        assert!(last_value.is_available());
    }
}
