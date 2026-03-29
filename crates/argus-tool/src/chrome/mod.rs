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
    fn click_rejects_url_argument() {
        let err = ChromeToolArgs::validate(json!({
            "action": "click",
            "url": "https://example.com"
        }))
        .unwrap_err();
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
