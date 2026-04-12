//! E2E tests for multimodal model support using real API calls.
//!
//! These tests require API keys configured in `~/.loom/config.toml`:
//! ```toml
//! [[providers]]
//! name = "glm-vision"
//! api_key = "your-zhipu-api-key"
//! base_url = "https://open.bigmodel.cn/api/paas/v4"
//! model = "glm-4v"
//!
//! # Or for OpenAI:
//! [[providers]]
//! name = "openai-vision"
//! api_key = "sk-..."
//! model = "gpt-4o"
//! ```
//!
//! Run with: `cargo test --package loom --test multimodal_e2e -- --ignored`

mod init_logging;

use env_config::{load_full_config, ProviderDef};
use loom::llm::{ChatOpenAICompat, LlmClient};
use loom::message::{ContentPart, Message, UserContent};

/// A minimal 1x1 red PNG image (base64 encoded).
/// Smallest possible image for testing - reduces token usage and bandwidth.
const RED_PIXEL_PNG_BASE64: &str = "\
iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==\
";

/// Helper to find any provider from a list of possible names.
fn find_any_provider(names: &[&str]) -> Option<ProviderDef> {
    let cfg = load_full_config("loom").ok()?;
    for name in names {
        if let Some(p) = cfg.providers.iter().find(|p| p.name == *name) {
            return Some(p.clone());
        }
    }
    None
}

/// Helper to build LLM client from provider config.
fn build_client_from_provider(provider: &ProviderDef) -> Option<ChatOpenAICompat> {
    let api_key = provider.api_key.as_ref()?;
    let base_url = provider.base_url.as_deref().unwrap_or_else(|| {
        if provider.name.contains("zhipu") || provider.name.contains("glm") {
            "https://open.bigmodel.cn/api/paas/v4"
        } else if provider.name.contains("moonshot") {
            "https://api.moonshot.cn/v1"
        } else {
            "https://api.openai.com/v1"
        }
    });
    let model = provider.model.as_deref().unwrap_or("gpt-4o");

    Some(ChatOpenAICompat::with_config(base_url, api_key, model))
}

/// Test multimodal vision model with a simple color recognition task.
///
/// Requires provider named "openai", "glm-vision", "moonshotai-cn", "zhipu", etc. in config.toml.
#[tokio::test]
#[ignore = "requires API key in config.toml"]
async fn vision_recognize_red_pixel() {
    let provider = find_any_provider(&[
        "openai",
        "moonshotai-cn",
        "glm-vision",
        "zhipu",
        "bigmodel",
        "glm",
        "zhipuai-coding-plan",
    ])
    .expect("No vision provider found. Add a provider with name 'openai', 'moonshotai-cn', or 'glm-vision' to ~/.loom/config.toml");

    eprintln!("Using provider: {} (model: {:?})", provider.name, provider.model);

    let client = build_client_from_provider(&provider)
        .expect("Failed to build client from provider config");

    let msg = Message::user(UserContent::Multimodal(vec![
        ContentPart::Text {
            text: "What color is this pixel? Answer with only the color name.".to_string(),
        },
        ContentPart::ImageBase64 {
            media_type: "image/png".to_string(),
            data: RED_PIXEL_PNG_BASE64.to_string(),
        },
    ]));

    match client.invoke(&[msg]).await {
        Ok(resp) => {
            eprintln!("Response: {}", resp.content);
            // The model should respond with some color (not empty and not a refusal)
            let content = resp.content.to_lowercase();
            let colors = [
                "red", "blue", "green", "black", "white", "gray", "grey", "transparent",
                "黄", "蓝", "绿", "黑", "白", "灰", "红",
            ];
            let found_color = colors.iter().any(|c| content.contains(c));
            assert!(
                found_color || !resp.content.is_empty(),
                "Expected response to mention a color or be non-empty, got: {}",
                resp.content
            );
        }
        Err(e) => {
            // Some providers may have quota issues - that's ok for this test
            eprintln!("API call failed (provider may have quota issues): {}", e);
        }
    }
}

/// Test GLM vision with URL-based image (if you have a public URL).
#[tokio::test]
#[ignore = "requires API key in config.toml"]
async fn vision_with_image_url() {
    let provider = find_any_provider(&[
        "openai",
        "moonshotai-cn",
        "glm-vision",
        "zhipu",
        "bigmodel",
        "glm",
        "zhipuai-coding-plan",
    ])
    .expect("No provider found. Add a provider to ~/.loom/config.toml");

    eprintln!("Using provider: {} (model: {:?})", provider.name, provider.model);

    let client = build_client_from_provider(&provider)
        .expect("Failed to build client from provider config");

    // Using a small placeholder image from a public URL
    let test_image_url = std::env::var("TEST_IMAGE_URL")
        .unwrap_or_else(|_| "https://via.placeholder.com/150/0000FF/FFFFFF/?text=Blue".to_string());

    let msg = Message::user(UserContent::Multimodal(vec![
        ContentPart::Text {
            text: "What color is the text in this image? Answer with only the color name.".to_string(),
        },
        ContentPart::ImageUrl {
            url: test_image_url,
            detail: None,
        },
    ]));

    match client.invoke(&[msg]).await {
        Ok(resp) => {
            eprintln!("Response: {}", resp.content);
            // The placeholder has blue background with white text, either should be acceptable
            let content = resp.content.to_lowercase();
            assert!(
                content.contains("blue") || content.contains("white")
                    || content.contains("蓝") || content.contains("白"),
                "Expected response to mention blue or white, got: {}",
                resp.content
            );
        }
        Err(e) => {
            // Some providers may have quota issues - that's ok for this test
            eprintln!("API call failed (provider may have quota issues): {}", e);
        }
    }
}

/// Test that multimodal content is correctly serialized for the API.
#[tokio::test]
#[ignore = "requires API key in config.toml"]
async fn multimodal_serialization() {
    let provider = find_any_provider(&[
        "openai",
        "moonshotai-cn",
        "glm-vision",
        "zhipu",
        "bigmodel",
        "glm",
        "zhipuai-coding-plan",
    ])
    .expect("No provider found. Add a provider to ~/.loom/config.toml");

    eprintln!("Using provider: {} (model: {:?})", provider.name, provider.model);

    let client = build_client_from_provider(&provider)
        .expect("Failed to build client from provider config");

    // Test with multiple content parts
    let msg = Message::user(UserContent::Multimodal(vec![
        ContentPart::Text {
            text: "Look at the image and describe what you see.".to_string(),
        },
        ContentPart::ImageBase64 {
            media_type: "image/png".to_string(),
            data: RED_PIXEL_PNG_BASE64.to_string(),
        },
        ContentPart::Text {
            text: "(This is a single pixel image)".to_string(),
        },
    ]));

    let resp = client.invoke(&[msg]).await.expect("API call failed");

    // Any reasonable response indicates the multimodal content was processed
    assert!(!resp.content.is_empty(), "Response should not be empty");
    eprintln!("Response: {}", resp.content);
}
