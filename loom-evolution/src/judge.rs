//! LLM-as-judge scoring for skill evaluation.

use crate::types::{EvalExample, RubricScore, RubricWeights};
use crate::types::ExecutionTrace;

/// Generate the LLM-judge prompt for evaluating a skill against an example.
pub fn judge_prompt(skill_text: &str, example: &EvalExample) -> String {
    format!(
        r#"你是一个技能评估专家。评估以下技能在处理给定任务时的表现。

## 技能文件
{skill_text}

## 任务输入
{task_input}

## 期望行为（评分标准）
{expected_behavior}

请评估该技能在此任务上的表现，输出以下 JSON 格式：
{{"procedure_followed": 0.0-1.0, "output_quality": 0.0-1.0, "conciseness": 0.0-1.0, "reasoning": "简短说明"}}

评分维度：
- procedure_followed: 技能的步骤和流程是否被正确遵循 (0-1)
- output_quality: 输出是否正确、有用、解决了问题 (0-1)  
- conciseness: 输出是否简洁，没有冗余信息 (0-1)
- reasoning: 简短说明评分理由

请直接输出 JSON，不要其他内容。"#,
        skill_text = skill_text,
        task_input = example.task_input,
        expected_behavior = example.expected_behavior,
    )
}

/// Generate the candidate mutation prompt for GEPA optimization.
pub fn mutation_prompt(
    baseline_skill: &str,
    failed_traces: &[ExecutionTrace],
    iteration: u32,
) -> String {
    let failures_summary: String = failed_traces
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, t)| {
            format!(
                "### 失败案例 {}\n- 任务: {}\n- 评分: {:.2}\n- 分析: {}\n",
                i + 1,
                t.task_input,
                t.score,
                t.failure_analysis.as_deref().unwrap_or("无"),
            )
        })
        .collect();

    format!(
        r#"你是一个技能优化专家。根据失败案例分析，改进以下技能文件。

## 当前技能文件（第 {iteration} 轮优化）
{baseline_skill}

## 失败案例分析
{failures_summary}

请基于失败原因分析，生成一个改进版的技能文件。要求：
1. 保持 YAML frontmatter 格式（name, description 字段）
2. 针对失败案例中暴露的问题进行改进
3. 不要增加过多内容（不超过原文件的 1.2 倍）
4. 保持已有的安全相关内容

直接输出改进后的完整技能文件内容，不要其他说明。"#,
        iteration = iteration,
        baseline_skill = baseline_skill,
        failures_summary = failures_summary,
    )
}

/// Parse the judge response into a RubricScore.
pub fn parse_judge_response(response: &str) -> Option<RubricScore> {
    // Try to extract JSON from the response
    let json_str = response
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    #[derive(serde::Deserialize)]
    struct JudgeResponse {
        procedure_followed: f64,
        output_quality: f64,
        conciseness: f64,
        #[allow(dead_code)]
        reasoning: Option<String>,
    }

    let parsed: JudgeResponse = serde_json::from_str(json_str).ok()?;
    Some(RubricScore {
        procedure_followed: parsed.procedure_followed.clamp(0.0, 1.0),
        output_quality: parsed.output_quality.clamp(0.0, 1.0),
        conciseness: parsed.conciseness.clamp(0.0, 1.0),
    })
}

/// Calculate the average fitness score across multiple evaluations.
pub fn average_fitness(scores: &[RubricScore], weights: &RubricWeights) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }
    scores.iter().map(|s| s.fitness(weights)).sum::<f64>() / scores.len() as f64
}

/// Generate failure analysis prompt.
pub fn failure_analysis_prompt(trace: &ExecutionTrace) -> String {
    format!(
        r#"分析以下技能执行失败的原因：

## 任务
{task_input}

## 技能内容（摘要前 500 字符）
{skill_summary}

## Agent 响应
{agent_response}

## 评分
- 流程遵循: {proc:.2}
- 输出质量: {quality:.2}
- 简洁性: {concise:.2}
- 综合: {score:.2}

请用 1-2 句话分析失败原因。"#,
        task_input = trace.task_input,
        skill_summary = &trace.skill_text[..trace.skill_text.len().min(500)],
        agent_response = &trace.agent_response[..trace.agent_response.len().min(500)],
        proc = trace.score_breakdown.procedure_followed,
        quality = trace.score_breakdown.output_quality,
        concise = trace.score_breakdown.conciseness,
        score = trace.score,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_judge_response() {
        let response = r#"{"procedure_followed": 0.8, "output_quality": 0.7, "conciseness": 0.9, "reasoning": "Good"}"#;
        let score = parse_judge_response(response).unwrap();
        assert!((score.procedure_followed - 0.8).abs() < 0.01);
        assert!((score.output_quality - 0.7).abs() < 0.01);
        assert!((score.conciseness - 0.9).abs() < 0.01);
    }

    #[test]
    fn parse_judge_response_with_markdown() {
        let response = "```json\n{\"procedure_followed\": 0.5, \"output_quality\": 0.6, \"conciseness\": 0.7}\n```";
        let score = parse_judge_response(response).unwrap();
        assert!((score.procedure_followed - 0.5).abs() < 0.01);
    }

    #[test]
    fn parse_judge_response_clamps_values() {
        let response = r#"{"procedure_followed": 1.5, "output_quality": -0.1, "conciseness": 0.5}"#;
        let score = parse_judge_response(response).unwrap();
        assert!((score.procedure_followed - 1.0).abs() < 0.01);
        assert!((score.output_quality - 0.0).abs() < 0.01);
    }

    #[test]
    fn average_fitness_empty() {
        let weights = RubricWeights::default();
        assert_eq!(average_fitness(&[], &weights), 0.0);
    }

    #[test]
    fn average_fitness_calculation() {
        let weights = RubricWeights::default();
        let scores = vec![
            RubricScore {
                procedure_followed: 1.0,
                output_quality: 1.0,
                conciseness: 1.0,
            },
            RubricScore {
                procedure_followed: 0.0,
                output_quality: 0.0,
                conciseness: 0.0,
            },
        ];
        let avg = average_fitness(&scores, &weights);
        assert!((avg - 0.5).abs() < 0.01);
    }

    #[test]
    fn judge_prompt_contains_skill_and_example() {
        let skill = "# My Skill\nDo things.";
        let example = EvalExample {
            task_input: "Fix bug".to_string(),
            expected_behavior: "Should fix the bug".to_string(),
            difficulty: crate::types::Difficulty::Medium,
        };
        let prompt = judge_prompt(skill, &example);
        assert!(prompt.contains("My Skill"));
        assert!(prompt.contains("Fix bug"));
    }
}
