//! Unit tests for LlmTool action dispatch and discovery handlers.

use std::sync::Arc;

use model_spec_core::ModelLimit;
use model_spec_core::Model;
use serde_json::{json, Value};

use tool_core::Tool;

use super::{LlmProviderData, LlmTool, LlmToolConfig, LlmToolData};

fn make_model(id: &str) -> Model {
    Model {
        id: id.to_string(),
        name: id.to_string(),
        limit: ModelLimit::new(128_000, 4096),
        ..Default::default()
    }
}

fn make_tool() -> LlmTool {
    let data = LlmToolData {
        default_provider: "openai".to_string(),
        default_model: "gpt-4o".to_string(),
        providers: vec![
            LlmProviderData {
                name: "openai".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "sk-test".to_string(),
                models: vec![make_model("gpt-4o"), make_model("gpt-4o-mini")],
            },
            LlmProviderData {
                name: "bigmodel".to_string(),
                base_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
                api_key: "bm-test".to_string(),
                models: vec![make_model("glm-4-flash")],
            },
        ],
    };
    LlmTool::new(Arc::new(data), None, LlmToolConfig::default())
}

/// Extract the JSON payload from a ToolCallContent::Text.
fn result_json(result: &loom_llm::ToolCallContent) -> Value {
    let text = result.as_text().expect("expected Text content");
    serde_json::from_str(text).unwrap_or_else(|e| panic!("invalid JSON in result: {e}\n{text}"))
}

// ── Discovery actions ──────────────────────────────────────────────────

#[tokio::test]
async fn test_handle_list_providers() {
    let tool = make_tool();
    let result = tool
        .call(json!({ "action": "list_providers" }), None)
        .await
        .unwrap();
    let v = result_json(&result);
    let providers = v["providers"].as_array().unwrap();
    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0]["name"], "openai");
    assert_eq!(providers[0]["models_count"], 2);
    assert_eq!(providers[1]["name"], "bigmodel");
    assert_eq!(providers[1]["models_count"], 1);
}

#[tokio::test]
async fn test_handle_list_models_default_provider() {
    let tool = make_tool();
    let result = tool
        .call(json!({ "action": "list_models" }), None)
        .await
        .unwrap();
    let v = result_json(&result);
    assert_eq!(v["provider"], "openai");
    let models = v["models"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    let ids: Vec<&str> = models.iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"gpt-4o"));
    assert!(ids.contains(&"gpt-4o-mini"));
}

#[tokio::test]
async fn test_handle_list_models_specific_provider() {
    let tool = make_tool();
    let result = tool
        .call(
            json!({ "action": "list_models", "provider": "bigmodel" }),
            None,
        )
        .await
        .unwrap();
    let v = result_json(&result);
    assert_eq!(v["provider"], "bigmodel");
    let models = v["models"].as_array().unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0]["id"], "glm-4-flash");
}

#[tokio::test]
async fn test_handle_list_models_unknown_provider() {
    let tool = make_tool();
    let err = tool
        .call(json!({ "action": "list_models", "provider": "nope" }), None)
        .await
        .unwrap_err();
    let s = err.to_string();
    assert!(s.contains("不存在"), "s = {s}");
}

#[tokio::test]
async fn test_handle_model_info_ok() {
    let tool = make_tool();
    let result = tool
        .call(
            json!({
                "action": "model_info",
                "provider": "bigmodel",
                "model": "glm-4-flash"
            }),
            None,
        )
        .await
        .unwrap();
    let v = result_json(&result);
    assert_eq!(v["id"], "glm-4-flash");
    assert_eq!(v["limit"]["context"], 128_000);
}

#[tokio::test]
async fn test_handle_model_info_not_found() {
    let tool = make_tool();
    let err = tool
        .call(
            json!({
                "action": "model_info",
                "provider": "bigmodel",
                "model": "gpt-4o"
            }),
            None,
        )
        .await
        .unwrap_err();
    let s = err.to_string();
    assert!(s.contains("不存在于 provider"), "s = {s}");
}

