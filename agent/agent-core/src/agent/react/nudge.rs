//! ReactLoop nudge detection — aligns with Hermes `conversation_loop.py:549-556, 850-852, 4642-4648`.
//!
//! Hermes maintains two independent counters on the agent instance:
//! - `_turns_since_memory` — incremented per **user turn**, checked against `_memory_nudge_interval`
//! - `_iters_since_skill` — incremented per **tool iteration**, checked against `_skill_nudge_interval`
//!
//! Both use a 3-gate trigger model: interval > 0 AND tool registered AND (for memory) store exists.
//! When the counter reaches the interval, the corresponding `should_review_*` flag is set and the
//! counter resets to 0.
//!
//! anureo's graph runner is stateless across invocations, so this struct carries the mutable counters
//! that a higher-level conversation coordinator (CLI / ACP / goal runner) holds between turns.

use crate::agent::ReactBuildConfig;

/// Snapshot of which nudge-relevant tools are available in the current tool registry.
///
/// Mirrors Hermes `agent.valid_tool_names` checks (`"memory" in valid_tool_names`,
/// `"skill_manage" in valid_tool_names`).
#[derive(Debug, Clone, Default)]
pub struct NudgeToolAvailability {
    /// Whether a `memory` tool is registered.
    pub has_memory_tool: bool,
    /// Whether a `skill_manage` tool is registered.
    pub has_skill_manage_tool: bool,
}

impl NudgeToolAvailability {
    /// Build from a list of tool names (case-sensitive, exact match).
    pub fn from_tool_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut av = Self::default();
        for n in names {
            let n = n.as_ref();
            if n == "memory" {
                av.has_memory_tool = true;
            }
            if n == "skill_manage" {
                av.has_skill_manage_tool = true;
            }
        }
        av
    }
}

/// Stateful nudge detector — the Rust analogue of Hermes's `_turns_since_memory` /
/// `_iters_since_skill` counters plus their 3-gate trigger logic.
///
/// Created once per agent instance (or per conversation coordinator) and persisted
/// across turns. The coordinator calls:
/// - [`Self::init_from_history`] when reconstructing from a prior session
/// - [`Self::on_user_turn_start`] at the beginning of each user turn
/// - [`Self::on_tool_batch_complete`] after each tool-execution batch
/// - [`Self::drain_pending_reviews`] after the turn finishes to collect the trigger flags
#[derive(Debug, Clone)]
pub struct ReactLoop {
    config: ReactBuildConfig,
    turns_since_memory: u32,
    iters_since_skill: u32,
    /// `Some` when a MemoryStore was created for this agent (gate 3 for memory nudge).
    has_memory_store: bool,
    should_review_memory: bool,
    should_review_skills: bool,
}

impl ReactLoop {
    /// Create a new nudge detector from a build config.
    ///
    /// `has_memory_store` should be `true` when the agent's tool source was built
    /// with a `MemoryStore` (i.e. `memory_enabled || user_profile_enabled`).
    pub fn new(config: ReactBuildConfig, has_memory_store: bool) -> Self {
        Self {
            config,
            turns_since_memory: 0,
            iters_since_skill: 0,
            has_memory_store,
            should_review_memory: false,
            should_review_skills: false,
        }
    }

    /// Hydrate counters from persisted conversation history.
    ///
    /// Aligns with Hermes `conversation_loop.py:510-520`: when a freshly-built agent
    /// instance (counters at 0) resumes a long conversation, reconstruct an effective
    /// count from prior user turns so the nudge fires on the same 1-in-N cadence.
    ///
    /// Uses modulo so a session that happens to land just past a multiple of N does
    /// **not** fire immediately on resume (which would surprise the user).
    ///
    /// Both counters are hydrated:
    /// - `turns_since_memory` ← `prior_user_turns % memory_nudge_interval`
    /// - `iters_since_skill` ← `prior_tool_iterations % skill_nudge_interval`
    ///
    /// The idempotency guard (`counter == 0`) ensures we don't overwrite a counter
    /// that was already explicitly set (e.g. a mid-flight conversation loop).
    pub fn init_from_history(&mut self, prior_user_turns: u32, prior_tool_iterations: u32) {
        if prior_user_turns > 0
            && self.turns_since_memory == 0
            && self.config.memory_nudge_interval > 0
        {
            self.turns_since_memory = prior_user_turns % self.config.memory_nudge_interval;
        }
        if prior_tool_iterations > 0
            && self.iters_since_skill == 0
            && self.config.skill_nudge_interval > 0
        {
            self.iters_since_skill = prior_tool_iterations % self.config.skill_nudge_interval;
        }
    }

