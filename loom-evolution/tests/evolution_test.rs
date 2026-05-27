use loom_evolution::*;

// ── Types tests ──

#[test]
fn difficulty_default_is_medium() {
    assert_eq!(types::Difficulty::default(), types::Difficulty::Medium);
}

#[test]
fn difficulty_serde_roundtrip() {
    let d = types::Difficulty::Easy;
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(json, "\"easy\"");
    let de: types::Difficulty = serde_json::from_str(&json).unwrap();
    assert_eq!(de, types::Difficulty::Easy);
}

#[test]
fn difficulty_all_variants() {
    let easy: types::Difficulty = serde_json::from_str("\"easy\"").unwrap();
    let medium: types::Difficulty = serde_json::from_str("\"medium\"").unwrap();
    let hard: types::Difficulty = serde_json::from_str("\"hard\"").unwrap();
    assert_eq!(easy, types::Difficulty::Easy);
    assert_eq!(medium, types::Difficulty::Medium);
    assert_eq!(hard, types::Difficulty::Hard);
}

#[test]
fn split_serde() {
    let json = serde_json::to_string(&types::Split::Train).unwrap();
    assert_eq!(json, "\"train\"");
    let json = serde_json::to_string(&types::Split::Val).unwrap();
    assert_eq!(json, "\"val\"");
    let json = serde_json::to_string(&types::Split::Holdout).unwrap();
    assert_eq!(json, "\"holdout\"");
}

#[test]
fn eval_example_serde_roundtrip() {
    let ex = types::EvalExample {
        task_input: "Fix bug".to_string(),
        expected_behavior: "Should fix the bug".to_string(),
        difficulty: types::Difficulty::Hard,
    };
    let json = serde_json::to_string(&ex).unwrap();
    let de: types::EvalExample = serde_json::from_str(&json).unwrap();
    assert_eq!(de.task_input, "Fix bug");
    assert_eq!(de.expected_behavior, "Should fix the bug");
    assert_eq!(de.difficulty, types::Difficulty::Hard);
}

#[test]
fn eval_example_default_difficulty() {
    let json = r#"{"task_input":"test","expected_behavior":"test"}"#;
    let ex: types::EvalExample = serde_json::from_str(json).unwrap();
    assert_eq!(ex.difficulty, types::Difficulty::Medium);
}

#[test]
fn constraint_result_serde() {
    let r = types::ConstraintResult {
        name: "size_budget".to_string(),
        passed: true,
        message: "OK".to_string(),
    };
    let json = serde_json::to_string(&r).unwrap();
    let de: types::ConstraintResult = serde_json::from_str(&json).unwrap();
    assert_eq!(de.name, "size_budget");
    assert!(de.passed);
}

#[test]
fn constraint_config_defaults() {
    let config = types::ConstraintConfig::default();
    assert!((config.max_size_ratio - 1.2).abs() < f64::EPSILON);
    assert!((config.min_semantic_similarity - 0.7).abs() < f64::EPSILON);
    assert!(!config.check_semantic);
}

#[test]
fn rubric_score_fitness_with_default_weights() {
    let score = types::RubricScore {
        procedure_followed: 1.0,
        output_quality: 1.0,
        conciseness: 1.0,
    };
    let weights = types::RubricWeights::default();
    let fitness = score.fitness(&weights);
    assert!((fitness - 1.0).abs() < f64::EPSILON);
}

#[test]
fn rubric_score_fitness_with_zero_weights() {
    let score = types::RubricScore {
        procedure_followed: 1.0,
        output_quality: 1.0,
        conciseness: 1.0,
    };
    let weights = types::RubricWeights {
        procedure: 0.0,
        quality: 0.0,
        conciseness: 0.0,
    };
    let fitness = score.fitness(&weights);
    assert!((fitness - 0.0).abs() < f64::EPSILON);
}

#[test]
fn rubric_score_fitness_partial() {
    let score = types::RubricScore {
        procedure_followed: 0.5,
        output_quality: 0.8,
        conciseness: 1.0,
    };
    let weights = types::RubricWeights::default();
    // 0.5*0.3 + 0.8*0.5 + 1.0*0.2 = 0.15 + 0.4 + 0.2 = 0.75
    let fitness = score.fitness(&weights);
    assert!((fitness - 0.75).abs() < 0.001);
}

#[test]
fn rubric_weights_default() {
    let w = types::RubricWeights::default();
    assert!((w.procedure - 0.3).abs() < f64::EPSILON);
    assert!((w.quality - 0.5).abs() < f64::EPSILON);
    assert!((w.conciseness - 0.2).abs() < f64::EPSILON);
}

