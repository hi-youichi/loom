//! Memory config block for run config summary.
//!
//! Implements [`ConfigSection`](super::ConfigSection). Distinguishes short-term
//! (checkpointer) and long-term (store). Does not display user_id (per-invoke runtime info).
//! Used by CLI to build the "Memory config" line.

use super::ConfigSection;

/// Memory configuration summary: mode, short_term (checkpointer), long_term (store).
///
/// Mode is one of `"none"`, `"short_term"`, `"long_term"`, `"both"`. When long-term
/// is vector store, output key for the store implementation is `"store"` (e.g. in_memory_vector).
/// user_id is not included in entries (per-invoke context).
pub struct MemoryConfigSummary {
    /// `"none"` | `"short_term"` | `"long_term"` | `"both"`.
    pub mode: String,
    /// Short-term backend, e.g. `"sqlite"`; `None` when no short-term.
    pub short_term: Option<String>,
    /// Thread ID when short-term or both.
    pub thread_id: Option<String>,
    /// Effective db path (e.g. `"memory.db"` when default).
    pub db_path: Option<String>,
    /// Long-term type: `"none"` or `"vector"`.
    pub long_term: Option<String>,
    /// When long-term is vector, store implementation (e.g. `in_memory_vector`, `sqlite_vec`, `lance`).
    /// Displayed in entries under key `"store"`.
    pub long_term_store: Option<String>,
}

impl ConfigSection for MemoryConfigSummary {
    fn section_name(&self) -> &str {
        "Memory config"
    }

    fn entries(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![("mode", self.mode.clone())];
        if let Some(ref st) = self.short_term {
            out.push(("short_term", st.clone()));
        }
        if let Some(ref t) = self.thread_id {
            out.push(("thread_id", t.clone()));
        }
        if let Some(ref p) = self.db_path {
            out.push(("db_path", p.clone()));
        }
        if let Some(ref lt) = self.long_term {
            out.push(("long_term", lt.clone()));
        }
        if let Some(ref s) = self.long_term_store {
            out.push(("store", s.clone()));
        }
        out
    }
}