    /// Called at the start of each user turn.
    ///
    /// Aligns with Hermes `conversation_loop.py:549-556`:
    /// ```text
    /// _should_review_memory = False
    /// if (agent._memory_nudge_interval > 0
    ///         and "memory" in agent.valid_tool_names
    ///         and agent._memory_store):
    ///     agent._turns_since_memory += 1
    ///     if agent._turns_since_memory >= agent._memory_nudge_interval:
    ///         _should_review_memory = True
    ///         agent._turns_since_memory = 0
    /// ```
    pub fn on_user_turn_start(&mut self, tools: &NudgeToolAvailability) {
        // Gate 1: interval > 0, Gate 2: memory tool registered, Gate 3: store exists
        let gate_open =
            self.config.memory_nudge_interval > 0 && tools.has_memory_tool && self.has_memory_store;

        if !gate_open {
            return;
        }

        self.turns_since_memory = self.turns_since_memory.saturating_add(1);
        if self.turns_since_memory >= self.config.memory_nudge_interval {
            self.should_review_memory = true;
            self.turns_since_memory = 0; // reset after fire
        }
    }

    /// Called after each tool-execution batch completes (each Act → Observe cycle).
    ///
    /// Aligns with Hermes `conversation_loop.py:848-852`:
    /// ```text
    /// if (agent._skill_nudge_interval > 0
    ///         and "skill_manage" in agent.valid_tool_names):
    ///     agent._iters_since_skill += 1
    /// ```
    ///
    /// The trigger check (`>= skill_nudge_interval`) and reset happen in
    /// [`Self::check_skill_trigger_after_turn`], matching Hermes's deferred check at
    /// `conversation_loop.py:4643-4648` (after the agent loop completes).
    pub fn on_tool_batch_complete(&mut self, tools: &NudgeToolAvailability) {
        let gate_open = self.config.skill_nudge_interval > 0 && tools.has_skill_manage_tool;

        if !gate_open {
            return;
        }

        self.iters_since_skill = self.iters_since_skill.saturating_add(1);
    }

    /// Deferred skill-trigger check, called after the turn's agent loop finishes.
    ///
    /// Aligns with Hermes `conversation_loop.py:4643-4648`:
    /// ```text
    /// _should_review_skills = False
    /// if (agent._skill_nudge_interval > 0
    ///         and agent._iters_since_skill >= agent._skill_nudge_interval
    ///         and "skill_manage" in agent.valid_tool_names):
    ///     _should_review_skills = True
    ///     agent._iters_since_skill = 0
    /// ```
    pub fn check_skill_trigger_after_turn(&mut self, tools: &NudgeToolAvailability) {
        let gate_open = self.config.skill_nudge_interval > 0
            && self.iters_since_skill >= self.config.skill_nudge_interval
            && tools.has_skill_manage_tool;

        if gate_open {
            self.should_review_skills = true;
            self.iters_since_skill = 0;
        }
    }

    /// Consume and return the pending review flags, resetting them to `false`.
    ///
    /// The coordinator calls this after the turn finishes to decide whether to
    /// spawn a background review.
    pub fn drain_pending_reviews(&mut self) -> ReviewTrigger {
        let memory = self.should_review_memory;
        let skills = self.should_review_skills;
        self.should_review_memory = false;
        self.should_review_skills = false;
        ReviewTrigger {
            review_memory: memory,
            review_skills: skills,
        }
    }

