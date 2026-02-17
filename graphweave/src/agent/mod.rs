//! Agent module: ReAct and other agent implementations.
//!
//! ReAct nodes (Think, Act, Observe), runner, and config-driven builder are unified
//! under [`react`](self::react). DUP, ToT, and GoT are under [`dup`](self::dup),
//! [`tot`](self::tot), and [`got`](self::got).

pub mod dup;
pub mod got;
pub mod react;
pub mod tot;

pub use dup::{build_dup_initial_state, DupRunError, DupRunner, DupState, UnderstandOutput};
pub use got::{
    build_got_initial_state, GotRunError, GotRunner, GotState, TaskGraph, TaskNode, TaskNodeState,
    TaskStatus,
};
pub use react::*;
pub use tot::{
    build_tot_initial_state, TotCandidate, TotExtension, TotRunError, TotRunner, TotState,
};
