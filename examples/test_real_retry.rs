//! Test program to demonstrate retry behavior with real provider
use argus_llm::providers::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};
use argus_llm::retry::{RetryConfig, RetryProvider};
use argus_llm::test_utils::TestRetryProvider;
use argus_protocol::llm::{ChatMessage, CompletionRequest, LlmProvider};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create real provider from config
    let base_url = std::env::var("ARGUS_LLM_BASE_URL")
        .unwrap_or_else(|_| "https://open.bigmodel.cn/api/paas/v4".to_string());
    let api_key = std::env::var("ARGUS_LLM_API_KEY")
        .unwrap_or_else(|_| "fb7e67c50762405c8a8e134449af6442.FcifS4oZxuQIkCvQ".to_string());
    let model = std::env::var("ARGUS_LLM_MODEL").unwrap_or_else(|_| "glm-4.7".to_string());

    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║   Test Retry Behavior with Real Provider                 ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    println!("Model: {}", model);
    println!("Provider pattern: Success → Fail × 3 → Success");
    println!();

    // Create real provider
    let openai_config = OpenAiCompatibleConfig::new(base_url, api_key, model);
    let factory_config = OpenAiCompatibleFactoryConfig::new(openai_config);
    let base_provider = create_openai_compatible_provider(factory_config)?;

    // Wrap with test retry behavior
    let test_provider = Arc::new(TestRetryProvider::new(base_provider));
    let retry_provider = Arc::new(RetryProvider::new(
        test_provider,
        RetryConfig { max_retries: 3 },
    ));

    // Make 5 calls to demonstrate the pattern
    for i in 1..=5 {
        println!("┌────────────────────────────────────────────────────────────┐");
        println!(
            "│ Call {}                                                           │",
            i
        );
        println!("└────────────────────────────────────────────────────────────┘");

        let request = CompletionRequest::new(vec![ChatMessage::user(format!(
            "Reply with exactly 'Call {} completed'",
            i
        ))]);

        match retry_provider.complete(request).await {
            Ok(response) => {
                println!("✅ Success!");
                println!("Response: {}", response.content.as_deref().unwrap_or("").trim());
            }
            Err(e) => {
                println!("❌ Failed: {}", e);
            }
        }
        println!();
    }

    println!("╔════════════════════════════════════════════════════════════╗");
    println!("║   Test Complete                                             ║");
    println!("╚════════════════════════════════════════════════════════════╝");
    println!();
    println!("Expected pattern:");
    println!("  Call 1: ✅ Success (no retry)");
    println!("  Call 2: 🔄 Retry × 3 then fail");
    println!("  Call 3: 🔄 Retry × 3 then fail");
    println!("  Call 4: 🔄 Retry × 3 then fail");
    println!("  Call 5: ✅ Success (no retry)");

    Ok(())
}