    // ── Accessors (for testing and inspection) ──────────────────

    pub fn turns_since_memory(&self) -> u32 {
        self.turns_since_memory
    }

    pub fn iters_since_skill(&self) -> u32 {
        self.iters_since_skill
    }

    pub fn should_review_memory(&self) -> bool {
        self.should_review_memory
    }

    pub fn should_review_skills(&self) -> bool {
        self.should_review_skills
    }
}

/// Snapshot of review trigger flags, returned by [`ReactLoop::drain_pending_reviews`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewTrigger {
    pub review_memory: bool,
    pub review_skills: bool,
}

impl ReviewTrigger {
    pub fn any(&self) -> bool {
        self.review_memory || self.review_skills
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> ReactBuildConfig {
        ReactBuildConfig::from_env()
    }

    fn tools_both() -> NudgeToolAvailability {
        NudgeToolAvailability {
            has_memory_tool: true,
            has_skill_manage_tool: true,
        }
    }

    fn tools_none() -> NudgeToolAvailability {
        NudgeToolAvailability::default()
    }

    // ── Test 4: nudge does not fire when memory tool not registered ──
    #[test]
    fn nudge_does_not_fire_when_tool_not_registered() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 10;
        let mut rl = ReactLoop::new(cfg, true); // store exists
        for _ in 0..20 {
            rl.on_user_turn_start(&tools_none());
        }
        assert!(!rl.should_review_memory());
        assert_eq!(rl.turns_since_memory(), 0);
    }

