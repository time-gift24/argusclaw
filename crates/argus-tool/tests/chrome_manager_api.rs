use argus_tool::{ChromeManager, ChromeToolError};

#[allow(dead_code)]
async fn external_with_webdriver_callback_shape(
    manager: &ChromeManager,
) -> Result<String, ChromeToolError> {
    manager
        .with_webdriver(|driver| {
            Box::pin(async move {
                driver
                    .current_url()
                    .await
                    .map(|url| url.to_string())
                    .map_err(|error| ChromeToolError::PageReadFailed {
                        reason: error.to_string(),
                    })
            })
        })
        .await
}

#[test]
fn chrome_tool_error_is_public_for_external_callbacks() {
    let error = ChromeToolError::SharedSessionUnavailable;
    assert_eq!(
        error.to_string(),
        "shared browser session is unavailable; run navigate(url) first"
    );
}
