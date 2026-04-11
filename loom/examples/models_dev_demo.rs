//! Example: Using models.dev complete metadata
//!
//! Run with: cargo run --example models_dev_demo

use loom::model_spec::{ModelLimitResolver, ModelsDevResolver};

#[tokio::main]
async fn main() {
    println!("=== Models.dev Complete Metadata Demo ===\n");

    let resolver = ModelsDevResolver::new();

    // Example 1: Query model with complete information
    println!("1. Query Claude 3.5 Sonnet:");
    if let Some(spec) = resolver
        .resolve("anthropic", "claude-3-5-sonnet-20241022")
        .await
    {
        println!("   Context limit: {} tokens", spec.context_limit);
        println!("   Output limit: {} tokens", spec.output_limit);
        println!("   Supports vision: {}", spec.supports_vision());
        println!("   Supports audio: {}", spec.supports_audio());
        println!("   Supports video: {}", spec.supports_video());
        println!("   Supports PDF: {}", spec.supports_pdf());
        println!("   Supports tool call: {}", spec.supports_tool_call());

        if let Some(cost) = spec.estimate_cost(100_000, 10_000) {
            println!("   Estimated cost (100K in, 10K out): ${:.4}", cost);
        }
    }

    // Example 2: Query Gemini 2.0 Flash
    println!("\n2. Query Gemini 2.0 Flash:");
    if let Some(model) = resolver.fetch_model("google", "gemini-2.0-flash").await {
        println!("   Model: {}", model.name);
        println!("   Family: {:?}", model.family);
        println!(
            "   Context: {} tokens",
            model.limit.as_ref().map(|l| l.context).unwrap_or(0)
        );
        println!("   Multimodal support:");
        println!("     - Text: {}", model.modalities.supports_text());
        println!("     - Image: {}", model.modalities.supports_vision());
        println!("     - Audio: {}", model.modalities.supports_audio());
        println!("     - Video: {}", model.modalities.supports_video());
        println!("     - PDF: {}", model.modalities.supports_pdf());
    }

    // Example 3: List all vision-capable models
    println!("\n3. Finding vision-capable models...");
    if let Ok(providers) = resolver.fetch_all_providers().await {
        let mut vision_count: usize = 0;
        let mut audio_count: usize = 0;
        let mut video_count: usize = 0;

        for (provider_id, provider) in providers.iter().take(5) {
            for (model_id, model) in &provider.models {
                if model.modalities.supports_vision() {
                    vision_count += 1;
                    if vision_count <= 3 {
                        println!(
                            "   📷 {} - {}/{} ({} tokens context)",
                            model.name,
                            provider_id,
                            model_id,
                            model.limit.as_ref().map(|l| l.context).unwrap_or(0)
                        );
                    }
                }
                if model.modalities.supports_audio() {
                    audio_count += 1;
                }
                if model.modalities.supports_video() {
                    video_count += 1;
                }
            }
        }

        println!(
            "   ... and {} more vision models",
            vision_count.saturating_sub(3)
        );
        println!(
            "   Total: {} vision, {} audio, {} video models",
            vision_count, audio_count, video_count
        );
    }

    println!("\n=== Demo Complete ===");
}
