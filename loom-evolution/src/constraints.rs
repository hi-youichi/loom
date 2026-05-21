//! Constraint checking for evolved skills.

use crate::types::{ConstraintConfig, ConstraintResult};

/// Check all constraints on an evolved skill vs baseline.
pub fn check_constraints(
    evolved: &str,
    baseline: &str,
    config: &ConstraintConfig,
) -> Vec<ConstraintResult> {
    let mut results = Vec::new();

    // C1: Size budget
    let baseline_size = baseline.len();
    let evolved_size = evolved.len();
    let ratio = if baseline_size > 0 {
        evolved_size as f64 / baseline_size as f64
    } else {
        1.0
    };
    results.push(ConstraintResult {
        name: "size_budget".to_string(),
        passed: ratio <= config.max_size_ratio,
        message: format!(
            "Size ratio: {:.2} ({} -> {} bytes, limit {:.2})",
            ratio, baseline_size, evolved_size, config.max_size_ratio
        ),
    });

    // C2: Improvement threshold (checked separately with scoring)
    // C3: Semantic preservation (placeholder — needs embedding API)
    if config.check_semantic {
        results.push(ConstraintResult {
            name: "semantic_preservation".to_string(),
            passed: true, // Placeholder: would need embedding API
            message: "Semantic check placeholder (not yet implemented)".to_string(),
        });
    }

    // C4: Structure integrity — check YAML frontmatter
    results.push(check_structure(evolved));

    // C5: No safety regression — check that error handling / safety sections are present
    results.push(check_safety_preservation(evolved, baseline));

    results
}

/// Check that evolved skill has valid YAML frontmatter with required fields.
fn check_structure(evolved: &str) -> ConstraintResult {
    let required_fields = ["name", "description"];

    if !evolved.starts_with("---") {
        return ConstraintResult {
            name: "structure_integrity".to_string(),
            passed: false,
            message: "Missing YAML frontmatter (must start with ---)".to_string(),
        };
    }

    // Find second ---
    let rest = &evolved[3..];
    if !rest.starts_with('\n') && !rest.starts_with('\r') {
        return ConstraintResult {
            name: "structure_integrity".to_string(),
            passed: false,
            message: "Invalid frontmatter format".to_string(),
        };
    }

    let after_first = if let Some(stripped) = rest.strip_prefix("\r\n") {
        stripped
    } else {
        &rest[1..]
    };

    let Some(sep) = after_first.find("---") else {
        return ConstraintResult {
            name: "structure_integrity".to_string(),
            passed: false,
            message: "Unclosed YAML frontmatter (missing closing ---)".to_string(),
        };
    };

    let yaml_str = after_first[..sep].trim();

    let missing: Vec<&str> = required_fields
        .iter()
        .filter(|field| {
            // Simple check: does the YAML contain "field:" ?
            !yaml_str.contains(&format!("{}:", field))
        })
        .copied()
        .collect();

    ConstraintResult {
        name: "structure_integrity".to_string(),
        passed: missing.is_empty(),
        message: if missing.is_empty() {
            "All required fields present".to_string()
        } else {
            format!("Missing fields: {:?}", missing)
        },
    }
}

/// Check that safety-related sections weren't removed.
fn check_safety_preservation(evolved: &str, baseline: &str) -> ConstraintResult {
    let safety_keywords = ["error", "safety", "warning", "caution", "不", "禁止", "避免"];

    let baseline_lower = baseline.to_lowercase();
    let evolved_lower = evolved.to_lowercase();

    let present_in_baseline: Vec<&str> = safety_keywords
        .iter()
        .filter(|kw| baseline_lower.contains(*kw))
        .copied()
        .collect();

    let missing_in_evolved: Vec<&str> = present_in_baseline
        .iter()
        .filter(|kw| !evolved_lower.contains(*kw))
        .copied()
        .collect();

    ConstraintResult {
        name: "safety_preservation".to_string(),
        passed: missing_in_evolved.is_empty(),
        message: if missing_in_evolved.is_empty() {
            "Safety keywords preserved".to_string()
        } else {
            format!("Safety keywords removed: {:?}", missing_in_evolved)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline_skill() -> String {
        "---\nname: test-skill\ndescription: A test skill\n---\n# Instructions\nDo stuff.\n\n## Safety\nWarning: be careful with errors.\n".to_string()
    }

    #[test]
    fn check_constraints_valid_evolution() {
        let baseline = baseline_skill();
        let evolved = "---\nname: test-skill\ndescription: An improved test skill\n---\n# Better Instructions\nDo stuff well.\n\n## Safety\nWarning: be careful with errors.\n".to_string();
        let config = ConstraintConfig::default();
        let results = check_constraints(&evolved, &baseline, &config);
        assert!(results.iter().all(|r| r.passed), "All constraints should pass: {:?}", results);
    }

    #[test]
    fn size_budget_fails_when_too_large() {
        let baseline = baseline_skill();
        let evolved = format!(
            "---\nname: test-skill\ndescription: test\n---\n# Instructions\n{}\n\n## Safety\nWarning: errors.\n",
            "x".repeat(baseline.len() * 3)
        );
        let config = ConstraintConfig::default();
        let results = check_constraints(&evolved, &baseline, &config);
        let size_check = results.iter().find(|r| r.name == "size_budget").unwrap();
        assert!(!size_check.passed);
    }

    #[test]
    fn structure_fails_without_frontmatter() {
        let evolved = "# Just a heading\nSome content.";
        let result = check_structure(evolved);
        assert!(!result.passed);
    }

    #[test]
    fn structure_fails_with_missing_name() {
        let evolved = "---\ndescription: test\n---\nBody";
        let result = check_structure(evolved);
        assert!(!result.passed);
    }

    #[test]
    fn safety_detects_removed_warnings() {
        let baseline = baseline_skill();
        let evolved = "---\nname: test-skill\ndescription: test\n---\n# Instructions\nDo stuff.\n".to_string();
        let config = ConstraintConfig::default();
        let results = check_constraints(&evolved, &baseline, &config);
        let safety = results.iter().find(|r| r.name == "safety_preservation").unwrap();
        assert!(!safety.passed);
    }
}