#[test]
fn candidate_serde() {
    let c = types::Candidate {
        id: "c-1".to_string(),
        content: "skill content".to_string(),
        generation: 3,
        parent_id: Some("c-0".to_string()),
    };
    let json = serde_json::to_string(&c).unwrap();
    let de: types::Candidate = serde_json::from_str(&json).unwrap();
    assert_eq!(de.id, "c-1");
    assert_eq!(de.generation, 3);
    assert_eq!(de.parent_id, Some("c-0".to_string()));
}

#[test]
fn execution_trace_serde() {
    let t = types::ExecutionTrace {
        candidate_id: "c-1".to_string(),
        task_input: "Do something".to_string(),
        skill_text: "The skill".to_string(),
        agent_response: "I did it".to_string(),
        score: 0.85,
        score_breakdown: types::RubricScore {
            procedure_followed: 0.9,
            output_quality: 0.8,
            conciseness: 0.85,
        },
        failure_analysis: Some("Minor issue".to_string()),
    };
    let json = serde_json::to_string(&t).unwrap();
    let de: types::ExecutionTrace = serde_json::from_str(&json).unwrap();
    assert!((de.score - 0.85).abs() < 0.001);
    assert_eq!(de.candidate_id, "c-1");
}

#[test]
fn evolution_config_defaults() {
    let config = types::EvolutionConfig::default();
    assert_eq!(config.max_iterations, 10);
    assert_eq!(config.candidates_per_iter, 5);
    assert!((config.max_cost_usd - 10.0).abs() < f64::EPSILON);
    assert!(config.dataset_path.is_none());
}

// ── Constraint tests ──

#[test]
fn constraints_size_budget_passes_when_within_ratio() {
    let baseline = "---\nname: test\ndescription: test\n---\nShort";
    let evolved = "---\nname: test\ndescription: test\n---\nA bit longer";
    let config = types::ConstraintConfig::default();
    let results = check_constraints(evolved, baseline, &config);
    let size = results.iter().find(|r| r.name == "size_budget").unwrap();
    assert!(size.passed);
}

#[test]
fn constraints_size_budget_fails_when_exceeds_ratio() {
    let baseline = "short";
    let evolved = "x".repeat(200);
    let config = types::ConstraintConfig::default();
    let results = check_constraints(&evolved, baseline, &config);
    let size = results.iter().find(|r| r.name == "size_budget").unwrap();
    assert!(!size.passed);
}

#[test]
fn constraints_size_budget_zero_baseline() {
    let evolved = "some content";
    let config = types::ConstraintConfig::default();
    let results = check_constraints(evolved, "", &config);
    let size = results.iter().find(|r| r.name == "size_budget").unwrap();
    assert!(size.passed); // ratio defaults to 1.0 when baseline is 0
}

#[test]
fn constraints_semantic_check_placeholder() {
    let baseline = "---\nname: t\ndescription: t\n---\nHello error safety";
    let evolved = "---\nname: t\ndescription: t\n---\nHello error safety";
    let config = types::ConstraintConfig {
        check_semantic: true,
        ..Default::default()
    };
    let results = check_constraints(evolved, baseline, &config);
    let semantic = results.iter().find(|r| r.name == "semantic_preservation").unwrap();
    assert!(semantic.passed);
}

#[test]
fn constraints_structure_missing_frontmatter() {
    let baseline = "---\nname: t\ndescription: t\n---\nHello";
    let evolved = "No frontmatter at all. But has error.";
    let config = types::ConstraintConfig::default();
    let results = check_constraints(evolved, baseline, &config);
    let structure = results.iter().find(|r| r.name == "structure_integrity").unwrap();
    assert!(!structure.passed);
}

#[test]
fn constraints_safety_keywords_preserved() {
    let baseline = "---\nname: t\ndescription: t\n---\nWarning: be careful. Error handling required.";
    let evolved = "---\nname: t\ndescription: t\n---\nWarning: still careful. Error handling present.";
    let config = types::ConstraintConfig::default();
    let results = check_constraints(evolved, baseline, &config);
    let safety = results.iter().find(|r| r.name == "safety_preservation").unwrap();
    assert!(safety.passed);
}

#[test]
fn constraints_safety_keywords_removed() {
    let baseline = "---\nname: t\ndescription: t\n---\nWarning: be careful with error handling";
    let evolved = "---\nname: t\ndescription: t\n---\nJust do it";
    let config = types::ConstraintConfig::default();
    let results = check_constraints(evolved, baseline, &config);
    let safety = results.iter().find(|r| r.name == "safety_preservation").unwrap();
    assert!(!safety.passed);
}

