#[path = "../llm/mod.rs"]
pub mod llm;
#[cfg(feature = "openai-compatible")]
pub mod providers;

#[must_use]
pub fn greeting() -> &'static str {
    "Hello, world!"
}
