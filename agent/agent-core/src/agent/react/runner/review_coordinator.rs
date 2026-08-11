//! ReviewCoordinator — bridges the nudge detector to production conversation loops.
//!
//! `ReactLoop` (in `nudge.rs`) implements the low-level 3-gate nudge logic but
//! has zero production callers. This module wraps it with a coordinator that
//! the ACP / CLI / serve loops can hold per session, feeding lifecycle events
//! (turn start, tool batch, turn end) and querying whether a review should
//! spawn.
//!
//! # Lifecycle
//!
//! ```text
//! prompt arrives
//!   │
//!   ▼
//! coordinator.on_turn_start(&tool_avail)
//!   │
//!   ▼
//! stream events arrive:
//!   ToolStart/ToolEnd → coordinator.on_tool_batch_complete(&tool_avail)
//!   │
//!   ▼
//! run finishes (Finished or Cancelled):
//!   coordinator.on_turn_end(final_response, interrupted, &tool_avail)
//!     → Option<ReviewTrigger>
//! ```
//!
//! The coordinator is designed to be held in a `Arc<Mutex<>>` by the session
//! owner. All methods take `&mut self` and are synchronous.

use crate::agent::react::nudge::{NudgeToolAvailability, ReactLoop, ReviewTrigger};
use crate::agent::ReactBuildConfig;

/// Decision returned by the coordinator when a turn finishes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CoordinatorTrigger {
    pub review_memory: bool,
    pub review_skills: bool,
}

impl CoordinatorTrigger {
    pub fn any(&self) -> bool {
        self.review_memory || self.review_skills
    }

    /// Convert into the `(review_memory, review_skills)` tuple used by the
    /// spawn functions.
    pub fn to_flags(self) -> (bool, bool) {
        (self.review_memory, self.review_skills)
    }
}

impl From<ReviewTrigger> for CoordinatorTrigger {
    fn from(t: ReviewTrigger) -> Self {
        Self {
            review_memory: t.review_memory,
            review_skills: t.review_skills,
        }
    }
}

/// Coordinates nudge-driven background review triggering across conversation turns.
///
/// Held per-session by the conversation owner (ACP agent, CLI REPL, etc.).
/// Each prompt cycle calls:
/// 1. [`on_turn_start`](Self::on_turn_start) at prompt receipt
/// 2. [`on_tool_batch_complete`](Self::on_tool_batch_complete) after each tool batch
/// 3. [`on_turn_end`](Self::on_turn_end) when the turn completes
pub struct ReviewCoordinator {
    nudge: ReactLoop,
}

impl ReviewCoordinator {
    /// Create a coordinator from a build config.
    ///
    /// `has_memory_store` should be `true` when the agent was built with a
    /// MemoryStore (i.e. `memory_enabled || user_profile_enabled`).
    pub fn new(config: ReactBuildConfig, has_memory_store: bool) -> Self {
        Self {
            nudge: ReactLoop::new(config, has_memory_store),
        }
    }

    /// Hydrate counters from a prior session history.
    ///
    /// Call once when resuming a session to reconstruct the effective turn/tool
    /// counters so nudges fire on the same cadence as a continuous session.
    ///
    /// - `prior_user_turns` is the number of completed user turns in the history.
    /// - `prior_tool_iterations` is the number of tool-role messages (each
    ///   corresponds to one Act→Observe cycle).
    pub fn init_from_history(&mut self, prior_user_turns: u32, prior_tool_iterations: u32) {
        self.nudge
            .init_from_history(prior_user_turns, prior_tool_iterations);
    }

    /// Called at the beginning of each user turn (before the agent runs).
    ///
    /// Increments the memory nudge counter and checks the 3-gate condition.
    pub fn on_turn_start(&mut self, tools: &NudgeToolAvailability) {
        self.nudge.on_user_turn_start(tools);
    }

    /// Called after each tool-execution batch completes (each Act→Observe cycle).
    ///
    /// Increments the skill nudge counter.
    pub fn on_tool_batch_complete(&mut self, tools: &NudgeToolAvailability) {
        self.nudge.on_tool_batch_complete(tools);
    }

    /// Called when the turn finishes.
    ///
    /// Implements the `final_response && !interrupted` gate (#11): reviews
    /// are only triggered on clean completions, never after cancellation or
    /// errors.
    ///
    /// Returns `Some(trigger)` if a nudge fired, `None` otherwise.
    pub fn on_turn_end(
        &mut self,
        final_response: bool,
        interrupted: bool,
        tools: &NudgeToolAvailability,
    ) -> Option<CoordinatorTrigger> {
        // #11 gate: only trigger on clean completions
        if !final_response || interrupted {
            return None;
        }

        self.nudge.check_skill_trigger_after_turn(tools);
        let trigger = self.nudge.drain_pending_reviews();
        if trigger.any() {
            Some(CoordinatorTrigger::from(trigger))
        } else {
            None
        }
    }

