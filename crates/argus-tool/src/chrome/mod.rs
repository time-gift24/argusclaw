pub mod error;
pub mod models;
pub mod policy;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::models::{ChromeAction, ChromeToolArgs};
    use super::policy::ExplorePolicy;

    #[test]
    fn open_requires_url() {
        let err = ChromeToolArgs::validate(json!({ "action": "open" })).unwrap_err();
        assert!(err.to_string().contains("url"));
    }

    #[test]
    fn click_is_rejected_by_policy() {
        let err = ExplorePolicy::readonly()
            .validate_action(ChromeAction::Click)
            .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
    }

    #[test]
    fn list_links_is_allowed() {
        ExplorePolicy::readonly()
            .validate_action(ChromeAction::ListLinks)
            .unwrap();
    }
}