#[tokio::test]
async fn test_handle_model_info_missing_model_arg() {
    let tool = make_tool();
    let err = tool
        .call(
            json!({ "action": "model_info", "provider": "openai" }),
            None,
        )
        .await
        .unwrap_err();
    let s = err.to_string();
    assert!(s.contains("需要 model 参数"), "s = {s}");
}

#[tokio::test]
async fn test_call_unknown_action() {
    let tool = make_tool();
    let err = tool
        .call(json!({ "action": "delete_everything" }), None)
        .await
        .unwrap_err();
    let s = err.to_string();
    assert!(s.contains("未知 action"), "s = {s}");
}

// ── Invoke validation (no real HTTP) ───────────────────────────────────

#[tokio::test]
async fn test_handle_invoke_wrong_provider() {
    let tool = make_tool();
    let err = tool
        .call(
            json!({
                "provider": "nope",
                "messages": [{ "role": "user", "content": "hi" }]
            }),
            None,
        )
        .await
        .unwrap_err();
    let s = err.to_string();
    assert!(s.contains("provider"), "s = {s}");
    assert!(s.contains("不存在"), "s = {s}");
}

#[tokio::test]
async fn test_handle_invoke_missing_messages() {
    let tool = make_tool();
    let err = tool
        .call(json!({ "provider": "openai" }), None)
        .await
        .unwrap_err();
    let s = err.to_string();
    assert!(s.contains("缺少 messages"), "s = {s}");
}

#[tokio::test]
async fn test_handle_invoke_empty_messages() {
    let tool = make_tool();
    let err = tool
        .call(json!({ "provider": "openai", "messages": [] }), None)
        .await
        .unwrap_err();
    let s = err.to_string();
    assert!(s.contains("messages 为空"), "s = {s}");
}

// ── Spec & config ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_spec_includes_all_actions() {
    let tool = make_tool();
    let spec = tool.spec();
    assert_eq!(spec.name, "llm");
    let s = spec.input_schema.to_string();
    assert!(s.contains("list_providers"), "s = {s}");
    assert!(s.contains("list_models"), "s = {s}");
    assert!(s.contains("model_info"), "s = {s}");
    assert!(s.contains("input_audio"), "s = {s}");
    assert!(s.contains("image_url"), "s = {s}");
    assert!(s.contains("image_path"), "s = {s}");
}

#[tokio::test]
async fn test_default_config_limits() {
    let cfg = LlmToolConfig::default();
    assert_eq!(cfg.max_messages, 50);
    assert_eq!(cfg.max_text_chars, 100_000);
    assert_eq!(cfg.max_file_size, 10_000_000);
    assert!(cfg.allowed_models.is_none());
}

// ── _path content part tests ───────────────────────────────────────────

mod path_tests {
    use super::*;
    use crate::llm::content::resolve_content_part;
    use loom_llm::message::ContentPart;
    use std::io::Write;

    fn make_tool_with_wf(wf: &std::path::Path) -> LlmTool {
        let data = LlmToolData {
            default_provider: "test".to_string(),
            default_model: "m".to_string(),
            providers: vec![LlmProviderData {
                name: "test".to_string(),
                base_url: "http://localhost".to_string(),
                api_key: "k".to_string(),
                models: vec![],
            }],
        };
        LlmTool::new(
            Arc::new(data),
            Some(Arc::new(wf.to_path_buf())),
            LlmToolConfig::default(),
        )
    }

    fn write_tmp(dir: &std::path::Path, name: &str, data: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(data).unwrap();
        path
    }

