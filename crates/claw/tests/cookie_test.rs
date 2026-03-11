use claw::cookie::Cookie;
use claw::tool::{NamedTool, ToolManager};

#[test]
fn test_cookie_tool_registered() {
    let manager = ToolManager::new();
    let tool = manager.get("cookie_extractor");
    assert!(tool.is_some());
}

#[test]
fn test_cookie_tool_definition() {
    use claw::tool::cookie::CookieTool;
    let tool = CookieTool;
    let def = tool.definition();

    assert_eq!(def.name, "cookie_extractor");
    assert!(def.description.contains("CDP"));

    let schema = def.parameters;
    assert!(schema["properties"]["cdpUrl"].is_object());
    assert!(schema["properties"]["domain"].is_object());
}

#[test]
fn test_cookie_serialization() {
    let cookie_json = r#"{
        "name": "test",
        "value": "value123",
        "domain": ".example.com",
        "path": "/",
        "secure": true,
        "httpOnly": false
    }"#;

    let cookie: Cookie = serde_json::from_str(cookie_json).unwrap();
    assert_eq!(cookie.name, "test");
    assert_eq!(cookie.value, "value123");
    assert_eq!(cookie.domain, ".example.com");
}

// Note: Live CDP test requires Chrome running
// Run manually with: cargo test --test cookie_test test_live_cookies -- --ignored
#[test]
#[ignore = "requires Chrome with --remote-debugging-port=9222"]
fn test_live_cookies() {
    use tokio::runtime::Runtime;

    let _rt = Runtime::new().unwrap();

    // Get first available CDP session
    // This requires Chrome to be running with --remote-debugging-port=9222
    // and at least one tab open

    // Skip if no Chrome available
    // Example:
    // let cdp_url = "ws://localhost:9222/devtools/page/...";
    // let cookies = rt.block_on(get_cookies(cdp_url, "example.com")).unwrap();
    // assert!(!cookies.is_empty());
}
