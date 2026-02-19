use loom::{
    memory::{LanceStore, Store},
    Embedder, Namespace, OpenAIEmbedder,
};
use serde_json::json;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("OpenAI Embeddings Example");

    println!("\nStep 1: Create embedder");
    let embedder = Arc::new(OpenAIEmbedder::new("text-embedding-3-small"));
    println!("Embedder created with model: text-embedding-3-small");
    println!("Embedding dimension: {}", embedder.dimension());

    println!("\nStep 2: Embed some text");
    let texts = vec![
        "Hello, world!",
        "The quick brown fox jumps over the lazy dog",
    ];
    let texts_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let vectors = embedder.embed(&texts_refs).await?;

    for (i, (text, vector)) in texts.iter().zip(vectors.iter()).enumerate() {
        let vector_len = vector.len();
        println!("  [{}] '{}' -> vector length: {}", i + 1, text, vector_len);
        let preview_len = 5.min(vector_len);
        println!("      First 5 values: {:.4?}", &vector[..preview_len]);
    }

    println!("\nStep 3: Create LanceStore with embeddings");
    let db_path = "data/embeddings.lance";
    let store = LanceStore::new(db_path, embedder).await?;
    println!("LanceStore created at: {}", db_path);

    println!("\nStep 4: Store some memories with embeddings");
    let ns: Namespace = vec!["user-123".to_string(), "memories".to_string()];

    store
        .put(
            &ns,
            "memory-1",
            &json!({"text": "User loves Rust programming language"}),
        )
        .await?;
    store
        .put(
            &ns,
            "memory-2",
            &json!({"text": "User enjoys hiking and outdoor activities"}),
        )
        .await?;
    store
        .put(
            &ns,
            "memory-3",
            &json!({"text": "User is interested in machine learning"}),
        )
        .await?;

    println!("  Stored 3 memories");

    println!("\nStep 5: Search memories using semantic similarity");
    let query = "programming languages";
    println!("  Query: '{}", query);

    let results = store.search(&ns, Some(query), Some(3)).await?;

    println!("  Found {} results:", results.len());
    for (i, hit) in results.iter().enumerate() {
        println!("\n  [{}] Key: {}", i + 1, hit.key);
        println!("      Score: {:.4}", hit.score.unwrap_or(0.0));
        println!("      Value: {}", hit.value);
    }

    println!("\nExample completed successfully!");
    Ok(())
}