    // ── Accessors (for testing) ──────────────────────────────────

    pub fn turns_since_memory(&self) -> u32 {
        self.nudge.turns_since_memory()
    }

    pub fn iters_since_skill(&self) -> u32 {
        self.nudge.iters_since_skill()
    }

    pub fn should_review_memory(&self) -> bool {
        self.nudge.should_review_memory()
    }

    pub fn should_review_skills(&self) -> bool {
        self.nudge.should_review_skills()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_intervals(memory: u32, skill: u32) -> ReactBuildConfig {
        let mut cfg = ReactBuildConfig::from_env();
        cfg.memory_nudge_interval = memory;
        cfg.skill_nudge_interval = skill;
        cfg
    }

    fn tools_both() -> NudgeToolAvailability {
        NudgeToolAvailability {
            has_memory_tool: true,
            has_skill_manage_tool: true,
        }
    }

    // ── #1: turn-level coordinator triggers after N turns ──

    #[test]
    fn coordinator_triggers_memory_review_after_n_turns() {
        let mut coord = ReviewCoordinator::new(config_with_intervals(3, 0), true);
        let tools = tools_both();

        for _ in 0..2 {
            coord.on_turn_start(&tools);
            let r = coord.on_turn_end(true, false, &tools);
            assert!(r.is_none(), "should not trigger before interval");
        }

        coord.on_turn_start(&tools);
        let r = coord.on_turn_end(true, false, &tools).unwrap();
        assert!(r.review_memory);
        assert!(!r.review_skills);
    }

    #[test]
    fn coordinator_triggers_skill_review_after_n_tool_batches() {
        let mut coord = ReviewCoordinator::new(config_with_intervals(0, 5), false);
        let tools = tools_both();

        coord.on_turn_start(&tools);
        for _ in 0..5 {
            coord.on_tool_batch_complete(&tools);
        }
        let r = coord.on_turn_end(true, false, &tools).unwrap();
        assert!(r.review_skills);
        assert!(!r.review_memory);
    }

    // ── #11: final_response && !interrupted gate ──

    #[test]
    fn coordinator_does_not_trigger_on_interrupted_turn() {
        let mut coord = ReviewCoordinator::new(config_with_intervals(1, 0), true);
        coord.on_turn_start(&tools_both());
        // interrupted = true → no trigger even though interval met
        let r = coord.on_turn_end(true, true, &tools_both());
        assert!(r.is_none(), "interrupted turn should not trigger review");
    }

    #[test]
    fn coordinator_does_not_trigger_on_cancelled_turn() {
        let mut coord = ReviewCoordinator::new(config_with_intervals(1, 0), true);
        coord.on_turn_start(&tools_both());
        // final_response = false (cancelled) → no trigger
        let r = coord.on_turn_end(false, false, &tools_both());
        assert!(r.is_none(), "cancelled turn should not trigger review");
    }

    #[test]
    fn coordinator_triggers_on_clean_completion() {
        let mut coord = ReviewCoordinator::new(config_with_intervals(1, 0), true);
        coord.on_turn_start(&tools_both());
        let r = coord.on_turn_end(true, false, &tools_both()).unwrap();
        assert!(r.review_memory);
    }

    // ── Hydration ──

    #[test]
    fn coordinator_hydration_preserves_counter() {
        let mut coord = ReviewCoordinator::new(config_with_intervals(10, 0), true);
        coord.init_from_history(15, 0);
        assert_eq!(coord.turns_since_memory(), 5);
    }

    #[test]
    fn coordinator_hydration_preserves_skill_counter() {
        let mut coord = ReviewCoordinator::new(config_with_intervals(0, 30), false);
        coord.init_from_history(0, 25);
        assert_eq!(coord.iters_since_skill(), 25);
    }

    // ── Counter persistence across turns ──

    #[test]
    fn memory_counter_persists_across_non_triggering_turns() {
        let mut coord = ReviewCoordinator::new(config_with_intervals(10, 0), true);
        let tools = tools_both();

        // 3 turns, none should trigger
        for _ in 0..3 {
            coord.on_turn_start(&tools);
            assert!(coord.on_turn_end(true, false, &tools).is_none());
        }
        assert_eq!(coord.turns_since_memory(), 3);
    }
}
