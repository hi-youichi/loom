//! Checkpoint and metadata types.
//!
//! Checkpoint (id, ts, channel_values, channel_versions, metadata).

use super::config::RunnableConfig;
use super::uuid6::uuid6;
use serde_json::Value;
use std::collections::HashMap;
use std::time::SystemTime;

/// Current version of checkpoint format.
pub const CHECKPOINT_VERSION: u32 = 2;

/// Special write type for errors.
pub const ERROR: &str = "__error__";
/// Special write type for scheduled tasks.
pub const SCHEDULED: &str = "__scheduled__";
/// Special write type for interrupts.
pub const INTERRUPT: &str = "__interrupt__";
/// Special write type for resume operations.
pub const RESUME: &str = "__resume__";

/// Mapping from special write types to negative indices.
/// Used by Checkpointer implementations in put_writes.
pub fn writes_idx_map(write_type: &str) -> Option<i32> {
    match write_type {
        ERROR => Some(-1),
        SCHEDULED => Some(-2),
        INTERRUPT => Some(-3),
        RESUME => Some(-4),
        _ => None,
    }
}

/// A pending write to be stored with a checkpoint.
///
/// Tuple of (task_id, channel, value).
pub type PendingWrite = (String, String, Value);

/// Channel versions map (channel name -> version).
///
/// Versions can be string, integer, or float but we use String for simplicity.
pub type ChannelVersions = HashMap<String, String>;

/// Metadata for a single checkpoint (source, step, created_at, parents).
///
/// Checkpoint metadata. Used by Checkpointer implementations
/// and by list() for time-travel UI.
#[derive(Debug, Clone, Default)]
pub struct CheckpointMetadata {
    /// The source of the checkpoint (input, loop, update, fork).
    pub source: CheckpointSource,
    /// The step number of the checkpoint (-1 for input, 0 for first loop, etc.).
    pub step: i64,
    /// Timestamp when this checkpoint was created.
    pub created_at: Option<std::time::SystemTime>,
    /// Parent checkpoint IDs (checkpoint_ns -> checkpoint_id).
    pub parents: HashMap<String, String>,
}

