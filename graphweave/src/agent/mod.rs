//! Agent module: ReAct and other agent implementations.
//!
//! ReAct nodes (Think, Act, Observe), runner, and config-driven builder are unified
//! under [`react`](self::react).

pub mod react;

pub use react::*;