    // ── Test 5: nudge does not fire when store missing ──
    #[test]
    fn nudge_does_not_fire_when_store_missing() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 10;
        let mut rl = ReactLoop::new(cfg, false); // no store
        for _ in 0..20 {
            rl.on_user_turn_start(&tools_both());
        }
        assert!(!rl.should_review_memory());
        assert_eq!(rl.turns_since_memory(), 0);
    }

    // ── Test 6: nudge resets counter after fire ──
    #[test]
    fn nudge_resets_counter_after_fire() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 5;
        let mut rl = ReactLoop::new(cfg, true);
        for _ in 0..5 {
            rl.on_user_turn_start(&tools_both());
        }
        assert!(rl.should_review_memory());
        assert_eq!(rl.turns_since_memory(), 0); // reset
    }

    // ── Test 7: memory nudge fires at interval, repeats ──
    #[test]
    fn memory_nudge_fires_at_interval() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 10;
        let mut rl = ReactLoop::new(cfg, true);

        // First 9 turns: no fire
        for _ in 0..9 {
            rl.on_user_turn_start(&tools_both());
            assert!(!rl.should_review_memory());
        }
        // 10th turn: fires
        rl.on_user_turn_start(&tools_both());
        assert!(rl.should_review_memory());
        assert_eq!(rl.turns_since_memory(), 0);

        // Next 10 turns fire again
        rl.drain_pending_reviews(); // clear flag
        for _ in 0..9 {
            rl.on_user_turn_start(&tools_both());
            assert!(!rl.should_review_memory());
        }
        rl.on_user_turn_start(&tools_both());
        assert!(rl.should_review_memory());
    }

    // ── Test 8: skill nudge counts tool iterations ──
    #[test]
    fn skill_nudge_counts_tool_iterations() {
        let mut cfg = base_config();
        cfg.skill_nudge_interval = 10;
        let mut rl = ReactLoop::new(cfg, false);

        // Simulate 10 tool batches within a single turn
        for _ in 0..10 {
            rl.on_tool_batch_complete(&tools_both());
        }
        rl.check_skill_trigger_after_turn(&tools_both());
        assert!(rl.should_review_skills());
        assert_eq!(rl.iters_since_skill(), 0); // reset
    }

    // ── Test 9: background review disables nudges via interval zero ──
    #[test]
    fn background_review_disables_nudges_via_interval_zero() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 0; // review-agent sets these to 0
        cfg.skill_nudge_interval = 0;
        cfg.is_background_review = true;
        let mut rl = ReactLoop::new(cfg, true);

        for _ in 0..100 {
            rl.on_user_turn_start(&tools_both());
            rl.on_tool_batch_complete(&tools_both());
        }
        rl.check_skill_trigger_after_turn(&tools_both());
        assert!(!rl.should_review_memory());
        assert!(!rl.should_review_skills());
    }

    // ── Test 10: nudge hydration from history ──
    #[test]
    fn nudge_hydration_from_history() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 10;
        let mut rl = ReactLoop::new(cfg, true);
        rl.init_from_history(15, 0); // 15 prior user turns, 0 tool iters
                                     // 15 % 10 = 5
        assert_eq!(rl.turns_since_memory(), 5);
        // Next 5 turns fire (5 + 5 = 10)
        for _ in 0..4 {
            rl.on_user_turn_start(&tools_both());
            assert!(!rl.should_review_memory());
        }
        rl.on_user_turn_start(&tools_both());
        assert!(rl.should_review_memory());
    }

    // ── Test 10b: hydration does not fire immediately on resume ──
    #[test]
    fn hydration_does_not_fire_immediately_on_resume() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 10;
        let mut rl = ReactLoop::new(cfg, true);
        rl.init_from_history(10, 0); // exactly at boundary → 10 % 10 = 0
        assert_eq!(rl.turns_since_memory(), 0);
        // 10 more turns needed to fire
        for _ in 0..9 {
            rl.on_user_turn_start(&tools_both());
            assert!(!rl.should_review_memory());
        }
        rl.on_user_turn_start(&tools_both());
        assert!(rl.should_review_memory());
    }

    // ── Test 12: nudge disabled (interval=0) never injects ──
    #[test]
    fn nudge_disabled_never_injects() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 0;
        cfg.skill_nudge_interval = 0;
        let mut rl = ReactLoop::new(cfg, true);
        for _ in 0..50 {
            rl.on_user_turn_start(&tools_both());
            rl.on_tool_batch_complete(&tools_both());
        }
        rl.check_skill_trigger_after_turn(&tools_both());
        assert!(!rl.should_review_memory());
        assert!(!rl.should_review_skills());
    }

    // ── Drain resets flags ──
    #[test]
    fn drain_pending_reviews_resets_flags() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 1;
        cfg.skill_nudge_interval = 1;
        let mut rl = ReactLoop::new(cfg, true);
        rl.on_user_turn_start(&tools_both());
        rl.on_tool_batch_complete(&tools_both());
        rl.check_skill_trigger_after_turn(&tools_both());
        assert!(rl.should_review_memory());
        assert!(rl.should_review_skills());

        let trigger = rl.drain_pending_reviews();
        assert!(trigger.review_memory);
        assert!(trigger.review_skills);
        assert!(trigger.any());
        assert!(!rl.should_review_memory());
        assert!(!rl.should_review_skills());
    }

    // ── Skill counter accumulates across turns without fire ──
    #[test]
    fn skill_counter_accumulates_across_turns_without_fire() {
        let mut cfg = base_config();
        cfg.skill_nudge_interval = 5;
        let mut rl = ReactLoop::new(cfg, false);

        // Turn 1: 3 tool batches
        for _ in 0..3 {
            rl.on_tool_batch_complete(&tools_both());
        }
        rl.check_skill_trigger_after_turn(&tools_both());
        assert!(!rl.should_review_skills());
        assert_eq!(rl.iters_since_skill(), 3);

        // Turn 2: 2 more → total 5 → fires
        for _ in 0..2 {
            rl.on_tool_batch_complete(&tools_both());
        }
        rl.check_skill_trigger_after_turn(&tools_both());
        assert!(rl.should_review_skills());
        assert_eq!(rl.iters_since_skill(), 0);
    }

    // ── Skill nudge does not fire when skill_manage not registered ──
    #[test]
    fn skill_nudge_no_fire_when_tool_not_registered() {
        let mut cfg = base_config();
        cfg.skill_nudge_interval = 5;
        let mut rl = ReactLoop::new(cfg, false);
        for _ in 0..10 {
            rl.on_tool_batch_complete(&tools_none()); // no skill_manage
        }
        rl.check_skill_trigger_after_turn(&tools_none());
        assert!(!rl.should_review_skills());
    }

    // ── Tool availability parsing ──
    #[test]
    fn tool_availability_from_names() {
        let av = NudgeToolAvailability::from_tool_names(["bash", "read", "memory"]);
        assert!(av.has_memory_tool);
        assert!(!av.has_skill_manage_tool);

        let av2 = NudgeToolAvailability::from_tool_names(["skill_manage", "todo_write"]);
        assert!(!av2.has_memory_tool);
        assert!(av2.has_skill_manage_tool);

        let av3 = NudgeToolAvailability::from_tool_names(["bash", "grep"]);
        assert!(!av3.has_memory_tool);
        assert!(!av3.has_skill_manage_tool);
    }

    // ── Hydration with zero prior turns is a no-op ──
    #[test]
    fn hydration_zero_prior_turns_is_noop() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 10;
        let mut rl = ReactLoop::new(cfg, true);
        rl.init_from_history(0, 0);
        assert_eq!(rl.turns_since_memory(), 0);
    }

    // ── Hydration is idempotent: second call does not overwrite ──
    #[test]
    fn hydration_is_idempotent_when_counter_already_set() {
        let mut cfg = base_config();
        cfg.memory_nudge_interval = 10;
        let mut rl = ReactLoop::new(cfg, true);
        rl.init_from_history(15, 0);
        assert_eq!(rl.turns_since_memory(), 5);
        // Simulate one turn passing
        rl.on_user_turn_start(&tools_both());
        assert_eq!(rl.turns_since_memory(), 6);
        // Second hydration should NOT reset (counter != 0)
        rl.init_from_history(15, 0);
        assert_eq!(rl.turns_since_memory(), 6);
    }

    // ── Skill counter hydration from history ──
    #[test]
    fn skill_counter_hydration_from_history() {
        let mut cfg = base_config();
        cfg.skill_nudge_interval = 30;
        let mut rl = ReactLoop::new(cfg, false);
        // 25 prior tool iterations, 25 % 30 = 25
        rl.init_from_history(0, 25);
        assert_eq!(rl.iters_since_skill(), 25);
        // Next 5 tool batches → total 30 → fires
        for _ in 0..5 {
            rl.on_tool_batch_complete(&tools_both());
        }
        rl.check_skill_trigger_after_turn(&tools_both());
        assert!(rl.should_review_skills());
        assert_eq!(rl.iters_since_skill(), 0);
    }

    // ── Skill counter hydration at boundary does not fire immediately ──
    #[test]
    fn skill_counter_hydration_boundary_no_immediate_fire() {
        let mut cfg = base_config();
        cfg.skill_nudge_interval = 30;
        let mut rl = ReactLoop::new(cfg, false);
        // 30 prior tool iterations, 30 % 30 = 0 → needs full 30 more
        rl.init_from_history(0, 30);
        assert_eq!(rl.iters_since_skill(), 0);
        for _ in 0..29 {
            rl.on_tool_batch_complete(&tools_both());
        }
        rl.check_skill_trigger_after_turn(&tools_both());
        assert!(!rl.should_review_skills());
    }

    // ── Skill counter hydration is idempotent ──
    #[test]
    fn skill_counter_hydration_is_idempotent() {
        let mut cfg = base_config();
        cfg.skill_nudge_interval = 30;
        let mut rl = ReactLoop::new(cfg, false);
        rl.init_from_history(0, 25);
        assert_eq!(rl.iters_since_skill(), 25);
        // One batch passes
        rl.on_tool_batch_complete(&tools_both());
        assert_eq!(rl.iters_since_skill(), 26);
        // Second hydration should NOT reset (counter != 0)
        rl.init_from_history(0, 25);
        assert_eq!(rl.iters_since_skill(), 26);
    }
}
