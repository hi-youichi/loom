//! Checkpoint and metadata types.
//!
//! A [`Checkpoint`] captures one persisted snapshot of graph or Pregel runtime
//! state, along with the frontier metadata needed to resume execution or inspect
//! history.

use super::config::RunnableConfig;
use super::uuid6::uuid6;
use serde_json::Value;
use std::collections::HashMap;
use std::time::SystemTime;

/// Current version of the serialized checkpoint format.
pub const CHECKPOINT_VERSION: u32 = 2;

/// Special write type for errors.
pub const ERROR: &str = "__error__";
/// Special write type for scheduled tasks.
pub const SCHEDULED: &str = "__scheduled__";
/// Special write type for interrupts.
pub const INTERRUPT: &str = "__interrupt__";
/// Special write type for resume operations.
pub const RESUME: &str = "__resume__";

/// Maps reserved write-channel names to the synthetic indices used by storage backends.
///
/// Checkpointer implementations use negative indices so reserved writes can be
/// persisted alongside normal writes without colliding with user channels.
pub fn writes_idx_map(write_type: &str) -> Option<i32> {
    match write_type {
        ERROR => Some(-1),
        SCHEDULED => Some(-2),
        INTERRUPT => Some(-3),
        RESUME => Some(-4),
        _ => None,
    }
}

/// A pending write stored alongside a checkpoint.
///
/// Tuple layout: `(task_id, channel, value)`.
pub type PendingWrite = (String, String, Value);

/// Channel versions map (`channel_name -> version`).
///
/// Versions are stored as strings so different backends can serialize them
/// without committing to one numeric type.
pub type ChannelVersions = HashMap<String, String>;

/// Metadata describing where a checkpoint came from and how it relates to others.
/// Belongs to the execution engine — does NOT contain domain-specific fields.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct KernelMetadata {
    /// The source of the checkpoint (input, loop, update, fork).
    pub source: CheckpointSource,
    /// The step number of the checkpoint (-1 for input, 0 for first loop, etc.).
    pub step: i64,
    /// Timestamp when this checkpoint was created.
    pub created_at: Option<std::time::SystemTime>,
    /// Parent checkpoint IDs (checkpoint_ns -> checkpoint_id).
    #[serde(default)]
    pub parents: HashMap<String, String>,
    /// Child checkpoint IDs grouped by child checkpoint namespace.
    #[serde(default)]
    pub children: HashMap<String, Vec<String>>,
    /// Session title extracted from state by the metadata extractor closure.
    /// Populated by `CompiledStateGraph` when a `metadata_extractor` is registered.
    #[serde(default)]
    pub summary: Option<String>,
}

/// Trait for user-defined checkpoint metadata.
///
/// Implementors define how their metadata is persisted to and read from
/// the database. The kernel never interprets the content — it only
/// clones and transports the value.
///
/// `to_persist()` output **must** be a valid JSON object (e.g. `{"summary":"..."}`)
/// so that SQLite `json_extract()` can query individual keys directly.
pub trait CheckpointUserMeta: Clone + std::fmt::Debug + Send + Sync + 'static {
    fn to_persist(&self) -> Option<String>;
    fn from_persist(s: &str) -> Self;
}

impl CheckpointUserMeta for () {
    fn to_persist(&self) -> Option<String> {
        None
    }
    fn from_persist(_: &str) -> Self {}
}

/// Backwards-compatible type alias.
/// `CheckpointMetadata` now equals `KernelMetadata` (summary field removed).
pub type CheckpointMetadata = KernelMetadata;

/// Why a checkpoint was created.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
        let _ = KernelMetadata {
            source: c,
            step: 0,
            created_at: None,
            parents: HashMap::new(),
            children: HashMap::new(),
                summary: None,
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
        assert_eq!(checkpoint.kernel.step, 5);
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
        assert_eq!(checkpoint.kernel.source, CheckpointSource::Input);
    }

    /// **Scenario**: CheckpointTuple holds all expected fields.
    #[test]
    fn checkpoint_tuple_holds_all_fields() {
        let checkpoint: Checkpoint<String> =
            Checkpoint::from_state("state".to_string(), CheckpointSource::Loop, 1);
        let config = RunnableConfig::default();
        let metadata = checkpoint.kernel.clone();

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
        let metadata = KernelMetadata::default();

        assert_eq!(metadata.source, CheckpointSource::Input);
        assert_eq!(metadata.step, 0);
        assert!(metadata.created_at.is_none());
        assert!(metadata.parents.is_empty());
        assert!(metadata.children.is_empty());
    }
}

fn default_checkpoint_version() -> u32 {
    CHECKPOINT_VERSION
}

