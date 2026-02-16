//! Config section trait and run config summary aggregate.
//!
//! [`ConfigSection`] is implemented by [`LlmConfigSummary`], [`MemoryConfigSummary`],
//! [`ToolConfigSummary`], and [`EmbeddingConfigSummary`]. [`RunConfigSummary`] holds
//! multiple sections and prints them in order (e.g. to stderr when verbose).

use std::io::Write;

mod embedding;
mod llm;
mod memory;
mod tools;

pub use embedding::EmbeddingConfigSummary;
pub use llm::LlmConfigSummary;
pub use memory::MemoryConfigSummary;
pub use tools::ToolConfigSummary;

/// One block of run config (LLM, memory, tools, embedding) for display and printing.
///
/// Callers use [`section_name`](ConfigSection::section_name) and [`entries`](ConfigSection::entries)
/// to read config programmatically; [`print_to_stderr`](ConfigSection::print_to_stderr) writes
/// one line to stderr in a uniform format. Printing is best-effort (errors are ignored).
pub trait ConfigSection: Send + Sync {
    /// Section label, e.g. `"LLM config"`, `"Memory config"`, `"Tools"`.
    fn section_name(&self) -> &str;
    /// Key-value pairs (no secrets). Keys are `&'static str` for use in display and tests.
    fn entries(&self) -> Vec<(&'static str, String)>;
    /// Print one line to stderr in the form `[section_name] k1=v1 k2=v2 ...`. Best-effort.
    fn print_to_stderr(&self) {
        let entries: Vec<String> = self
            .entries()
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        let _ = writeln!(
            std::io::stderr(),
            "[{}] {}",
            self.section_name(),
            entries.join(" ")
        );
        let _ = std::io::stderr().flush();
    }
}

/// Aggregated run config summary (LLM, memory, tools, embedding sections).
///
/// Built via [`RunConfigSummary::new()`](RunConfigSummary::new) and
/// [`with_section`](RunConfigSummary::with_section). Call [`print_to_stderr`](RunConfigSummary::print_to_stderr)
/// to emit all sections in order.
pub struct RunConfigSummary {
    sections: Vec<Box<dyn ConfigSection>>,
}

impl RunConfigSummary {
    /// Creates an empty summary.
    pub fn new() -> Self {
        Self { sections: vec![] }
    }

    /// Appends a section and returns `self` for chaining.
    pub fn with_section(mut self, s: Box<dyn ConfigSection>) -> Self {
        self.sections.push(s);
        self
    }

    /// Returns the list of sections in order.
    pub fn sections(&self) -> &[Box<dyn ConfigSection>] {
        self.sections.as_slice()
    }

    /// Prints each section to stderr, one line per section. Best-effort.
    pub fn print_to_stderr(&self) {
        for s in &self.sections {
            s.print_to_stderr();
        }
    }
}

impl Default for RunConfigSummary {
    fn default() -> Self {
        Self::new()
    }
}

/// Source of the four config sections used to build a [`RunConfigSummary`].
///
/// Implement this trait for your run config type so that
/// [`build_config_summary`] can produce a summary (e.g. for verbose logging).
/// CLI crates with a RunConfig-like type can implement this trait.
pub trait RunConfigSummarySource: Send + Sync {
    /// LLM section (model, api_base, temperature, tool_choice).
    fn llm_section(&self) -> LlmConfigSummary;
    /// Memory section (mode, short_term, long_term, store).
    fn memory_section(&self) -> MemoryConfigSummary;
    /// Tools section (sources, exa_url).
    fn tools_section(&self) -> ToolConfigSummary;
    /// Embedding section (model, api_base).
    fn embedding_section(&self) -> EmbeddingConfigSummary;
}

/// Builds a run config summary from any source that implements [`RunConfigSummarySource`].
///
/// Call [`RunConfigSummary::print_to_stderr`] on the result to print the summary
/// (e.g. when `--verbose` is set). Used by the CLI and by other crates that have
/// a config type implementing the trait.
pub fn build_config_summary(source: &impl RunConfigSummarySource) -> RunConfigSummary {
    RunConfigSummary::new()
        .with_section(Box::new(source.llm_section()))
        .with_section(Box::new(source.memory_section()))
        .with_section(Box::new(source.tools_section()))
        .with_section(Box::new(source.embedding_section()))
}
