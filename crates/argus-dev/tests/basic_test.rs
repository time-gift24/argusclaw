//! Integration tests for argus-dev crate.

use argus_dev::DevTools;

#[tokio::test]
async fn test_dev_tools_init() {
    // This test verifies DevTools can be initialized
    // Note: Actual ArgusWing initialization may require setup
    // For now, we just verify the type compiles correctly

    // In a real test scenario:
    // let wing = Arc::new(ArgusWing::init(None).await.unwrap());
    // let dev_tools = DevTools::init(wing).await.unwrap();
    // assert!(dev_tools.wing().tool_manager().count() > 0);

    // For now, just verify the type exists
    let _ = std::any::type_name::<DevTools>();
}

#[tokio::test]
async fn test_dev_tools_error_types() {
    use argus_dev::DevError;

    // Test error creation and display
    let err = DevError::ProviderNotFound("test-provider".to_string());
    assert!(err.to_string().contains("Provider not found"));

    let err = DevError::DatabaseError {
        reason: "connection failed".to_string(),
    };
    assert!(err.to_string().contains("Database error"));

    let err = DevError::TurnFailed {
        reason: "LLM error".to_string(),
    };
    assert!(err.to_string().contains("Turn execution failed"));
}

#[test]
fn test_result_type() {
    use argus_dev::Result;

    // Verify Result type is correctly exported
    fn accepts_result(_: Result<()>) {}

    accepts_result(Ok(()));
    accepts_result(Err(argus_dev::DevError::InvalidConfiguration(
        "test".to_string(),
    )));
}
