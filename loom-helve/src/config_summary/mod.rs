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

#[cfg(test)]
mod tests {
    use super::*;

    struct DummySection {
        name: &'static str,
        entries: Vec<(&'static str, String)>,
    }

    impl ConfigSection for DummySection {
        fn section_name(&self) -> &str {
            self.name
        }

        fn entries(&self) -> Vec<(&'static str, String)> {
            self.entries.clone()
        }
    }

    struct DummySource;

    impl RunConfigSummarySource for DummySource {
        fn llm_section(&self) -> LlmConfigSummary {
            LlmConfigSummary {
                model: "glm-5".to_string(),
                api_base: "https://api.example.com/v1".to_string(),
                temperature: Some(0.2),
                tool_choice: "auto".to_string(),
            }
        }

        fn memory_section(&self) -> MemoryConfigSummary {
            MemoryConfigSummary {
                mode: "short_term".to_string(),
                short_term: Some("sqlite".to_string()),
                thread_id: Some("thread-1".to_string()),
                db_path: Some("memory.db".to_string()),
                long_term: None,
                long_term_store: None,
            }
        }

        fn tools_section(&self) -> ToolConfigSummary {
            ToolConfigSummary {
                sources: vec!["memory".to_string(), "exa".to_string()],
                exa_url: Some("https://example.com/mcp".to_string()),
            }
        }

        fn embedding_section(&self) -> EmbeddingConfigSummary {
            EmbeddingConfigSummary {
                model: "text-embedding-3-small".to_string(),
                api_base: "https://api.example.com/v1".to_string(),
            }
        }
    }

    #[test]
    fn run_config_summary_new_and_default_are_empty() {
        assert!(RunConfigSummary::new().sections().is_empty());
        assert!(RunConfigSummary::default().sections().is_empty());
    }

    #[test]
    fn with_section_preserves_insertion_order() {
        let summary = RunConfigSummary::new()
            .with_section(Box::new(DummySection {
                name: "first",
                entries: vec![("k1", "v1".to_string())],
            }))
            .with_section(Box::new(DummySection {
                name: "second",
                entries: vec![("k2", "v2".to_string())],
            }));
        let names: Vec<&str> = summary
            .sections()
            .iter()
            .map(|s| s.section_name())
            .collect();
        assert_eq!(names, vec!["first", "second"]);
    }

    #[test]
    fn build_config_summary_includes_all_four_sections() {
        let summary = build_config_summary(&DummySource);
        let names: Vec<&str> = summary
            .sections()
            .iter()
            .map(|s| s.section_name())
            .collect();
        assert_eq!(
            names,
            vec!["LLM config", "Memory config", "Tools", "Embedding"]
        );
        summary.print_to_stderr();
    }

    #[test]
    fn config_section_print_to_stderr_is_best_effort() {
        let section = DummySection {
            name: "dummy",
            entries: vec![
                ("model", "glm-5".to_string()),
                ("temperature", "0.2".to_string()),
            ],
        };
        section.print_to_stderr();
        assert_eq!(section.entries().len(), 2);
    }
}
