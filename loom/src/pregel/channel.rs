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
    Topic { accumulate: bool },
    Tasks,
    BinaryAggregate { reducer: ReducerFn },
}

impl fmt::Debug for ChannelKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LastValue => write!(f, "LastValue"),
            Self::Topic { accumulate } => f
                .debug_struct("Topic")
                .field("accumulate", accumulate)
                .finish(),
            Self::Tasks => write!(f, "Tasks"),
            Self::BinaryAggregate { .. } => write!(f, "BinaryAggregate"),
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

/// Builds a boxed channel instance from a declarative spec.
pub fn build_channel(spec: &ChannelSpec) -> BoxedChannel {
    match &spec.kind {
        ChannelKind::LastValue => Box::new(LastValueChannel::new()),
        ChannelKind::Topic { accumulate } => Box::new(TopicChannel::new(*accumulate)),
        ChannelKind::Tasks => Box::new(TasksChannel::new()),
        ChannelKind::BinaryAggregate { reducer } => {
            Box::new(BinaryAggregateChannel::new(Arc::clone(reducer)))
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
}
