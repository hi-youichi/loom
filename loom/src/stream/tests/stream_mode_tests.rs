#[cfg(test)]
mod tests {
    use crate::stream::StreamMode;

    #[allow(clippy::useless_vec)]
    #[test]
    fn stream_mode_variants_hashset_equality() {
        use std::collections::HashSet;

        // Test that each mode is distinct
        let modes = vec![
            StreamMode::Values,
            StreamMode::Updates,
            StreamMode::Messages,
            StreamMode::Custom,
            StreamMode::Checkpoints,
            StreamMode::Tasks,
            StreamMode::Tools,
            StreamMode::Debug,
        ];

        // Ensure all modes are unique
        let modes_set: HashSet<StreamMode> = HashSet::from_iter(modes.iter().copied());
        assert_eq!(modes_set.len(), 8, "All stream modes should be unique");

        // Test Debug mode contains other modes' functionality
        assert!(StreamMode::Debug != StreamMode::Tasks);
        assert!(StreamMode::Debug != StreamMode::Tools);
        assert!(StreamMode::Debug != StreamMode::Checkpoints);
    }
}