/// One persisted checkpoint snapshot.
///
/// Checkpoints are keyed by `(thread_id, checkpoint_ns, checkpoint_id)` inside
/// a [`crate::memory::Checkpointer`]. `channel_values` stores the user-visible
/// state snapshot, while the version and pending-write fields preserve enough
/// runtime context to continue execution or inspect lineage later.
///
/// The `M` parameter is the user-defined metadata type. Default `M = ()` means
/// no user metadata. The kernel never interprets `M` — it only clones and
/// transports the value.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(bound = "S: serde::Serialize + serde::de::DeserializeOwned, M: Clone + Default + serde::Serialize + serde::de::DeserializeOwned")]
pub struct Checkpoint<S, M = ()> {
    /// The version of the checkpoint format. Currently `2`.
    #[serde(default = "default_checkpoint_version")]
    pub v: u32,
    /// Unique checkpoint id.
    pub id: String,
    /// Creation timestamp expressed as milliseconds since the Unix epoch.
    pub ts: String,
    /// Materialized graph state at the time the checkpoint was written.
    pub channel_values: S,
    /// Channel frontier at the time of the checkpoint.
    #[serde(default)]
    pub channel_versions: ChannelVersions,
    /// Per-node view of channel versions already observed.
    #[serde(default)]
    pub versions_seen: HashMap<String, ChannelVersions>,
    /// Channels updated at the barrier that produced this checkpoint.
    #[serde(default)]
    pub updated_channels: Option<Vec<String>>,
    /// Pending sends that should be materialized in future execution steps.
    #[serde(default)]
    pub pending_sends: Vec<PendingWrite>,
    /// Pending reserved writes that are not represented as normal channel state.
    #[serde(default)]
    pub pending_writes: Vec<PendingWrite>,
    /// Pending interrupts persisted so a later run can resume them.
    #[serde(default)]
    pub pending_interrupts: Vec<Value>,
    /// Kernel metadata describing source, step, and lineage.
    pub kernel: KernelMetadata,
    /// User-defined metadata. The kernel never interprets this field.
    #[serde(default)]
    pub user: M,
}

/// Lightweight checkpoint list item for history and time-travel UIs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckpointListItem {
    /// Unique checkpoint id.
    pub checkpoint_id: String,
    /// Metadata associated with the checkpoint.
    pub metadata: KernelMetadata,
}

/// Expanded checkpoint record returned by [`crate::memory::Checkpointer::get_tuple`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(bound = "S: serde::Serialize + serde::de::DeserializeOwned, M: Clone + Default + serde::Serialize + serde::de::DeserializeOwned")]
pub struct CheckpointTuple<S, M = ()> {
    /// Configuration used to load this checkpoint.
    pub config: RunnableConfig,
    /// The checkpoint snapshot.
    pub checkpoint: Checkpoint<S, M>,
    /// Metadata duplicated for convenience.
    pub metadata: KernelMetadata,
    /// Parent configuration, when the backend can reconstruct it.
    pub parent_config: Option<RunnableConfig>,
    /// Pending writes returned separately by some backends.
    pub pending_writes: Option<Vec<PendingWrite>>,
}

impl<S, M: Default> Checkpoint<S, M> {
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
            pending_writes: Vec::new(),
            pending_interrupts: Vec::new(),
            kernel: KernelMetadata {
                source,
                step,
                created_at: Some(now),
                parents: HashMap::new(),
                children: HashMap::new(),
                summary: None,
            },
            user: M::default(),
        }
    }

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
            pending_writes: Vec::new(),
            pending_interrupts: Vec::new(),
            kernel: KernelMetadata {
                source,
                step,
                created_at: Some(now),
                parents: HashMap::new(),
                children: HashMap::new(),
                summary: None,
            },
            user: M::default(),
        }
    }
}

impl<S: Clone, M: Clone> Checkpoint<S, M> {
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
            pending_writes: self.pending_writes.clone(),
            pending_interrupts: self.pending_interrupts.clone(),
            kernel: self.kernel.clone(),
            user: self.user.clone(),
        }
    }

    pub fn fork_from(&self, parent_namespace: String, parent_checkpoint_id: String) -> Self
    where
        M: Default,
    {
        let mut forked = Checkpoint::from_state(
            self.channel_values.clone(),
            CheckpointSource::Fork,
            self.kernel.step,
        );
        forked.channel_versions = self.channel_versions.clone();
        forked.versions_seen = self.versions_seen.clone();
        forked.updated_channels = self.updated_channels.clone();
        forked.pending_sends = self.pending_sends.clone();
        forked.pending_writes = self.pending_writes.clone();
        forked.pending_interrupts = self.pending_interrupts.clone();
        forked.kernel.parents = self.kernel.parents.clone();
        forked
            .kernel
            .parents
            .insert(parent_namespace, parent_checkpoint_id);
        forked.kernel.children = self.kernel.children.clone();
        forked
    }
}

impl<S: Default, M: Default> Default for Checkpoint<S, M> {
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
            pending_writes: Vec::new(),
            pending_interrupts: Vec::new(),
            kernel: KernelMetadata::default(),
            user: M::default(),
        }
    }
}
