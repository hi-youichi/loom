//! UUID version 6 implementation for checkpoint IDs.
//!
//! UUID6 is a time-ordered variant of UUID that is suitable for database
//! indexing and sorted storage. It provides better locality than UUIDv4
//! while maintaining uniqueness guarantees.
//!
//! Aligns with UUID6 implementation for checkpoint IDs (cf. checkpoint/base/id.py).
//!
//! # Features
//!
//! - Time-ordered UUIDs for better DB locality
//! - Monotonically increasing within the same timestamp
//! - Compatible with standard UUID format
//!
//! # Example
//!
//! ```rust
//! use loom::memory::uuid6;
//!
//! let id = uuid6();
//! println!("Checkpoint ID: {}", id);
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The last UUID6 timestamp to ensure monotonic ordering.
static LAST_V6_TIMESTAMP: AtomicU64 = AtomicU64::new(0);

/// UUID version 6 - a time-ordered UUID variant.
///
/// UUID6 reorders the timestamp fields of UUID1 for better database locality.
/// The timestamp is stored in big-endian order at the beginning of the UUID,
/// which ensures that UUIDs generated later sort after earlier ones.
///
/// # Structure
///
/// ```text
/// xxxxxxxx-xxxx-6xxx-yxxx-xxxxxxxxxxxx
/// |--------|----| |   |---|-----------|
/// time_high time_mid | var clock_seq node
///              version
/// ```
///
/// # Fields
///
/// - `time_high` (32 bits): High bits of timestamp
/// - `time_mid` (16 bits): Middle bits of timestamp
/// - `version` (4 bits): Always 6
/// - `time_low` (12 bits): Low bits of timestamp
/// - `variant` (2 bits): RFC 4122 variant
/// - `clock_seq` (14 bits): Random or counter
/// - `node` (48 bits): Random node identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Uuid6 {
    /// The 128-bit UUID value.
    bytes: [u8; 16],
}

impl Uuid6 {
    /// Creates a new UUID6 with the given integer value and version.
    ///
    /// # Arguments
    ///
    /// - `int_val`: The 128-bit integer value
    /// - `version`: The UUID version (should be 6)
    fn from_int(mut int_val: u128, version: u8) -> Self {
        // Set the variant to RFC 4122 (10xx)
        int_val &= !(0xC000_u128 << 48);
        int_val |= 0x8000_u128 << 48;

        // Set the version number
        int_val &= !(0xF000_u128 << 64);
        int_val |= (version as u128) << 76;

        let bytes = int_val.to_be_bytes();
        Self { bytes }
    }

    /// Returns the UUID as a 128-bit integer.
    pub fn as_u128(&self) -> u128 {
        u128::from_be_bytes(self.bytes)
    }

    /// Returns the UUID as a byte array.
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.bytes
    }

    /// Returns the UUID version.
    pub fn version(&self) -> u8 {
        (self.bytes[6] >> 4) & 0x0F
    }

    /// Returns the timestamp from the UUID.
    ///
    /// For UUID6, the timestamp is the number of 100-nanosecond intervals
    /// since October 15, 1582 (UUID epoch).
    pub fn timestamp(&self) -> u64 {
        let int_val = self.as_u128();

        // Extract time fields (UUID6 format)
        let time_low = ((int_val >> 64) & 0x0FFF) as u64;
        let time_mid = ((int_val >> 80) & 0xFFFF) as u64;
        let time_high = ((int_val >> 96) & 0xFFFF_FFFF) as u64;

        (time_high << 28) | (time_mid << 12) | time_low
    }
}

impl std::fmt::Display for Uuid6 {
    /// Formats the UUID as a hyphenated string.
    ///
    /// Format: `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3],
            self.bytes[4], self.bytes[5],
            self.bytes[6], self.bytes[7],
            self.bytes[8], self.bytes[9],
            self.bytes[10], self.bytes[11], self.bytes[12], self.bytes[13], self.bytes[14], self.bytes[15]
        )
    }
}

/// Generates a new UUID version 6.
///
/// UUID6 is a field-compatible version of UUIDv1, reordered for improved
/// database locality. The timestamp is stored in big-endian order, ensuring
/// that UUIDs generated later sort after earlier ones.
///
/// # Monotonicity
///
/// If multiple UUIDs are generated within the same 100-nanosecond interval,
/// the function ensures monotonicity by incrementing the timestamp.
///
/// # Example
///
/// ```rust
/// use loom::memory::uuid6;
///
/// let id1 = uuid6();
/// let id2 = uuid6();
///
/// // UUIDs are time-ordered
/// assert!(id2.to_string() >= id1.to_string());
/// ```
pub fn uuid6() -> Uuid6 {
    uuid6_with_params(None, None)
}