#[test]
fn constraints_custom_size_ratio() {
    let config = types::ConstraintConfig {
        max_size_ratio: 100.0, // very high ratio
        ..Default::default()
    };
    let baseline = "short"; // 5 bytes
    let evolved = &"x".repeat(200); // 200 bytes, ratio = 200/5 = 40
    let results = check_constraints(evolved, baseline, &config);
    let size = results.iter().find(|r| r.name == "size_budget").unwrap();
    assert!(size.passed); // with ratio 100, 40 < 100 passes
}

// ── Judge tests ──

#[test]
fn judge_parse_valid_json() {
    let resp = r#"{"procedure_followed": 0.8, "output_quality": 0.7, "conciseness": 0.9, "reasoning": "OK"}"#;
    let score = judge::parse_judge_response(resp).unwrap();
    assert!((score.procedure_followed - 0.8).abs() < 0.01);
    assert!((score.output_quality - 0.7).abs() < 0.01);
    assert!((score.conciseness - 0.9).abs() < 0.01);
}

#[test]
fn judge_parse_with_markdown_wrapper() {
    let resp = "```json\n{\"procedure_followed\": 0.5, \"output_quality\": 0.6, \"conciseness\": 0.7}\n```";
    let score = judge::parse_judge_response(resp).unwrap();
    assert!((score.procedure_followed - 0.5).abs() < 0.01);
}

#[test]
fn judge_parse_clamps_high() {
    let resp = r#"{"procedure_followed": 2.0, "output_quality": 1.5, "conciseness": 0.5}"#;
    let score = judge::parse_judge_response(resp).unwrap();
    assert!((score.procedure_followed - 1.0).abs() < 0.01);
    assert!((score.output_quality - 1.0).abs() < 0.01);
}

#[test]
fn judge_parse_clamps_low() {
    let resp = r#"{"procedure_followed": -0.5, "output_quality": 0.0, "conciseness": 0.0}"#;
    let score = judge::parse_judge_response(resp).unwrap();
    assert!((score.procedure_followed - 0.0).abs() < 0.01);
}

#[test]
fn judge_parse_invalid_returns_none() {
    let resp = "not json at all";
    assert!(judge::parse_judge_response(resp).is_none());
}

#[test]
fn judge_average_fitness_empty() {
    let weights = types::RubricWeights::default();
    assert!((judge::average_fitness(&[], &weights) - 0.0).abs() < f64::EPSILON);
}

#[test]
fn judge_average_fitness_calculation() {
    let weights = types::RubricWeights::default();
    let scores = vec![
        types::RubricScore {
            procedure_followed: 1.0,
            output_quality: 1.0,
            conciseness: 1.0,
        },
        types::RubricScore {
            procedure_followed: 0.0,
            output_quality: 0.0,
            conciseness: 0.0,
        },
    ];
    let avg = judge::average_fitness(&scores, &weights);
    assert!((avg - 0.5).abs() < 0.01);
}

#[test]
fn judge_prompt_contains_content() {
    let skill = "# My Skill\nDo things.";
    let example = types::EvalExample {
        task_input: "Fix bug".to_string(),
        expected_behavior: "Should fix the bug".to_string(),
        difficulty: types::Difficulty::Medium,
    };
    let prompt = judge::judge_prompt(skill, &example);
    assert!(prompt.contains("My Skill"));
    assert!(prompt.contains("Fix bug"));
    assert!(prompt.contains("Should fix the bug"));
}

#[test]
fn mutation_prompt_contains_failures() {
    let traces = vec![types::ExecutionTrace {
        candidate_id: "c-1".to_string(),
        task_input: "Task 1".to_string(),
        skill_text: "Skill".to_string(),
        agent_response: "Bad response".to_string(),
        score: 0.3,
        score_breakdown: types::RubricScore {
            procedure_followed: 0.2,
            output_quality: 0.3,
            conciseness: 0.4,
        },
        failure_analysis: Some("Bad quality".to_string()),
    }];
    let prompt = judge::mutation_prompt("baseline skill", &traces, 2);
    assert!(prompt.contains("baseline skill"));
    assert!(prompt.contains("Task 1"));
    assert!(prompt.contains("0.30"));
    assert!(prompt.contains("Bad quality"));
    assert!(prompt.contains("2"));
}

#[test]
fn failure_analysis_prompt_contains_trace_info() {
    let trace = types::ExecutionTrace {
        candidate_id: "c-1".to_string(),
        task_input: "Do the thing".to_string(),
        skill_text: "Skill instructions here".to_string(),
        agent_response: "I tried to do the thing".to_string(),
        score: 0.4,
        score_breakdown: types::RubricScore {
            procedure_followed: 0.3,
            output_quality: 0.5,
            conciseness: 0.4,
        },
        failure_analysis: None,
    };
    let prompt = judge::failure_analysis_prompt(&trace);
    assert!(prompt.contains("Do the thing"));
    assert!(prompt.contains("0.40"));
}