/// Source of the checkpoint (input, loop, update, fork).
///
/// Checkpoint metadata.source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CheckpointSource {
    /// Created from an input to invoke/stream/batch.
    #[default]
    Input,
    /// Created from inside the pregel loop.
    Loop,
    /// Created from a manual state update.
    Update,
    /// Created as a copy of another checkpoint.
    Fork,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Scenario**: All CheckpointSource variants are Debug/Clone and can be used in metadata.
    #[test]
    fn checkpoint_source_all_variants() {
        let _ = CheckpointSource::Input;
        let _ = CheckpointSource::Loop;
        let _ = CheckpointSource::Update;
        let _ = CheckpointSource::Fork;
        let s = CheckpointSource::Input;
        let _ = format!("{:?}", s);
        let c = s.clone();
        let _ = CheckpointMetadata {
            source: c,
            step: 0,
            created_at: None,
            parents: HashMap::new(),
        };
    }

    /// **Scenario**: Checkpoint from_state generates UUID6 ID.
    #[test]
    fn checkpoint_from_state_uuid6_id() {
        let checkpoint: Checkpoint<String> =
            Checkpoint::from_state("test state".to_string(), CheckpointSource::Loop, 1);

        // UUID6 format: 8-4-4-4-12 (36 chars with hyphens)
        let parts: Vec<&str> = checkpoint.id.split('-').collect();
        assert_eq!(
            parts.len(),
            5,
            "UUID should have 5 parts separated by hyphens"
        );
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);

        // Version should be '6' at the start of the third part
        assert!(parts[2].starts_with('6'), "UUID version should be 6");

        // New fields should have defaults
        assert_eq!(checkpoint.v, CHECKPOINT_VERSION);
        assert!(checkpoint.versions_seen.is_empty());
        assert!(checkpoint.pending_sends.is_empty());
        assert!(checkpoint.updated_channels.is_none());
    }

    /// **Scenario**: Multiple checkpoints have unique IDs.
    #[test]
    fn checkpoint_unique_ids() {
        let cp1: Checkpoint<i32> = Checkpoint::from_state(1, CheckpointSource::Input, -1);
        let cp2: Checkpoint<i32> = Checkpoint::from_state(2, CheckpointSource::Loop, 0);
        let cp3: Checkpoint<i32> = Checkpoint::from_state(3, CheckpointSource::Loop, 1);

        assert_ne!(cp1.id, cp2.id);
        assert_ne!(cp2.id, cp3.id);
        assert_ne!(cp1.id, cp3.id);
    }

    /// **Scenario**: Checkpoint with_id allows custom ID.
    #[test]
    fn checkpoint_with_custom_id() {
        let custom_id = "custom-checkpoint-id";
        let checkpoint: Checkpoint<String> = Checkpoint::with_id(
            custom_id.to_string(),
            "state".to_string(),
            CheckpointSource::Fork,
            5,
        );

        assert_eq!(checkpoint.id, custom_id);
        assert_eq!(checkpoint.metadata.step, 5);
        assert_eq!(checkpoint.v, CHECKPOINT_VERSION);
    }

    /// **Scenario**: Checkpoint copy creates a deep clone.
    #[test]
    fn checkpoint_copy_creates_deep_clone() {
        let mut original: Checkpoint<String> =
            Checkpoint::from_state("state".to_string(), CheckpointSource::Loop, 1);
        original
            .channel_versions
            .insert("ch1".to_string(), "1".to_string());
        original.versions_seen.insert(
            "node1".to_string(),
            [("ch1".to_string(), "1".to_string())].into_iter().collect(),
        );

        let copied = original.copy();

        assert_eq!(original.id, copied.id);
        assert_eq!(original.channel_versions, copied.channel_versions);
        assert_eq!(original.versions_seen, copied.versions_seen);
    }

    /// **Scenario**: Default checkpoint has expected values.
    #[test]
    fn checkpoint_default_has_expected_values() {
        let checkpoint: Checkpoint<i32> = Checkpoint::default();

        assert_eq!(checkpoint.v, CHECKPOINT_VERSION);
        assert_eq!(checkpoint.channel_values, 0);
        assert!(checkpoint.channel_versions.is_empty());
        assert!(checkpoint.versions_seen.is_empty());
        assert!(checkpoint.pending_sends.is_empty());
        assert!(checkpoint.updated_channels.is_none());
        assert_eq!(checkpoint.metadata.source, CheckpointSource::Input);
    }

    /// **Scenario**: CheckpointTuple holds all expected fields.
    #[test]
    fn checkpoint_tuple_holds_all_fields() {
        let checkpoint: Checkpoint<String> =
            Checkpoint::from_state("state".to_string(), CheckpointSource::Loop, 1);
        let config = RunnableConfig::default();
        let metadata = checkpoint.metadata.clone();

        let tuple = CheckpointTuple {
            config: config.clone(),
            checkpoint,
            metadata,
            parent_config: Some(config),
            pending_writes: Some(vec![(
                "task1".to_string(),
                "channel".to_string(),
                Value::Null,
            )]),
        };

        assert!(tuple.parent_config.is_some());
        assert_eq!(tuple.pending_writes.as_ref().unwrap().len(), 1);
    }

    /// **Scenario**: writes_idx_map returns correct indices for special types.
    #[test]
    fn writes_idx_map_returns_correct_indices() {
        assert_eq!(writes_idx_map(ERROR), Some(-1));
        assert_eq!(writes_idx_map(SCHEDULED), Some(-2));
        assert_eq!(writes_idx_map(INTERRUPT), Some(-3));
        assert_eq!(writes_idx_map(RESUME), Some(-4));
        assert_eq!(writes_idx_map("regular"), None);
    }

    /// **Scenario**: CheckpointMetadata has correct defaults.
    #[test]
    fn checkpoint_metadata_default() {
        let metadata = CheckpointMetadata::default();

        assert_eq!(metadata.source, CheckpointSource::Input);
        assert_eq!(metadata.step, 0);
        assert!(metadata.created_at.is_none());
        assert!(metadata.parents.is_empty());
    }
}