/// Generates a UUID6 with optional node and clock_seq parameters.
///
/// # Arguments
///
/// - `node`: Optional 48-bit node identifier. If None, a random value is used.
/// - `clock_seq`: Optional 14-bit clock sequence. If None, a random value is used.
pub fn uuid6_with_params(node: Option<u64>, clock_seq: Option<u16>) -> Uuid6 {
    // Get current time in nanoseconds since Unix epoch
    let nanoseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);

    // Convert to 100-nanosecond intervals since UUID epoch
    // 0x01b21dd213814000 is the number of 100-ns intervals between
    // UUID epoch (1582-10-15) and Unix epoch (1970-01-01)
    const UUID_EPOCH_OFFSET: u64 = 0x01b2_1dd2_1381_4000;
    let mut timestamp = nanoseconds / 100 + UUID_EPOCH_OFFSET;

    // Ensure monotonicity
    loop {
        let last = LAST_V6_TIMESTAMP.load(Ordering::SeqCst);
        if timestamp <= last {
            timestamp = last + 1;
        }

        match LAST_V6_TIMESTAMP.compare_exchange(
            last,
            timestamp,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => break,
            Err(_) => continue, // Retry if another thread updated
        }
    }

    // Generate random values for node and clock_seq if not provided
    let node = node.unwrap_or_else(|| rand_u48());
    let clock_seq = clock_seq.unwrap_or_else(|| rand_u14());

    // Build UUID6 integer
    let time_high_and_time_mid = (timestamp >> 12) & 0xFFFF_FFFF_FFFF;
    let time_low_and_version = timestamp & 0x0FFF;

    let mut uuid_int: u128 = (time_high_and_time_mid as u128) << 80;
    uuid_int |= (time_low_and_version as u128) << 64;
    uuid_int |= ((clock_seq & 0x3FFF) as u128) << 48;
    uuid_int |= (node & 0xFFFF_FFFF_FFFF) as u128;

    Uuid6::from_int(uuid_int, 6)
}

/// Generates a simple random 48-bit value.
fn rand_u48() -> u64 {
    // Use simple XorShift for randomness
    // In production, consider using a proper random number generator
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(42);

    let mut state = seed ^ 0xDEAD_BEEF_CAFE_BABE;
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;

    state & 0xFFFF_FFFF_FFFF
}

/// Generates a simple random 14-bit value.
fn rand_u14() -> u16 {
    (rand_u48() & 0x3FFF) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// **Scenario**: UUID6 has correct version.
    #[test]
    fn test_uuid6_version() {
        let id = uuid6();
        assert_eq!(id.version(), 6);
    }

    /// **Scenario**: UUID6 generates unique values.
    #[test]
    fn test_uuid6_uniqueness() {
        let mut ids: HashSet<String> = HashSet::new();
        for _ in 0..1000 {
            let id = uuid6();
            assert!(ids.insert(id.to_string()), "Duplicate UUID generated");
        }
    }

    /// **Scenario**: UUID6 is monotonically increasing.
    #[test]
    fn test_uuid6_monotonic() {
        let id1 = uuid6();
        let id2 = uuid6();
        let id3 = uuid6();

        // Timestamps should be non-decreasing
        assert!(id2.timestamp() >= id1.timestamp());
        assert!(id3.timestamp() >= id2.timestamp());
    }

    /// **Scenario**: UUID6 string format is correct.
    #[test]
    fn test_uuid6_string_format() {
        let id = uuid6();
        let s = id.to_string();

        // Check format: 8-4-4-4-12
        let parts: Vec<&str> = s.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);

        // Check version digit (should be '6')
        assert!(parts[2].starts_with('6'));
    }

    /// **Scenario**: UUID6 Display trait works.
    #[test]
    fn test_uuid6_display() {
        let id = uuid6();
        let formatted = format!("{}", id);
        assert_eq!(formatted, id.to_string());
    }

    /// **Scenario**: UUID6 with custom params works.
    #[test]
    fn test_uuid6_with_params() {
        let node = 0x123456789ABC;
        let clock_seq = 0x1234;

        let id = uuid6_with_params(Some(node), Some(clock_seq));
        assert_eq!(id.version(), 6);

        // The node ID should be present in the lower 48 bits
        let int_val = id.as_u128();
        let extracted_node = (int_val & 0xFFFF_FFFF_FFFF) as u64;
        assert_eq!(extracted_node, node);
    }

    /// **Scenario**: UUID6 bytes can be retrieved.
    #[test]
    fn test_uuid6_as_bytes() {
        let id = uuid6();
        let bytes = id.as_bytes();
        assert_eq!(bytes.len(), 16);

        // Version should be in byte 6, high nibble
        assert_eq!((bytes[6] >> 4) & 0x0F, 6);
    }

    /// **Scenario**: UUID6 timestamp can be extracted.
    #[test]
    fn test_uuid6_timestamp_extraction() {
        let id1 = uuid6();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let id2 = uuid6();

        // Second UUID should have later timestamp
        assert!(id2.timestamp() > id1.timestamp());
    }

    /// **Scenario**: Multiple rapid UUID6 generations maintain uniqueness.
    #[test]
    fn test_uuid6_rapid_generation() {
        let ids: Vec<Uuid6> = (0..100).map(|_| uuid6()).collect();
        let unique: HashSet<_> = ids.iter().map(|id| id.to_string()).collect();
        assert_eq!(unique.len(), 100, "All UUIDs should be unique");
    }

    /// **Scenario**: UUID6 can be cloned and compared.
    #[test]
    fn test_uuid6_clone_eq() {
        let id1 = uuid6();
        let id2 = id1.clone();
        assert_eq!(id1, id2);

        let id3 = uuid6();
        assert_ne!(id1, id3);
    }

    /// **Scenario**: UUID6 can be hashed.
    #[test]
    fn test_uuid6_hash() {
        let mut set: HashSet<Uuid6> = HashSet::new();
        let id1 = uuid6();
        let id2 = uuid6();

        set.insert(id1);
        set.insert(id2);
        assert_eq!(set.len(), 2);

        // Same UUID should not increase set size
        set.insert(id1);
        assert_eq!(set.len(), 2);
    }
}
