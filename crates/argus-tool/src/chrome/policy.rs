use std::collections::HashSet;

use super::error::ChromeToolError;
use super::models::ChromeAction;

pub struct ExplorePolicy {
    allowed: HashSet<ChromeAction>,
}

impl ExplorePolicy {
    #[must_use]
    pub fn readonly() -> Self {
        let allowed = HashSet::from([
            ChromeAction::Install,
            ChromeAction::Navigate,
            ChromeAction::Refresh,
            ChromeAction::Close,
            ChromeAction::Wait,
            ChromeAction::ExtractText,
            ChromeAction::NewTab,
            ChromeAction::SwitchTab,
            ChromeAction::CloseTab,
            ChromeAction::ListTabs,
        ]);
        Self { allowed }
    }

    #[must_use]
    pub fn interactive() -> Self {
        let allowed = HashSet::from([
            ChromeAction::Install,
            ChromeAction::Navigate,
            ChromeAction::Refresh,
            ChromeAction::Close,
            ChromeAction::Wait,
            ChromeAction::ExtractText,
            ChromeAction::Click,
            ChromeAction::Type,
            ChromeAction::GetUrl,
            ChromeAction::GetCookies,
            ChromeAction::NewTab,
            ChromeAction::SwitchTab,
            ChromeAction::CloseTab,
            ChromeAction::ListTabs,
        ]);
        Self { allowed }
    }

    pub fn validate_action(&self, action: ChromeAction) -> Result<(), ChromeToolError> {
        if self.allowed.contains(&action) {
            Ok(())
        } else {
            Err(ChromeToolError::ActionNotAllowed {
                action: action.as_str().to_string(),
            })
        }
    }
}
