pub mod error;
pub mod models;
pub mod policy;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::error::ChromeToolError;
    use super::models::{ChromeAction, ChromeToolArgs};
    use super::policy::ExplorePolicy;

    #[test]
    fn open_requires_url() {
        let err = ChromeToolArgs::validate(json!({ "action": "open" })).unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::MissingRequiredField { action, field }
            if action == "open" && field == "url"
        ));
    }

    #[test]
    fn open_rejects_malformed_url() {
        let err =
            ChromeToolArgs::validate(json!({ "action": "open", "url": "not-a-url" })).unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn open_rejects_non_http_scheme() {
        let err =
            ChromeToolArgs::validate(json!({ "action": "open", "url": "file:///tmp/a.txt" }))
                .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err =
            ChromeToolArgs::validate(json!({ "action": "open", "url": "chrome://settings" }))
                .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn open_rejects_local_or_private_targets() {
        let err =
            ChromeToolArgs::validate(json!({ "action": "open", "url": "https://localhost/path" }))
                .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err =
            ChromeToolArgs::validate(json!({ "action": "open", "url": "https://127.0.0.1/path" }))
                .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err = ChromeToolArgs::validate(json!({ "action": "open", "url": "http://10.0.0.1" }))
            .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err = ChromeToolArgs::validate(json!({ "action": "open", "url": "http://0.0.0.0" }))
            .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn open_stores_trimmed_validated_url() {
        let args = ChromeToolArgs::validate(
            json!({ "action": "open", "url": "  https://example.com/path?q=1  " }),
        )
        .unwrap();

        assert_eq!(args.url.as_deref(), Some("https://example.com/path?q=1"));
    }

    #[test]
    fn click_rejects_url_argument() {
        let err = ChromeToolArgs::validate(json!({
            "action": "click",
            "url": "https://example.com"
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn deny_unknown_fields_is_enforced() {
        let err =
            ChromeToolArgs::validate(json!({ "action": "wait", "unexpected": "value" })).unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn click_is_rejected_by_policy() {
        let err = ExplorePolicy::readonly()
            .validate_action(ChromeAction::Click)
            .unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::ActionNotAllowed { action } if action == "click"
        ));
    }

    #[test]
    fn list_links_is_allowed() {
        ExplorePolicy::readonly()
            .validate_action(ChromeAction::ListLinks)
            .unwrap();
    }
}
