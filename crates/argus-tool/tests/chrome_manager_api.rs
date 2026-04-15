use argus_tool::{ChromeManager, ChromeToolError};

#[allow(dead_code)]
async fn external_element_text_query_shape(
    manager: &ChromeManager,
) -> Result<String, ChromeToolError> {
    manager.element_text("main").await
}

#[test]
fn chrome_tool_error_is_public_for_external_query_helpers() {
    let error = ChromeToolError::SharedSessionUnavailable;
    assert_eq!(
        error.to_string(),
        "shared browser session is unavailable; run navigate(url) first"
    );
}