/// One checkpoint: state snapshot + channel versions + id/ts.
///
/// Stored by Checkpointer keyed by (thread_id, checkpoint_ns, checkpoint_id).
/// channel_values is the graph state S; channel_versions used for reducer/merge.
///
/// **Interaction**: Produced by graph execution; consumed by Checkpointer::put,
/// returned by get_tuple.
#[derive(Debug, Clone)]
pub struct Checkpoint<S> {
    /// The version of the checkpoint format. Currently `2`.
    pub v: u32,
    /// The ID of the checkpoint. Unique and monotonically increasing.
    pub id: String,
    /// The timestamp of the checkpoint in ISO 8601 format (milliseconds since epoch).
    pub ts: String,
    /// The values of the channels at the time of the checkpoint (graph state).
    pub channel_values: S,
    /// The versions of the channels at the time of the checkpoint.
    pub channel_versions: ChannelVersions,
    /// Map from node ID to map from channel name to version seen.
    /// Used to determine which nodes to execute next.
    pub versions_seen: HashMap<String, ChannelVersions>,
    /// The channels that were updated in this checkpoint.
    pub updated_channels: Option<Vec<String>>,
    /// Pending sends for message passing.
    pub pending_sends: Vec<PendingWrite>,
    /// Metadata for the checkpoint.
    pub metadata: CheckpointMetadata,
}

/// Item returned by Checkpointer::list for history / time-travel.
#[derive(Debug, Clone)]
pub struct CheckpointListItem {
    pub checkpoint_id: String,
    pub metadata: CheckpointMetadata,
}

/// A tuple containing a checkpoint and its associated data.
///
/// Returned by Checkpointer::get_tuple.
#[derive(Debug, Clone)]
pub struct CheckpointTuple<S> {
    /// Configuration for the checkpoint.
    pub config: RunnableConfig,
    /// The checkpoint snapshot.
    pub checkpoint: Checkpoint<S>,
    /// Metadata for the checkpoint.
    pub metadata: CheckpointMetadata,
    /// Parent configuration (if any).
    pub parent_config: Option<RunnableConfig>,
    /// Pending writes (if any).
    pub pending_writes: Option<Vec<PendingWrite>>,
}

impl<S> Checkpoint<S> {
    /// Creates a checkpoint from current state for saving after invoke.
    ///
    /// Uses UUID6 for the checkpoint ID, ensuring time-ordered and unique identifiers.
    /// Uses UUID6 for checkpoint IDs.
    ///
    /// # Arguments
    ///
    /// - `state`: The state to checkpoint
    /// - `source`: The source of the checkpoint (Input, Loop, Update, Fork)
    /// - `step`: The step number (-1 for input, 0+ for loop steps)
    pub fn from_state(state: S, source: CheckpointSource, step: i64) -> Self {
        let now = SystemTime::now();
        let id = uuid6().to_string();
        let ts = format!(
            "{}",
            now.duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        Self {
            v: CHECKPOINT_VERSION,
            id,
            ts,
            channel_values: state,
            channel_versions: HashMap::new(),
            versions_seen: HashMap::new(),
            updated_channels: None,
            pending_sends: Vec::new(),
            metadata: CheckpointMetadata {
                source,
                step,
                created_at: Some(now),
                parents: HashMap::new(),
            },
        }
    }

    /// Creates a checkpoint with a specific ID.
    ///
    /// Useful for restoring checkpoints or creating checkpoints with known IDs.
    pub fn with_id(id: String, state: S, source: CheckpointSource, step: i64) -> Self {
        let now = SystemTime::now();
        let ts = format!(
            "{}",
            now.duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        Self {
            v: CHECKPOINT_VERSION,
            id,
            ts,
            channel_values: state,
            channel_versions: HashMap::new(),
            versions_seen: HashMap::new(),
            updated_channels: None,
            pending_sends: Vec::new(),
            metadata: CheckpointMetadata {
                source,
                step,
                created_at: Some(now),
                parents: HashMap::new(),
            },
        }
    }
}

impl<S: Clone> Checkpoint<S> {
    /// Creates a deep copy of the checkpoint.
    ///
    /// Used for forking checkpoints or creating mutable copies.
    pub fn copy(&self) -> Self {
        Self {
            v: self.v,
            id: self.id.clone(),
            ts: self.ts.clone(),
            channel_values: self.channel_values.clone(),
            channel_versions: self.channel_versions.clone(),
            versions_seen: self
                .versions_seen
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            updated_channels: self.updated_channels.clone(),
            pending_sends: self.pending_sends.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

impl<S: Default> Default for Checkpoint<S> {
    /// Creates an empty checkpoint with default values.
    fn default() -> Self {
        let now = SystemTime::now();
        let id = uuid6().to_string();
        let ts = format!(
            "{}",
            now.duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        Self {
            v: CHECKPOINT_VERSION,
            id,
            ts,
            channel_values: S::default(),
            channel_versions: HashMap::new(),
            versions_seen: HashMap::new(),
            updated_channels: None,
            pending_sends: Vec::new(),
            metadata: CheckpointMetadata::default(),
        }
    }
}