    #[test]
    fn test_resolve_image_path() {
        let tmp = tempfile::tempdir().unwrap();
        let wf = tmp.path();
        let img = write_tmp(wf, "test.png", &[0x89, 0x50, 0x4E, 0x47]);
        let img_str = img.to_str().unwrap();

        let v = json!({ "type": "image_path", "path": img_str });
        let part = resolve_content_part(&v, Some(wf), 10_000_000).unwrap();

        match part {
            ContentPart::ImageBase64 { media_type, data } => {
                assert_eq!(media_type, "image/png");
                assert!(!data.is_empty());
            }
            other => panic!("expected ImageBase64, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_audio_path() {
        let tmp = tempfile::tempdir().unwrap();
        let wf = tmp.path();
        let audio = write_tmp(wf, "clip.mp3", &[0x49, 0x44, 0x33]);
        let audio_str = audio.to_str().unwrap();

        let v = json!({ "type": "audio_path", "path": audio_str });
        let part = resolve_content_part(&v, Some(wf), 10_000_000).unwrap();

        match part {
            ContentPart::AudioBase64 { media_type, data } => {
                assert_eq!(media_type, "audio/mpeg");
                assert!(!data.is_empty());
            }
            other => panic!("expected AudioBase64, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_pdf_path() {
        let tmp = tempfile::tempdir().unwrap();
        let wf = tmp.path();
        let pdf = write_tmp(wf, "doc.pdf", b"%PDF-1.4 test");
        let pdf_str = pdf.to_str().unwrap();

        let v = json!({ "type": "pdf_path", "path": pdf_str });
        let part = resolve_content_part(&v, Some(wf), 10_000_000).unwrap();

        match part {
            ContentPart::PdfBase64 { data } => {
                assert!(!data.is_empty());
            }
            other => panic!("expected PdfBase64, got {other:?}"),
        }
    }

    #[test]
    fn test_resolve_path_outside_working_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let wf = tmp.path();

        let v = json!({ "type": "image_path", "path": "../../../etc/passwd" });
        let result = resolve_content_part(&v, Some(wf), 10_000_000);
        assert!(result.is_err(), "should reject path outside wf");
    }

    #[test]
    fn test_resolve_path_file_too_large() {
        let tmp = tempfile::tempdir().unwrap();
        let wf = tmp.path();
        let img = write_tmp(wf, "big.png", &[0u8; 100]);

        let v = json!({ "type": "image_path", "path": img.to_str().unwrap() });
        let result = resolve_content_part(&v, Some(wf), 50);
        assert!(result.is_err(), "should reject file > max_file_size");
    }

    #[test]
    fn test_resolve_path_no_working_folder() {
        let v = json!({ "type": "image_path", "path": "test.png" });
        let result = resolve_content_part(&v, None, 10_000_000);
        assert!(result.is_err(), "should error without working folder");
    }

    /// Ensure the tool is usable (compile-time check that make_tool_with_wf links).
    #[allow(dead_code)]
    fn _smoke(wf: &std::path::Path) {
        let _ = make_tool_with_wf(wf);
    }
}

// ── E2E: real LLM call via LlmTool ──────────────────────────────────────
// Run with:  cargo test -p tool-experimental test_e2e_deepseek_say_hello -- --ignored --nocapture

fn make_e2e_tool(model: &str) -> LlmTool {
    let base_url =
        std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.deepseek.com/v1".into());
    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");

    let data = LlmToolData {
        default_provider: "e2e".to_string(),
        default_model: model.to_string(),
        providers: vec![LlmProviderData {
            name: "e2e".to_string(),
            base_url,
            api_key,
            models: vec![make_model(model)],
        }],
    };
    LlmTool::new(Arc::new(data), None, LlmToolConfig::default())
}

#[tokio::test]
#[ignore = "e2e: requires real API key"]
async fn test_e2e_deepseek_say_hello() {
    let model = std::env::var("TEST_MODEL").unwrap_or_else(|_| "glm-5".into());
    let tool = make_e2e_tool(&model);
    let result = tool
        .call(
            json!({
                "model": model,
                "messages": [
                    { "role": "user", "content": "Say hello in one sentence." }
                ],
                "temperature": 0.0,
                "max_tokens": 100,
            }),
            None,
        )
        .await;

    match &result {
        Ok(content) => {
            let s = format!("{:?}", content);
            println!("OK LlmTool invoke succeeded (model={}):\n{}", model, s);
            assert!(!s.is_empty(), "response content should not be empty");
        }
        Err(e) => {
            let msg = format!("{}", e);
            eprintln!("FAIL LlmTool invoke failed (model={}): {}", model, msg);
            panic!("LLM call failed: {}", msg);
        }
    }
}
